//! HTTP proxy surface and upstream forwarding.

use std::time::Duration;

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{any, get},
};
use serde_json::{Value, json};

use crate::{config::Settings, schema::sanitize_request_tools};

const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// Shared proxy state.
#[derive(Debug, Clone)]
pub struct AppState {
    settings: Settings,
    http: reqwest::Client,
}

impl AppState {
    /// Build shared state and the upstream HTTP client.
    pub fn new(settings: Settings) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .user_agent(USER_AGENT)
            .build()?;
        Ok(Self { settings, http })
    }

    /// Return the loaded settings.
    pub const fn settings(&self) -> &Settings {
        &self.settings
    }
}

/// Build the local adapter router.
///
/// Every non-health path is forwarded to the upstream with the leading `/v1`
/// stripped, so `/v1/responses`, `/v1/responses/compact`, `/v1/models`, and
/// `/v1/chat/completions` all map onto the same upstream base.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .fallback(any(forward))
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(json!({"ok": true}))
}

#[derive(Debug, thiserror::Error)]
enum ProxyError {
    #[error("failed to read request body")]
    ReadBody(#[from] axum::Error),
    #[error("upstream request failed")]
    Upstream(#[from] reqwest::Error),
    #[error("failed to build response")]
    BuildResponse(#[from] axum::http::Error),
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::ReadBody(_) => StatusCode::BAD_REQUEST,
            Self::Upstream(_) | Self::BuildResponse(_) => StatusCode::BAD_GATEWAY,
        };
        (
            status,
            Json(json!({
                "error": {
                    "type": "codex_kimi_switch_error",
                    "message": self.to_string(),
                }
            })),
        )
            .into_response()
    }
}

async fn forward(State(state): State<AppState>, request: Request) -> Result<Response, ProxyError> {
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, MAX_REQUEST_BYTES)
        .await
        .map_err(ProxyError::ReadBody)?;

    // The only payload rewrite this adapter ever performs: MFJS tool schema
    // normalization on JSON request bodies, and only for Kimi/Moonshot
    // upstreams. Anything unparseable is forwarded byte-identical.
    let is_json = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("json"));
    let mut outgoing = bytes.to_vec();
    if state.settings.should_sanitize() && is_json && !bytes.is_empty() {
        if let Ok(mut payload) = serde_json::from_slice::<Value>(&bytes) {
            let dropped = sanitize_request_tools(&mut payload);
            if !dropped.is_empty() {
                tracing::info!(
                    dropped_types = ?dropped,
                    "dropped tool types unsupported by the upstream"
                );
            }
            if let Ok(serialized) = serde_json::to_vec(&payload) {
                outgoing = serialized;
            }
        }
    }

    let mut headers = HeaderMap::new();
    if let Some(content_type) = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    {
        if let Ok(value) = HeaderValue::from_str(content_type) {
            headers.insert(header::CONTENT_TYPE, value);
        }
    } else if !outgoing.is_empty() {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
    }
    if let Some(accept) = parts
        .headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
    {
        if let Ok(value) = HeaderValue::from_str(accept) {
            headers.insert(header::ACCEPT, value);
        }
    }

    // A key configured on the adapter always wins: it replaces whatever
    // Authorization the client sent, so Codex never needs to hold the real
    // Kimi credential. Without a configured key the client header passes
    // through untouched.
    if let Some(api_key) = state.settings.api_key.as_deref() {
        if let Ok(value) = HeaderValue::from_str(&format!("Bearer {api_key}")) {
            headers.insert(header::AUTHORIZATION, value);
        }
    } else if let Some(authorization) = parts
        .headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        if let Ok(value) = HeaderValue::from_str(authorization) {
            headers.insert(header::AUTHORIZATION, value);
        }
    }

    let url = upstream_url(
        state.settings().upstream_base.as_str(),
        parts.uri.path(),
        parts.uri.query(),
    );
    let upstream = state
        .http
        .request(parts.method, url)
        .headers(headers)
        .body(outgoing)
        .send()
        .await
        .map_err(ProxyError::Upstream)?;

    let mut response = Response::builder().status(upstream.status());
    if let Some(content_type) = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
    {
        response = response.header(header::CONTENT_TYPE, content_type);
    }
    response
        .body(Body::from_stream(upstream.bytes_stream()))
        .map_err(ProxyError::BuildResponse)
}

fn upstream_url(base: &str, path: &str, query: Option<&str>) -> String {
    let base = base.trim_end_matches('/');
    let path = path.strip_prefix("/v1").unwrap_or(path);
    let path = if path.is_empty() { "/" } else { path };
    match query {
        Some(query) if !query.is_empty() => format!("{base}{path}?{query}"),
        _ => format!("{base}{path}"),
    }
}
