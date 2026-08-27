//! Unit tests for Moonshot schema normalization.

use codex_kimi_switch::schema::{sanitize_request_tools, sanitize_tool_parameters};
use serde_json::{Value, json};

#[test]
fn fills_missing_property_types() {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "query": {}
        },
        "required": ["query"]
    });

    sanitize_tool_parameters(&mut schema);

    assert_eq!(
        schema.pointer("/properties/query/type"),
        Some(&json!("string"))
    );
}

#[test]
fn converts_nullable_type_array_to_anyof() {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "mode": {"type": ["string", "null"]}
        },
        "required": []
    });

    sanitize_tool_parameters(&mut schema);

    assert_eq!(
        schema.pointer("/properties/mode/anyOf/0/type"),
        Some(&json!("string"))
    );
    assert_eq!(
        schema.pointer("/properties/mode/anyOf/1/type"),
        Some(&json!("null"))
    );
    assert!(schema.pointer("/properties/mode/type").is_none());
}

#[test]
fn pushes_parent_type_into_anyof_branches() {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "filter": {
                "type": "object",
                "anyOf": [
                    {"properties": {"name": {"type": "string"}}},
                    {"type": "null"}
                ]
            }
        },
        "required": []
    });

    sanitize_tool_parameters(&mut schema);

    assert_eq!(
        schema.pointer("/properties/filter/anyOf/0/type"),
        Some(&json!("object"))
    );
    assert!(schema.pointer("/properties/filter/type").is_none());
}

#[test]
fn strips_conflicting_siblings_around_ref_and_sets_def_type() {
    let mut schema = json!({
        "type": "object",
        "$defs": {
            "Name": {
                "properties": {
                    "value": {}
                }
            }
        },
        "properties": {
            "name": {
                "$ref": "#/$defs/Name",
                "type": "object",
                "minProperties": 1
            }
        },
        "required": []
    });

    sanitize_tool_parameters(&mut schema);

    assert_eq!(schema.pointer("/$defs/Name/type"), Some(&json!("object")));
    assert_eq!(
        schema.pointer("/$defs/Name/properties/value/type"),
        Some(&json!("string"))
    );
    assert_eq!(
        schema.pointer("/properties/name/$ref"),
        Some(&json!("#/$defs/Name"))
    );
    assert!(schema.pointer("/properties/name/type").is_none());
    assert!(schema.pointer("/properties/name/minProperties").is_none());
}

#[test]
fn request_sanitizer_only_rewrites_tool_parameters() {
    let mut body = json!({
        "model": "kimi-k2",
        "stream": true,
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
        }],
        "messages": [{"role": "user", "content": "hi"}]
    });

    let dropped = sanitize_request_tools(&mut body);

    assert!(dropped.is_empty());
    assert_eq!(
        body.pointer("/tools/0/function/parameters/properties/query/type"),
        Some(&json!("string"))
    );
    assert_eq!(body.pointer("/model"), Some(&json!("kimi-k2")));
    assert_eq!(body.pointer("/stream"), Some(&Value::Bool(true)));
}

#[test]
fn drops_tool_types_moonshot_does_not_support() {
    let mut body = json!({
        "model": "kimi-k3",
        "tools": [
            {"type": "function", "name": "search", "parameters": {"type": "object", "properties": {"query": {}}, "required": ["query"]}},
            {"type": "tool_search"},
            {"type": "namespace", "name": "mcp", "tools": []},
            {"type": "custom", "name": "apply_patch"}
        ]
    });

    let dropped = sanitize_request_tools(&mut body);

    assert_eq!(dropped, vec!["tool_search", "namespace", "custom"]);
    assert_eq!(body.pointer("/tools/0/name"), Some(&json!("search")));
    assert_eq!(
        body.pointer("/tools/0/parameters/properties/query/type"),
        Some(&json!("string"))
    );
    assert!(body.pointer("/tools/1").is_none());
}

#[test]
fn removes_tools_key_when_every_tool_is_dropped() {
    let mut body = json!({
        "model": "kimi-k3",
        "tools": [{"type": "tool_search"}]
    });

    let dropped = sanitize_request_tools(&mut body);

    assert_eq!(dropped, vec!["tool_search"]);
    assert!(body.get("tools").is_none());
}
