//! Integration test for the local forwarding path.

use std::{sync::mpsc, time::Duration};

use axum::{
    Json, Router,
    extract::{OriginalUri, State},
    http::{HeaderMap, StatusCode},
    routing::any,
};
use codex_kimi_switch::{
    config::Settings,
    proxy::{AppState, build_router},
};
use serde_json::{Value, json};
use tower::ServiceExt;

#[derive(Debug)]
struct Captured {
    path: String,
    authorization: Option<String>,
    body: Value,
}

async fn spawn_upstream(
    tx: mpsc::Sender<Captured>,
) -> Result<std::net::SocketAddr, Box<dyn std::error::Error>> {
    let app = Router::new()
        .fallback(any(
            |State(tx): State<mpsc::Sender<Captured>>,
             OriginalUri(uri): OriginalUri,
             headers: HeaderMap,
             Json(body): Json<Value>| async move {
                let authorization = headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                let _ = tx.send(Captured {
                    path: uri.path().to_owned(),
                    authorization,
                    body,
                });
                Json(json!({"id": "resp_test", "object": "response", "output": []}))
            },
        ))
        .with_state(tx);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let _server = tokio::spawn(async move { axum::serve(listener, app).await });
    Ok(addr)
}

fn request(
    path: &str,
    body: &Value,
) -> Result<axum::http::Request<axum::body::Body>, Box<dyn std::error::Error>> {
    Ok(axum::http::Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .header("authorization", "Bearer incoming-key")
        .body(axum::body::Body::from(serde_json::to_vec(body)?))?)
}

async fn capture_one(
    api_key: Option<&str>,
    request_path: &str,
    body: Value,
) -> Result<Captured, Box<dyn std::error::Error>> {
    let (tx, rx) = mpsc::channel::<Captured>();
    let upstream_addr = spawn_upstream(tx).await?;
    let settings = Settings {
        listen_addr: "127.0.0.1:0".to_owned(),
        upstream_base: format!("http://{upstream_addr}/kimi/coding/v1"),
        api_key: api_key.map(str::to_owned),
        sanitize_always: false,
    };
    let app = build_router(AppState::new(settings)?);
    let outgoing = request(request_path, &body)?;
    let response = ServiceExt::oneshot(app, outgoing).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let captured =
        tokio::task::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(2))).await??;
    Ok(captured)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rewrites_responses_tool_schema_before_forwarding() -> Result<(), Box<dyn std::error::Error>>
{
    let captured = capture_one(
        None,
        "/v1/responses",
        json!({
            "model": "kimi-k3",
            "input": "hi",
            "tools": [{
                "type": "function",
                "name": "search",
                "parameters": {
                    "type": "object",
                    "properties": {"query": {}},
                    "required": ["query"]
                }
            }, {
                "type": "tool_search"
            }]
        }),
    )
    .await?;
    assert_eq!(captured.path, "/kimi/coding/v1/responses");
    assert_eq!(
        captured
            .body
            .pointer("/tools/0/parameters/properties/query/type"),
        Some(&json!("string"))
    );
    assert!(captured.body.pointer("/tools/1").is_none());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rewrites_chat_completions_tool_schema_before_forwarding()
-> Result<(), Box<dyn std::error::Error>> {
    let captured = capture_one(
        None,
        "/v1/chat/completions",
        json!({
            "model": "kimi-k3",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "search",
                    "parameters": {
                        "type": "object",
                        "properties": {"query": {}},
                        "required": ["query"]
                    }
                }
            }]
        }),
    )
    .await?;
    assert_eq!(captured.path, "/kimi/coding/v1/chat/completions");
    assert_eq!(
        captured
            .body
            .pointer("/tools/0/function/parameters/properties/query/type"),
        Some(&json!("string"))
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn incoming_authorization_is_forwarded_when_no_key_configured()
-> Result<(), Box<dyn std::error::Error>> {
    let captured = capture_one(None, "/v1/responses", json!({"model": "kimi-k3"})).await?;
    assert_eq!(
        captured.authorization.as_deref(),
        Some("Bearer incoming-key")
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn configured_api_key_replaces_incoming_authorization()
-> Result<(), Box<dyn std::error::Error>> {
    let captured = capture_one(
        Some("configured-key"),
        "/v1/responses",
        json!({"model": "kimi-k3"}),
    )
    .await?;
    assert_eq!(
        captured.authorization.as_deref(),
        Some("Bearer configured-key")
    );
    Ok(())
}
