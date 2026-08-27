//! Moonshot Flavored JSON Schema normalization for Chat Completions tools.

use serde_json::{Map, Value, json};

const LOCAL_DEFS_PREFIX: &str = "#/$defs/";
const MAX_DEF_PASSES: usize = 32;

/// Normalize the tool list in a request body for Moonshot.
///
/// Moonshot only accepts `type = "function"` tools, so `tool_search`,
/// `custom`, `namespace`, and any other tool types are dropped; if nothing
/// survives, the `tools` key is removed entirely. Each surviving tool's
/// `parameters` schema is normalized. Handles both the Responses API shape
/// (`tools[*].parameters`) and the Chat Completions shape
/// (`tools[*].function.parameters`). Returns the dropped tool types so the
/// caller can log the degradation.
pub fn sanitize_request_tools(body: &mut Value) -> Vec<String> {
    let mut dropped = Vec::new();
    let Some(obj) = body.as_object_mut() else {
        return dropped;
    };
    let Some(tools) = obj.get_mut("tools").and_then(Value::as_array_mut) else {
        return dropped;
    };

    tools.retain(|tool| {
        let typ = tool.get("type").and_then(Value::as_str).unwrap_or("<none>");
        if typ == "function" {
            true
        } else {
            dropped.push(typ.to_owned());
            false
        }
    });

    let now_empty = tools.is_empty();
    for tool in tools.iter_mut() {
        if let Some(parameters) = tool.get_mut("parameters") {
            sanitize_tool_parameters(parameters);
        }
        if let Some(function) = tool.get_mut("function").and_then(Value::as_object_mut) {
            if let Some(parameters) = function.get_mut("parameters") {
                sanitize_tool_parameters(parameters);
            }
        }
    }
    if now_empty {
        obj.remove("tools");
    }
    dropped
}

/// Normalize one tool `parameters` schema into the subset Moonshot accepts.
pub fn sanitize_tool_parameters(schema: &mut Value) {
    if !schema.is_object() {
        *schema = json!({"type": "object", "properties": {}, "required": []});
        return;
    }

    {
        let Some(obj) = schema.as_object_mut() else {
            return;
        };
        if obj.get("type").and_then(Value::as_str) != Some("object") {
            obj.insert("type".to_owned(), json!("object"));
        }
        if !matches!(obj.get("properties"), Some(Value::Object(_))) {
            obj.insert("properties".to_owned(), json!({}));
        }
        if !matches!(obj.get("required"), Some(Value::Array(_))) {
            obj.insert("required".to_owned(), json!([]));
        }
        if let Some(defs) = obj.get_mut("$defs").and_then(Value::as_object_mut) {
            sanitize_defs(defs);
        }
    }

    sanitize_schema_node(schema, Some("object"), None);
}

fn sanitize_defs(defs: &mut Map<String, Value>) {
    for _ in 0..MAX_DEF_PASSES {
        let mut changed = false;
        let keys: Vec<String> = defs.keys().cloned().collect();
        for key in keys {
            let Some(mut def) = defs.remove(&key) else {
                continue;
            };
            let before = def.clone();
            sanitize_schema_node(&mut def, None, Some(defs));
            if def != before {
                changed = true;
            }
            defs.insert(key, def);
        }
        if !changed {
            break;
        }
    }
}

fn sanitize_schema_node(
    node: &mut Value,
    inherited_type: Option<&str>,
    mut defs: Option<&mut Map<String, Value>>,
) {
    let Some(obj) = node.as_object_mut() else {
        return;
    };

    normalize_type_array(obj);
    let explicit_type = obj.get("type").and_then(Value::as_str).map(String::from);
    let effective_type = explicit_type.as_deref().or(inherited_type);

    if let Some(name) = obj.get("$ref").and_then(extract_local_ref_name) {
        if let Some(typ) = effective_type {
            if let Some(defs) = defs.as_deref_mut() {
                ensure_def_type(defs, name, typ);
            }
        }
        obj.retain(|key, _| matches!(key.as_str(), "$ref" | "description" | "title"));
        return;
    }

    let mut parent_had_type = false;
    for combinator in ["anyOf", "oneOf", "allOf"] {
        let Some(branches) = obj.get_mut(combinator).and_then(Value::as_array_mut) else {
            continue;
        };
        if let Some(typ) = effective_type {
            for branch in branches.iter_mut() {
                if let Some(branch_obj) = branch.as_object_mut() {
                    branch_obj
                        .entry("type".to_owned())
                        .or_insert_with(|| json!(typ));
                }
            }
        }
        for branch in branches.iter_mut() {
            sanitize_schema_node(branch, None, defs.as_deref_mut());
        }
        if explicit_type.is_some() {
            parent_had_type = true;
        }
    }
    if parent_had_type {
        obj.remove("type");
    }

    let has_combinator = ["anyOf", "oneOf", "allOf"]
        .iter()
        .any(|key| obj.contains_key(*key));
    if !has_combinator && obj.get("type").is_none() {
        let typ = inherited_type.unwrap_or_else(|| infer_type(obj));
        obj.insert("type".to_owned(), json!(typ));
    }
    if obj.get("type").and_then(Value::as_str) == Some("object") && !obj.contains_key("required") {
        obj.insert("required".to_owned(), json!([]));
    }

    if let Some(properties) = obj.get_mut("properties").and_then(Value::as_object_mut) {
        for property in properties.values_mut() {
            sanitize_schema_node(property, None, defs.as_deref_mut());
        }
    }
    if let Some(items) = obj.get_mut("items") {
        sanitize_schema_node(items, None, defs.as_deref_mut());
    }
    if let Some(prefix_items) = obj.get_mut("prefixItems").and_then(Value::as_array_mut) {
        for item in prefix_items.iter_mut() {
            sanitize_schema_node(item, None, defs.as_deref_mut());
        }
    }
    if let Some(additional) = obj.get_mut("additionalProperties") {
        if additional.is_object() {
            sanitize_schema_node(additional, None, defs);
        }
    }
    if let Some(defs_map) = obj.get_mut("$defs").and_then(Value::as_object_mut) {
        sanitize_defs(defs_map);
    }
}

fn normalize_type_array(obj: &mut Map<String, Value>) {
    let Some(Value::Array(arr)) = obj.get("type") else {
        return;
    };

    let concrete: Vec<&str> = arr
        .iter()
        .filter_map(|value| value.as_str().filter(|typ| *typ != "null"))
        .collect();
    let has_null = arr.iter().any(|value| value.as_str() == Some("null"));

    match (concrete.len(), has_null) {
        (1, false) => {
            if let Some(first) = concrete.first() {
                obj.insert("type".to_owned(), json!(*first));
            }
        }
        (0, true) => {
            obj.remove("type");
        }
        _ => {
            let mut branches: Vec<Value> =
                concrete.iter().map(|typ| json!({"type": *typ})).collect();
            if has_null {
                branches.push(json!({"type": "null"}));
            }
            obj.remove("type");
            obj.insert("anyOf".to_owned(), Value::Array(branches));
        }
    }
}

fn ensure_def_type(defs: &mut Map<String, Value>, name: &str, typ: &str) {
    let Some(mut def) = defs.remove(name) else {
        return;
    };
    if let Some(obj) = def.as_object_mut() {
        let has_combinator = ["anyOf", "oneOf", "allOf"]
            .iter()
            .any(|key| obj.contains_key(*key));
        if obj.get("type").is_none() && !has_combinator {
            obj.insert("type".to_owned(), json!(typ));
        }
    }
    defs.insert(name.to_owned(), def);
}

fn extract_local_ref_name(value: &Value) -> Option<&str> {
    let reference = value.as_str()?;
    if reference == "#" {
        return Some("");
    }
    reference.strip_prefix(LOCAL_DEFS_PREFIX)
}

fn infer_type(obj: &Map<String, Value>) -> &'static str {
    if obj.contains_key("properties")
        || obj.contains_key("required")
        || obj
            .get("additionalProperties")
            .is_some_and(Value::is_object)
    {
        return "object";
    }
    if obj.contains_key("items") || obj.contains_key("prefixItems") {
        return "array";
    }
    if let Some(value) = obj
        .get("enum")
        .and_then(Value::as_array)
        .and_then(|values| values.iter().find(|value| !value.is_null()))
    {
        return match value {
            Value::Bool(_) => "boolean",
            Value::Number(number) if number.as_i64().is_some() => "integer",
            Value::Number(_) => "number",
            Value::Null => "null",
            Value::String(_) | Value::Array(_) | Value::Object(_) => "string",
        };
    }
    if let Some(value) = obj.get("const") {
        return match value {
            Value::Bool(_) => "boolean",
            Value::Number(number) if number.as_i64().is_some() => "integer",
            Value::Number(_) => "number",
            Value::Null => "null",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
            Value::String(_) => "string",
        };
    }
    "string"
}
