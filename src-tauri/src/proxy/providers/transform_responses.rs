//! OpenAI Responses API 格式转换模块
//!
//! 实现 Anthropic Messages ↔ OpenAI Responses API 格式转换。
//! Responses API 是 OpenAI 2025 年推出的新一代 API，采用扁平化的 input/output 结构。
//!
//! 与 Chat Completions 的主要差异：
//! - tool_use/tool_result 从 message content 中"提升"为顶层 input item
//! - system prompt 使用 `instructions` 字段而非 system role message
//! - usage 字段命名与 Anthropic 一致 (input_tokens/output_tokens)

use crate::proxy::error::ProxyError;
use serde_json::{json, Value};

/// Anthropic 请求 → OpenAI Responses 请求
pub fn anthropic_to_responses(body: Value) -> Result<Value, ProxyError> {
    let mut result = json!({});

    // NOTE: 模型映射由上游统一处理（proxy::model_mapper），格式转换层只做结构转换。
    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        result["model"] = json!(model);
    }

    // system → instructions (Responses API 使用 instructions 字段)
    if let Some(system) = body.get("system") {
        let instructions = if let Some(text) = system.as_str() {
            text.to_string()
        } else if let Some(arr) = system.as_array() {
            arr.iter()
                .filter_map(|msg| msg.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n\n")
        } else {
            String::new()
        };
        if !instructions.is_empty() {
            result["instructions"] = json!(instructions);
        }
    }

    // messages → input
    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        let input = convert_messages_to_input(msgs)?;
        result["input"] = json!(input);
    }

    // max_tokens → max_output_tokens
    if let Some(v) = body.get("max_tokens") {
        result["max_output_tokens"] = v.clone();
    }

    // 直接透传的参数
    if let Some(v) = body.get("temperature") {
        result["temperature"] = v.clone();
    }
    if let Some(v) = body.get("top_p") {
        result["top_p"] = v.clone();
    }
    if let Some(v) = body.get("stream") {
        result["stream"] = v.clone();
    }

    // stop_sequences → 丢弃 (Responses API 不支持)

    // 转换 tools (过滤 BatchTool)
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let response_tools: Vec<Value> = tools
            .iter()
            .filter(|t| t.get("type").and_then(|v| v.as_str()) != Some("BatchTool"))
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                    "description": t.get("description"),
                    "parameters": super::transform::clean_schema(
                        t.get("input_schema").cloned().unwrap_or(json!({}))
                    )
                })
            })
            .collect();

        if !response_tools.is_empty() {
            result["tools"] = json!(response_tools);
        }
    }

    if let Some(v) = body.get("tool_choice") {
        result["tool_choice"] = v.clone();
    }

    Ok(result)
}

/// 将 Anthropic messages 数组转换为 Responses API input 数组
///
/// 核心转换逻辑：
/// - user/assistant 的 text 内容 → 对应 role 的 message item
/// - tool_use 从 assistant message 中"提升"为独立的 function_call item
/// - tool_result 从 user message 中"提升"为独立的 function_call_output item
/// - thinking blocks → 丢弃
fn convert_messages_to_input(messages: &[Value]) -> Result<Vec<Value>, ProxyError> {
    let mut input = Vec::new();

    for msg in messages {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
        let content = msg.get("content");

        match content {
            // 字符串内容
            Some(Value::String(text)) => {
                let content_type = if role == "assistant" {
                    "output_text"
                } else {
                    "input_text"
                };
                input.push(json!({
                    "role": role,
                    "content": [{ "type": content_type, "text": text }]
                }));
            }

            // 数组内容（多模态/工具调用）
            Some(Value::Array(blocks)) => {
                let mut message_content = Vec::new();

                for block in blocks {
                    let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");

                    match block_type {
                        "text" => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                let content_type = if role == "assistant" {
                                    "output_text"
                                } else {
                                    "input_text"
                                };
                                message_content.push(json!({ "type": content_type, "text": text }));
                            }
                        }

                        "image" => {
                            if let Some(source) = block.get("source") {
                                let media_type = source
                                    .get("media_type")
                                    .and_then(|m| m.as_str())
                                    .unwrap_or("image/png");
                                let data =
                                    source.get("data").and_then(|d| d.as_str()).unwrap_or("");
                                message_content.push(json!({
                                    "type": "input_image",
                                    "image_url": format!("data:{media_type};base64,{data}")
                                }));
                            }
                        }

                        "tool_use" => {
                            // 先刷新已累积的消息内容
                            if !message_content.is_empty() {
                                input.push(json!({
                                    "role": role,
                                    "content": message_content.clone()
                                }));
                                message_content.clear();
                            }

                            // 提升为独立的 function_call item
                            let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("");
                            let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                            let arguments = block.get("input").cloned().unwrap_or(json!({}));

                            input.push(json!({
                                "type": "function_call",
                                "call_id": id,
                                "name": name,
                                "arguments": serde_json::to_string(&arguments).unwrap_or_default()
                            }));
                        }

                        "tool_result" => {
                            // 先刷新已累积的消息内容
                            if !message_content.is_empty() {
                                input.push(json!({
                                    "role": role,
                                    "content": message_content.clone()
                                }));
                                message_content.clear();
                            }

                            // 提升为独立的 function_call_output item
                            let call_id = block
                                .get("tool_use_id")
                                .and_then(|i| i.as_str())
                                .unwrap_or("");
                            let output = match block.get("content") {
                                Some(Value::String(s)) => s.clone(),
                                Some(v) => serde_json::to_string(v).unwrap_or_default(),
                                None => String::new(),
                            };

                            input.push(json!({
                                "type": "function_call_output",
                                "call_id": call_id,
                                "output": output
                            }));
                        }

                        "thinking" => {
                            // 丢弃 thinking blocks（与 openai_chat 一致）
                        }

                        _ => {}
                    }
                }

                // 刷新剩余的消息内容
                if !message_content.is_empty() {
                    input.push(json!({
                        "role": role,
                        "content": message_content
                    }));
                }
            }

            _ => {
                // 无内容或 null
                input.push(json!({ "role": role }));
            }
        }
    }

    Ok(input)
}

/// OpenAI Responses 响应 → Anthropic 响应
pub fn responses_to_anthropic(body: Value) -> Result<Value, ProxyError> {
    let output = body
        .get("output")
        .and_then(|o| o.as_array())
        .ok_or_else(|| ProxyError::TransformError("No output in response".to_string()))?;

    let mut content = Vec::new();

    for item in output {
        let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match item_type {
            "message" => {
                if let Some(msg_content) = item.get("content").and_then(|c| c.as_array()) {
                    for block in msg_content {
                        let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        if block_type == "output_text" {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                if !text.is_empty() {
                                    content.push(json!({"type": "text", "text": text}));
                                }
                            }
                        }
                    }
                }
            }

            "function_call" => {
                let call_id = item.get("call_id").and_then(|i| i.as_str()).unwrap_or("");
                let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args_str = item
                    .get("arguments")
                    .and_then(|a| a.as_str())
                    .unwrap_or("{}");
                let input: Value = serde_json::from_str(args_str).unwrap_or(json!({}));

                content.push(json!({
                    "type": "tool_use",
                    "id": call_id,
                    "name": name,
                    "input": input
                }));
            }

            "reasoning" => {
                // 映射 reasoning summary → thinking block
                if let Some(summary) = item.get("summary").and_then(|s| s.as_array()) {
                    let thinking_text: String = summary
                        .iter()
                        .filter_map(|s| {
                            if s.get("type").and_then(|t| t.as_str()) == Some("summary_text") {
                                s.get("text").and_then(|t| t.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("");

                    if !thinking_text.is_empty() {
                        content.push(json!({
                            "type": "thinking",
                            "thinking": thinking_text
                        }));
                    }
                }
            }

            _ => {}
        }
    }

    // status → stop_reason
    let stop_reason = body
        .get("status")
        .and_then(|s| s.as_str())
        .map(|s| match s {
            "completed" => "end_turn",
            "incomplete" => "max_tokens",
            _ => "end_turn",
        });

    // Usage — Responses API 使用与 Anthropic 相同的字段名
    let usage = body.get("usage").cloned().unwrap_or(json!({}));
    let input_tokens = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let output_tokens = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let result = json!({
        "id": body.get("id").and_then(|i| i.as_str()).unwrap_or(""),
        "type": "message",
        "role": "assistant",
        "content": content,
        "model": body.get("model").and_then(|m| m.as_str()).unwrap_or(""),
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens
        }
    });

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anthropic_to_responses_simple() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input).unwrap();
        assert_eq!(result["model"], "gpt-4o");
        assert_eq!(result["max_output_tokens"], 1024);
        assert_eq!(result["input"][0]["role"], "user");
        assert_eq!(result["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(result["input"][0]["content"][0]["text"], "Hello");
        // stop_sequences should not appear
        assert!(result.get("stop_sequences").is_none());
    }

    #[test]
    fn test_anthropic_to_responses_with_system_string() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "system": "You are a helpful assistant.",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input).unwrap();
        assert_eq!(result["instructions"], "You are a helpful assistant.");
        // system should not appear in input
        assert_eq!(result["input"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_anthropic_to_responses_with_system_array() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "system": [
                {"type": "text", "text": "Part 1"},
                {"type": "text", "text": "Part 2"}
            ],
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input).unwrap();
        assert_eq!(result["instructions"], "Part 1\n\nPart 2");
    }

    #[test]
    fn test_anthropic_to_responses_with_tools() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Weather?"}],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather info",
                "input_schema": {"type": "object", "properties": {"location": {"type": "string"}}}
            }]
        });

        let result = anthropic_to_responses(input).unwrap();
        assert_eq!(result["tools"][0]["type"], "function");
        assert_eq!(result["tools"][0]["name"], "get_weather");
        assert!(result["tools"][0].get("parameters").is_some());
        // input_schema should not appear
        assert!(result["tools"][0].get("input_schema").is_none());
    }

    #[test]
    fn test_anthropic_to_responses_tool_use_lifting() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "Let me check"},
                    {"type": "tool_use", "id": "call_123", "name": "get_weather", "input": {"location": "Tokyo"}}
                ]
            }]
        });

        let result = anthropic_to_responses(input).unwrap();
        let input_arr = result["input"].as_array().unwrap();

        // Should produce: assistant message (text) + function_call item
        assert_eq!(input_arr.len(), 2);

        // First: assistant message with output_text
        assert_eq!(input_arr[0]["role"], "assistant");
        assert_eq!(input_arr[0]["content"][0]["type"], "output_text");
        assert_eq!(input_arr[0]["content"][0]["text"], "Let me check");

        // Second: function_call item (lifted from message)
        assert_eq!(input_arr[1]["type"], "function_call");
        assert_eq!(input_arr[1]["call_id"], "call_123");
        assert_eq!(input_arr[1]["name"], "get_weather");
    }

    #[test]
    fn test_anthropic_to_responses_tool_result_lifting() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "tool_result", "tool_use_id": "call_123", "content": "Sunny, 25°C"}
                ]
            }]
        });

        let result = anthropic_to_responses(input).unwrap();
        let input_arr = result["input"].as_array().unwrap();

        // Should produce: function_call_output item (lifted)
        assert_eq!(input_arr.len(), 1);
        assert_eq!(input_arr[0]["type"], "function_call_output");
        assert_eq!(input_arr[0]["call_id"], "call_123");
        assert_eq!(input_arr[0]["output"], "Sunny, 25°C");
    }

    #[test]
    fn test_anthropic_to_responses_thinking_discarded() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "Let me think..."},
                    {"type": "text", "text": "The answer is 42"}
                ]
            }]
        });

        let result = anthropic_to_responses(input).unwrap();
        let input_arr = result["input"].as_array().unwrap();

        // thinking should be discarded, only text remains
        assert_eq!(input_arr.len(), 1);
        assert_eq!(input_arr[0]["content"][0]["type"], "output_text");
        assert_eq!(input_arr[0]["content"][0]["text"], "The answer is 42");
    }

    #[test]
    fn test_anthropic_to_responses_image() {
        let input = json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "What is this?"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "abc123"}}
                ]
            }]
        });

        let result = anthropic_to_responses(input).unwrap();
        let content = result["input"][0]["content"].as_array().unwrap();

        assert_eq!(content[0]["type"], "input_text");
        assert_eq!(content[1]["type"], "input_image");
        assert_eq!(content[1]["image_url"], "data:image/png;base64,abc123");
    }

    #[test]
    fn test_responses_to_anthropic_simple() {
        let input = json!({
            "id": "resp_123",
            "object": "response",
            "status": "completed",
            "model": "gpt-4o",
            "output": [{
                "type": "message",
                "id": "msg_123",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Hello!"}]
            }],
            "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}
        });

        let result = responses_to_anthropic(input).unwrap();
        assert_eq!(result["id"], "resp_123");
        assert_eq!(result["type"], "message");
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["content"][0]["text"], "Hello!");
        assert_eq!(result["stop_reason"], "end_turn");
        assert_eq!(result["usage"]["input_tokens"], 10);
        assert_eq!(result["usage"]["output_tokens"], 5);
    }

    #[test]
    fn test_responses_to_anthropic_with_function_call() {
        let input = json!({
            "id": "resp_123",
            "object": "response",
            "status": "completed",
            "model": "gpt-4o",
            "output": [{
                "type": "function_call",
                "id": "fc_123",
                "call_id": "call_123",
                "name": "get_weather",
                "arguments": "{\"location\": \"Tokyo\"}",
                "status": "completed"
            }],
            "usage": {"input_tokens": 10, "output_tokens": 15}
        });

        let result = responses_to_anthropic(input).unwrap();
        assert_eq!(result["content"][0]["type"], "tool_use");
        assert_eq!(result["content"][0]["id"], "call_123");
        assert_eq!(result["content"][0]["name"], "get_weather");
        assert_eq!(result["content"][0]["input"]["location"], "Tokyo");
    }

    #[test]
    fn test_responses_to_anthropic_with_reasoning() {
        let input = json!({
            "id": "resp_123",
            "object": "response",
            "status": "completed",
            "model": "gpt-4o",
            "output": [
                {
                    "type": "reasoning",
                    "id": "rs_123",
                    "summary": [
                        {"type": "summary_text", "text": "Thinking about the problem..."}
                    ]
                },
                {
                    "type": "message",
                    "id": "msg_123",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "The answer is 42"}]
                }
            ],
            "usage": {"input_tokens": 10, "output_tokens": 20}
        });

        let result = responses_to_anthropic(input).unwrap();
        // Should have thinking + text
        assert_eq!(result["content"][0]["type"], "thinking");
        assert_eq!(
            result["content"][0]["thinking"],
            "Thinking about the problem..."
        );
        assert_eq!(result["content"][1]["type"], "text");
        assert_eq!(result["content"][1]["text"], "The answer is 42");
    }

    #[test]
    fn test_responses_to_anthropic_incomplete_status() {
        let input = json!({
            "id": "resp_123",
            "status": "incomplete",
            "model": "gpt-4o",
            "output": [{
                "type": "message",
                "content": [{"type": "output_text", "text": "Partial..."}]
            }],
            "usage": {"input_tokens": 10, "output_tokens": 4096}
        });

        let result = responses_to_anthropic(input).unwrap();
        assert_eq!(result["stop_reason"], "max_tokens");
    }

    #[test]
    fn test_model_passthrough() {
        let input = json!({
            "model": "o3-mini",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = anthropic_to_responses(input).unwrap();
        assert_eq!(result["model"], "o3-mini");
    }
}
