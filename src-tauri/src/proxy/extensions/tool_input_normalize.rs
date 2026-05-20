// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/tool-input-normalize.mjs
// 翻译: 2026-05-20
//
// Sorts tool_use.input fields to match schema definitions, removes extra fields.
// This stabilizes the JSON serialization order for cache-friendliness.

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use serde_json::Value;
use std::collections::BTreeMap;

pub struct ToolInputNormalize;

impl ToolInputNormalize {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for ToolInputNormalize {
    fn name(&self) -> &str {
        "tool-input-normalize"
    }
    fn order(&self) -> u32 {
        340
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

impl RequestExtension for ToolInputNormalize {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        let body = &ctx.body;
        let _messages = match body.get("messages").and_then(|v| v.as_array()) {
            Some(m) => m,
            None => return Ok(None),
        };
        let tools = match body.get("tools").and_then(|v| v.as_array()) {
            Some(t) => t,
            None => return Ok(None),
        };

        // Build map: tool_name -> sorted property keys from input_schema.
        let tool_schemas = build_tool_schema_map(tools);

        let mut modified_count = 0u32;
        let messages = ctx.body["messages"].as_array_mut().unwrap();

        for msg in messages.iter_mut() {
            if msg.get("role").and_then(|v| v.as_str()) != Some("assistant") {
                continue;
            }
            let content = match msg.get_mut("content").and_then(|v| v.as_array_mut()) {
                Some(c) => c,
                None => continue,
            };
            for block in content.iter_mut() {
                if block.get("type").and_then(|v| v.as_str()) != Some("tool_use") {
                    continue;
                }
                let tool_name = match block.get("name").and_then(|v| v.as_str()) {
                    Some(n) => n,
                    None => continue,
                };
                let input = match block.get("input") {
                    Some(i) => i,
                    None => continue,
                };
                if !input.is_object() {
                    continue;
                }
                let schema_keys = match tool_schemas.get(tool_name) {
                    Some(keys) => keys,
                    None => continue,
                };

                let new_input = normalize_input(input, schema_keys);
                if let Some(new_input) = new_input {
                    block["input"] = new_input;
                    modified_count += 1;
                }
            }
        }

        if modified_count > 0 {
            ctx.meta.set(
                "toolInputNormalizeCount",
                Value::Number(modified_count.into()),
            );
        }

        Ok(None)
    }
}

/// Build a map from tool name to sorted property keys from its input_schema.
fn build_tool_schema_map(tools: &[Value]) -> BTreeMap<String, Vec<String>> {
    let mut map = BTreeMap::new();
    for tool in tools {
        let name = match tool.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => continue,
        };
        let props = match tool.get("input_schema").and_then(|v| v.get("properties")) {
            Some(p) => p,
            None => continue,
        };
        let obj = match props.as_object() {
            Some(o) => o,
            None => continue,
        };
        let keys: Vec<String> = obj.keys().cloned().collect();
        // Schema keys are already in JSON declaration order thanks to
        // serde_json preserve_order feature.
        map.insert(name.to_string(), keys);
    }
    map
}

/// Normalize tool_use input: reorder to match schema keys, drop extra fields.
/// Returns Some(new_input) if changes were made, None otherwise.
fn normalize_input(input: &Value, schema_keys: &[String]) -> Option<Value> {
    let input_obj = input.as_object()?;

    // Collect current keys that are in schema.
    let current_in_schema: Vec<&String> = input_obj
        .keys()
        .filter(|k| schema_keys.contains(k))
        .collect();

    // Check for extra fields.
    let schema_set: std::collections::HashSet<&String> = schema_keys.iter().collect();
    let has_extras = input_obj.keys().any(|k| !schema_set.contains(k));

    // Check if order differs.
    let present_schema_keys: Vec<&String> =
        schema_keys.iter().filter(|k| input_obj.contains_key(*k)).collect();
    let order_differs = present_schema_keys.len() != current_in_schema.len()
        || present_schema_keys
            .iter()
            .zip(current_in_schema.iter())
            .any(|(a, b)| a != b);

    if !has_extras && !order_differs {
        return None;
    }

    // Rebuild with schema keys in schema order, dropping extras.
    let mut map = serde_json::Map::new();
    for key in &present_schema_keys {
        if let Some(val) = input_obj.get(*key) {
            map.insert((*key).clone(), val.clone());
        }
    }
    Some(Value::Object(map))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_input_reorders_and_drops_extras() {
        let input = json!({"z": 1, "a": 2, "extra": 3});
        let schema_keys = vec!["a".to_string(), "b".to_string(), "z".to_string()];
        let result = normalize_input(&input, &schema_keys);
        assert!(result.is_some());
        let new_input = result.unwrap();
        // Should have keys in schema order: a, z (b is not present, extra is dropped).
        let keys: Vec<&str> = new_input
            .as_object()
            .unwrap()
            .keys()
            .map(|s| s.as_str())
            .collect();
        assert_eq!(keys, vec!["a", "z"]);
        assert_eq!(new_input["a"], json!(2));
        assert_eq!(new_input["z"], json!(1));
    }

    #[test]
    fn normalize_input_no_change_when_already_ordered() {
        let input = json!({"a": 1, "b": 2});
        let schema_keys = vec!["a".to_string(), "b".to_string()];
        let result = normalize_input(&input, &schema_keys);
        assert!(result.is_none());
    }

    #[test]
    fn normalize_input_returns_none_for_non_object() {
        let input = json!("not an object");
        let schema_keys = vec!["a".to_string()];
        assert!(normalize_input(&input, &schema_keys).is_none());
    }

    #[test]
    fn full_pipeline_normalizes_tool_use() {
        let body = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "tool_use", "name": "read", "input": {"z": 1, "a": 2, "extra": 3}}
                ]}
            ],
            "tools": [
                {"name": "read", "input_schema": {"properties": {"a": {}, "b": {}, "z": {}}}}
            ]
        });
        let mut ctx = RequestContext {
            body,
            headers: axum::http::HeaderMap::new(),
            meta: ExtensionMeta::default(),
        };
        let ext = ToolInputNormalize::new();
        ext.on_request(&mut ctx).unwrap();
        let input = &ctx.body["messages"][0]["content"][0]["input"];
        let keys: Vec<&str> = input
            .as_object()
            .unwrap()
            .keys()
            .map(|s| s.as_str())
            .collect();
        assert_eq!(keys, vec!["a", "z"]);
        assert!(input.get("extra").is_none());
    }
}
