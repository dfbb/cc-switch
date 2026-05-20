// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/fresh-session-sort.mjs
// 翻译: 2026-05-20
//
// Relocates scattered blocks (hooks/skills/deferred-tools/MCP) to messages[0]
// in deterministic fresh-session order: deferred -> mcp -> skills -> hooks.
// Also strips /clear artifacts from the first user message.

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

const SR_PREFIX: &str = "<system-reminder>\n";

pub struct FreshSessionSort;

impl FreshSessionSort {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for FreshSessionSort {
    fn name(&self) -> &str {
        "fresh-session-sort"
    }
    fn order(&self) -> u32 {
        250
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

impl RequestExtension for FreshSessionSort {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        let messages = match ctx.body.get_mut("messages") {
            Some(Value::Array(arr)) => arr,
            _ => return Ok(None),
        };

        // Find first user message index
        let first_user_idx = match messages.iter().position(|m| {
            m.get("role").and_then(|v| v.as_str()) == Some("user")
        }) {
            Some(idx) => idx,
            None => return Ok(None),
        };

        // Validate first message has content array
        let first_content = match messages[first_user_idx].get("content") {
            Some(Value::Array(arr)) => arr.clone(),
            _ => return Ok(None),
        };

        // Strip /clear artifacts from first user message
        let before_len = first_content.len();
        let first_content: Vec<Value> = first_content
            .into_iter()
            .filter(|b| !is_clear_artifact(b.get("text").and_then(|v| v.as_str()).unwrap_or("")))
            .collect();

        // Check for scattered relocatable blocks outside first user message
        let mut has_scattered = false;
        for msg in messages.iter().skip(first_user_idx + 1) {
            if msg.get("role").and_then(|v| v.as_str()) != Some("user") {
                continue;
            }
            let content = match msg.get("content") {
                Some(Value::Array(arr)) => arr,
                _ => continue,
            };
            for block in content {
                if is_relocatable_block(
                    block.get("text").and_then(|v| v.as_str()).unwrap_or(""),
                ) {
                    has_scattered = true;
                    break;
                }
            }
            if has_scattered {
                break;
            }
        }

        if !has_scattered {
            // Still sort and pin blocks in-place for deterministic first-call baseline
            let mut modified = false;
            let new_content: Vec<Value> = first_content
                .iter()
                .map(|block| {
                    let text = block.get("text").and_then(|v| v.as_str()).unwrap_or("");
                    let block_type = get_block_type(text);
                    if block_type.is_none() {
                        return block.clone();
                    }

                    let fixed_text = fix_block_text(block_type.unwrap(), text);
                    if fixed_text != text {
                        modified = true;
                        let mut new_block = block.clone();
                        new_block["text"] = Value::String(fixed_text);
                        // Strip cache_control if present
                        if let Some(obj) = new_block.as_object_mut() {
                            obj.remove("cache_control");
                        }
                        return new_block;
                    }
                    block.clone()
                })
                .collect();

            if modified || new_content.len() != before_len {
                messages[first_user_idx]["content"] = Value::Array(new_content);
            }
            return Ok(None);
        }

        // Scan backwards to find latest instance of each relocatable block type
        let mut found: HashMap<&str, Value> = HashMap::new();
        for msg in messages.iter().rev() {
            if msg.get("role").and_then(|v| v.as_str()) != Some("user") {
                continue;
            }
            let content = match msg.get("content") {
                Some(Value::Array(arr)) => arr,
                _ => continue,
            };
            for block in content.iter().rev() {
                let text = block.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let block_type = match get_block_type(text) {
                    Some(t) => t,
                    None => continue,
                };
                if found.contains_key(block_type) {
                    continue;
                }

                let fixed_text = fix_block_text(block_type, text);
                let mut new_block = block.clone();
                new_block["text"] = Value::String(fixed_text);
                // Strip cache_control
                if let Some(obj) = new_block.as_object_mut() {
                    obj.remove("cache_control");
                }
                found.insert(block_type, new_block);
            }
        }

        if found.is_empty() {
            return Ok(None);
        }

        // Remove all relocatable blocks from all user messages
        for msg in messages.iter_mut() {
            if msg.get("role").and_then(|v| v.as_str()) != Some("user") {
                continue;
            }
            let content = match msg.get_mut("content") {
                Some(Value::Array(arr)) => arr,
                _ => continue,
            };
            let filtered: Vec<Value> = content
                .drain(..)
                .filter(|b| {
                    !is_relocatable_block(
                        b.get("text").and_then(|v| v.as_str()).unwrap_or(""),
                    )
                })
                .collect();
            *content = filtered;
        }

        // Prepend in deterministic order: deferred -> mcp -> skills -> hooks
        const ORDER: [&str; 4] = ["deferred", "mcp", "skills", "hooks"];
        let mut to_relocate: Vec<Value> = Vec::new();
        for block_type in &ORDER {
            if let Some(block) = found.remove(*block_type) {
                to_relocate.push(block);
            }
        }

        // Prepend to first user message content
        let existing = match messages[first_user_idx].get_mut("content") {
            Some(Value::Array(arr)) => arr,
            _ => return Ok(None),
        };
        let mut new_content: Vec<Value> = to_relocate;
        new_content.append(existing);
        *existing = new_content;

        Ok(None)
    }
}

// --- Helper functions ---

fn is_system_reminder(text: &str) -> bool {
    text.starts_with("<system-reminder>")
}

fn is_hooks_block(text: &str) -> bool {
    is_system_reminder(text)
        && text
            .chars()
            .take(200)
            .collect::<String>()
            .contains("hook success")
}

fn is_skills_block(text: &str) -> bool {
    text.starts_with(&format!("{}The following skills are available", SR_PREFIX))
}

fn is_deferred_tools_block(text: &str) -> bool {
    text.starts_with(&format!(
        "{}The following deferred tools are now available",
        SR_PREFIX
    ))
}

fn is_mcp_block(text: &str) -> bool {
    text.starts_with(&format!("{}# MCP Server Instructions", SR_PREFIX))
}

fn is_relocatable_block(text: &str) -> bool {
    is_hooks_block(text)
        || is_skills_block(text)
        || is_deferred_tools_block(text)
        || is_mcp_block(text)
}

fn is_clear_artifact(text: &str) -> bool {
    text.starts_with("<local-command-caveat>")
        || text.starts_with("<command-name>")
        || text.starts_with("<local-command-stdout>")
}

fn sort_skills_block(text: &str) -> String {
    let re = Regex::new(r"^([\s\S]*?\n\n)(- [\s\S]+?)(\n</system-reminder>\s*)$").unwrap();
    if let Some(caps) = re.captures(text) {
        let header = caps.get(1).unwrap().as_str();
        let entries_text = caps.get(2).unwrap().as_str();
        let footer = caps.get(3).unwrap().as_str();

        // Split on "\n- " and re-add "- " prefix (avoids lookahead unsupported by regex crate).
        let parts: Vec<&str> = entries_text.split("\n- ").collect();
        let mut entries: Vec<String> = parts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if i == 0 {
                    p.to_string()
                } else {
                    format!("- {}", p)
                }
            })
            .collect();
        entries.sort();

        return format!("{}{}{}", header, entries.join("\n"), footer);
    }
    text.to_string()
}

fn sort_deferred_tools_block(text: &str) -> String {
    let re = Regex::new(
        r"^(<system-reminder>\nThe following deferred tools are now available[^\n]*\n)([\s\S]+?)(\n</system-reminder>\s*)$",
    )
    .unwrap();
    if let Some(caps) = re.captures(text) {
        let header = caps.get(1).unwrap().as_str();
        let tools_list = caps.get(2).unwrap().as_str();
        let footer = caps.get(3).unwrap().as_str();
        let mut tools: Vec<&str> = tools_list
            .split('\n')
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect();
        tools.sort();
        return format!("{}{}{}", header, tools.join("\n"), footer);
    }
    text.to_string()
}

fn strip_session_knowledge(text: &str) -> String {
    let re = Regex::new(r"\n<session_knowledge[^>]*>[\s\S]*?</session_knowledge>").unwrap();
    re.replace_all(text, "").to_string()
}

/// Normalize trailing whitespace before `</system-reminder>`.
fn normalize_reminder_trailing(text: &str) -> String {
    let re = Regex::new(r"\s+(</system-reminder>)\s*$").unwrap();
    re.replace(text, "\n$1").to_string()
}

/// Compute a short content hash for pinning (first 16 hex chars of SHA-256).
fn content_hash(text: &str) -> String {
    let hash = Sha256::digest(text.as_bytes());
    hash.iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
        .chars()
        .take(16)
        .collect()
}

fn pin_block_content(_block_type: &str, text: &str) -> String {
    // Without module-level cache, just normalize and return.
    // The pinning cache in the JS is a cross-request optimization;
    // per-request normalization produces the same semantic result.
    normalize_reminder_trailing(text)
}

fn get_block_type(text: &str) -> Option<&'static str> {
    if is_skills_block(text) {
        return Some("skills");
    }
    if is_deferred_tools_block(text) {
        return Some("deferred");
    }
    if is_mcp_block(text) {
        return Some("mcp");
    }
    if is_hooks_block(text) {
        return Some("hooks");
    }
    None
}

fn fix_block_text(block_type: &str, text: &str) -> String {
    let fixed = match block_type {
        "skills" => sort_skills_block(text),
        "deferred" => sort_deferred_tools_block(text),
        "hooks" => strip_session_knowledge(text),
        _ => text.to_string(),
    };
    pin_block_content(block_type, &fixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_relocatable_blocks() {
        assert!(is_relocatable_block(
            "<system-reminder>\nThe following skills are available"
        ));
        assert!(is_relocatable_block(
            "<system-reminder>\n# MCP Server Instructions"
        ));
        assert!(!is_relocatable_block("plain user message"));
    }

    #[test]
    fn test_is_clear_artifact() {
        assert!(is_clear_artifact("<local-command-caveat>foo"));
        assert!(is_clear_artifact("<command-name>/clear</command-name>"));
        assert!(!is_clear_artifact("normal text"));
    }

    #[test]
    fn test_strip_session_knowledge() {
        let input = "before\n<session_knowledge>secret stuff</session_knowledge>\nafter";
        let result = strip_session_knowledge(input);
        assert!(!result.contains("session_knowledge"));
        assert!(result.contains("before"));
        assert!(result.contains("after"));
    }

    #[test]
    fn test_normalize_reminder_trailing() {
        let input = "text   </system-reminder>  ";
        let result = normalize_reminder_trailing(input);
        assert_eq!(result, "text\n</system-reminder>");
    }

    #[test]
    fn test_content_hash_deterministic() {
        assert_eq!(content_hash("hello"), content_hash("hello"));
        assert_ne!(content_hash("hello"), content_hash("world"));
    }
}
