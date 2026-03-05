//! OpenAI Responses API 流式转换模块
//!
//! 实现 Responses API SSE → Anthropic SSE 格式转换。
//!
//! Responses API 使用命名事件 (named events) 的生命周期模型：
//! response.created → output_item.added → content_part.added →
//! output_text.delta → content_part.done → output_item.done → response.completed
//!
//! 与 Chat Completions 的 delta chunk 模型完全不同，需要独立的状态机处理。

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};

/// 创建从 Responses API SSE 到 Anthropic SSE 的转换流
///
/// 状态机跟踪: message_id, current_model, content_index, has_sent_message_start
/// SSE 解析支持 named events (event: + data: 行)
pub fn create_anthropic_sse_stream_from_responses(
    stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut message_id: Option<String> = None;
        let mut current_model: Option<String> = None;
        let mut content_index: u32 = 0;
        let mut has_sent_message_start = false;

        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    buffer.push_str(&text);

                    // SSE 事件由 \n\n 分隔
                    while let Some(pos) = buffer.find("\n\n") {
                        let block = buffer[..pos].to_string();
                        buffer = buffer[pos + 2..].to_string();

                        if block.trim().is_empty() {
                            continue;
                        }

                        // 解析 SSE 块：提取 event: 和 data: 行
                        let mut event_type: Option<String> = None;
                        let mut data_parts: Vec<String> = Vec::new();

                        for line in block.lines() {
                            if let Some(evt) = line.strip_prefix("event: ") {
                                event_type = Some(evt.trim().to_string());
                            } else if let Some(d) = line.strip_prefix("data: ") {
                                data_parts.push(d.to_string());
                            }
                        }

                        if data_parts.is_empty() {
                            continue;
                        }

                        let data_str = data_parts.join("\n");
                        let event_name = event_type.as_deref().unwrap_or("");

                        // 解析 JSON 数据
                        let data: Value = match serde_json::from_str(&data_str) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };

                        log::debug!("[Claude/Responses] <<< SSE event: {event_name}");

                        match event_name {
                            // ================================================
                            // response.created → message_start
                            // ================================================
                            "response.created" => {
                                if let Some(id) = data.get("id").and_then(|i| i.as_str()) {
                                    message_id = Some(id.to_string());
                                }
                                if let Some(model) = data.get("model").and_then(|m| m.as_str()) {
                                    current_model = Some(model.to_string());
                                }

                                has_sent_message_start = true;
                                let event = json!({
                                    "type": "message_start",
                                    "message": {
                                        "id": message_id.clone().unwrap_or_default(),
                                        "type": "message",
                                        "role": "assistant",
                                        "model": current_model.clone().unwrap_or_default(),
                                        "usage": {
                                            "input_tokens": 0,
                                            "output_tokens": 0
                                        }
                                    }
                                });
                                let sse = format!("event: message_start\ndata: {}\n\n",
                                    serde_json::to_string(&event).unwrap_or_default());
                                log::debug!("[Claude/Responses] >>> Anthropic SSE: message_start");
                                yield Ok(Bytes::from(sse));
                            }

                            // ================================================
                            // response.content_part.added → content_block_start (text)
                            // ================================================
                            "response.content_part.added" => {
                                // 确保 message_start 已发送
                                if !has_sent_message_start {
                                    let start_event = json!({
                                        "type": "message_start",
                                        "message": {
                                            "id": message_id.clone().unwrap_or_default(),
                                            "type": "message",
                                            "role": "assistant",
                                            "model": current_model.clone().unwrap_or_default(),
                                            "usage": { "input_tokens": 0, "output_tokens": 0 }
                                        }
                                    });
                                    let sse = format!("event: message_start\ndata: {}\n\n",
                                        serde_json::to_string(&start_event).unwrap_or_default());
                                    yield Ok(Bytes::from(sse));
                                    has_sent_message_start = true;
                                }

                                if let Some(part) = data.get("part") {
                                    if part.get("type").and_then(|t| t.as_str()) == Some("output_text") {
                                        let event = json!({
                                            "type": "content_block_start",
                                            "index": content_index,
                                            "content_block": {
                                                "type": "text",
                                                "text": ""
                                            }
                                        });
                                        let sse = format!("event: content_block_start\ndata: {}\n\n",
                                            serde_json::to_string(&event).unwrap_or_default());
                                        yield Ok(Bytes::from(sse));
                                    }
                                }
                            }

                            // ================================================
                            // response.output_text.delta → content_block_delta (text_delta)
                            // ================================================
                            "response.output_text.delta" => {
                                if let Some(delta) = data.get("delta").and_then(|d| d.as_str()) {
                                    let event = json!({
                                        "type": "content_block_delta",
                                        "index": content_index,
                                        "delta": {
                                            "type": "text_delta",
                                            "text": delta
                                        }
                                    });
                                    let sse = format!("event: content_block_delta\ndata: {}\n\n",
                                        serde_json::to_string(&event).unwrap_or_default());
                                    yield Ok(Bytes::from(sse));
                                }
                            }

                            // ================================================
                            // response.content_part.done → content_block_stop
                            // ================================================
                            "response.content_part.done" => {
                                let event = json!({
                                    "type": "content_block_stop",
                                    "index": content_index
                                });
                                let sse = format!("event: content_block_stop\ndata: {}\n\n",
                                    serde_json::to_string(&event).unwrap_or_default());
                                yield Ok(Bytes::from(sse));
                                content_index += 1;
                            }

                            // ================================================
                            // response.output_item.added (function_call) → content_block_start (tool_use)
                            // ================================================
                            "response.output_item.added" => {
                                if let Some(item) = data.get("item") {
                                    let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                    if item_type == "function_call" {
                                        // 确保 message_start 已发送
                                        if !has_sent_message_start {
                                            let start_event = json!({
                                                "type": "message_start",
                                                "message": {
                                                    "id": message_id.clone().unwrap_or_default(),
                                                    "type": "message",
                                                    "role": "assistant",
                                                    "model": current_model.clone().unwrap_or_default(),
                                                    "usage": { "input_tokens": 0, "output_tokens": 0 }
                                                }
                                            });
                                            let sse = format!("event: message_start\ndata: {}\n\n",
                                                serde_json::to_string(&start_event).unwrap_or_default());
                                            yield Ok(Bytes::from(sse));
                                            has_sent_message_start = true;
                                        }

                                        let call_id = item.get("call_id").and_then(|i| i.as_str()).unwrap_or("");
                                        let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");

                                        let event = json!({
                                            "type": "content_block_start",
                                            "index": content_index,
                                            "content_block": {
                                                "type": "tool_use",
                                                "id": call_id,
                                                "name": name
                                            }
                                        });
                                        let sse = format!("event: content_block_start\ndata: {}\n\n",
                                            serde_json::to_string(&event).unwrap_or_default());
                                        yield Ok(Bytes::from(sse));
                                    }
                                    // message type output_item.added is handled via content_part.added
                                }
                            }

                            // ================================================
                            // response.function_call_arguments.delta → content_block_delta (input_json_delta)
                            // ================================================
                            "response.function_call_arguments.delta" => {
                                if let Some(delta) = data.get("delta").and_then(|d| d.as_str()) {
                                    let event = json!({
                                        "type": "content_block_delta",
                                        "index": content_index,
                                        "delta": {
                                            "type": "input_json_delta",
                                            "partial_json": delta
                                        }
                                    });
                                    let sse = format!("event: content_block_delta\ndata: {}\n\n",
                                        serde_json::to_string(&event).unwrap_or_default());
                                    yield Ok(Bytes::from(sse));
                                }
                            }

                            // ================================================
                            // response.function_call_arguments.done → content_block_stop
                            // ================================================
                            "response.function_call_arguments.done" => {
                                let event = json!({
                                    "type": "content_block_stop",
                                    "index": content_index
                                });
                                let sse = format!("event: content_block_stop\ndata: {}\n\n",
                                    serde_json::to_string(&event).unwrap_or_default());
                                yield Ok(Bytes::from(sse));
                                content_index += 1;
                            }

                            // ================================================
                            // response.completed → message_delta + message_stop
                            // ================================================
                            "response.completed" => {
                                let stop_reason = data.get("status")
                                    .and_then(|s| s.as_str())
                                    .map(|s| match s {
                                        "completed" => "end_turn",
                                        "incomplete" => "max_tokens",
                                        _ => "end_turn",
                                    });

                                let usage_json = data.get("usage").map(|u| json!({
                                    "input_tokens": u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                                    "output_tokens": u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                                }));

                                // Emit message_delta (with usage + stop_reason)
                                let delta_event = json!({
                                    "type": "message_delta",
                                    "delta": {
                                        "stop_reason": stop_reason,
                                        "stop_sequence": null
                                    },
                                    "usage": usage_json
                                });
                                let sse = format!("event: message_delta\ndata: {}\n\n",
                                    serde_json::to_string(&delta_event).unwrap_or_default());
                                log::debug!("[Claude/Responses] >>> Anthropic SSE: message_delta");
                                yield Ok(Bytes::from(sse));

                                // Emit message_stop
                                let stop_event = json!({"type": "message_stop"});
                                let stop_sse = format!("event: message_stop\ndata: {}\n\n",
                                    serde_json::to_string(&stop_event).unwrap_or_default());
                                log::debug!("[Claude/Responses] >>> Anthropic SSE: message_stop");
                                yield Ok(Bytes::from(stop_sse));
                            }

                            // Ignore other events (response.in_progress, output_item.done, etc.)
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    log::error!("Responses stream error: {e}");
                    let error_event = json!({
                        "type": "error",
                        "error": {
                            "type": "stream_error",
                            "message": format!("Stream error: {e}")
                        }
                    });
                    let sse = format!("event: error\ndata: {}\n\n",
                        serde_json::to_string(&error_event).unwrap_or_default());
                    yield Ok(Bytes::from(sse));
                    break;
                }
            }
        }
    }
}
