// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/sort-stabilization.mjs
// 翻译: 2026-05-20
//
// Deterministic ordering of skills, deferred tools, and tool definitions.
// Alphabetically sorts skills blocks and deferred-tools blocks in body.system,
// and sorts tool definitions in body.tools by name.

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use regex::Regex;
use serde_json::Value;

pub struct SortStabilization;

impl SortStabilization {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for SortStabilization {
    fn name(&self) -> &str {
        "sort-stabilization"
    }
    fn order(&self) -> u32 {
        200
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

impl RequestExtension for SortStabilization {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        // Sort skills blocks and deferred-tools blocks in body.system
        if let Some(system) = ctx.body.get_mut("system") {
            if let Some(blocks) = system.as_array_mut() {
                for block in blocks.iter_mut() {
                    if block.get("type").and_then(|v| v.as_str()) != Some("text") {
                        continue;
                    }
                    let text = match block.get("text").and_then(|v| v.as_str()) {
                        Some(t) => t.to_string(),
                        None => continue,
                    };

                    if is_skills_block(&text) {
                        let sorted = sort_skills_block(&text);
                        if sorted != text {
                            block["text"] = Value::String(sorted);
                        }
                    } else if is_deferred_tools_block(&text) {
                        let sorted = sort_deferred_tools_block(&text);
                        if sorted != text {
                            block["text"] = Value::String(sorted);
                        }
                    }
                }
            }
        }

        // Sort tool definitions by name
        if let Some(tools) = ctx.body.get_mut("tools") {
            if let Some(tools_arr) = tools.as_array_mut() {
                tools_arr.sort_by(|a, b| {
                    let name_a = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let name_b = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    name_a.cmp(name_b)
                });
            }
        }

        Ok(None)
    }
}

fn is_skills_block(text: &str) -> bool {
    text.contains("User-invocable skills")
}

fn is_deferred_tools_block(text: &str) -> bool {
    text.contains("deferred tools are now available")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_skills_block() {
        assert!(is_skills_block(
            "<system-reminder>\nThe following User-invocable skills are available"
        ));
        assert!(!is_skills_block("plain text"));
    }

    #[test]
    fn test_is_deferred_tools_block() {
        assert!(is_deferred_tools_block(
            "<system-reminder>\nThe following deferred tools are now available:"
        ));
        assert!(!is_deferred_tools_block("plain text"));
    }

    #[test]
    fn test_sort_skills_block_sorts_entries() {
        let input = concat!(
            "<system-reminder>\n",
            "The following skills are available:\n\n",
            "- z-skill: desc\n",
            "- a-skill: desc\n",
            "- m-skill: desc\n",
            "</system-reminder>\n"
        );
        let sorted = sort_skills_block(input);
        assert!(sorted.contains("- a-skill: desc"));
        assert!(sorted.contains("- m-skill: desc"));
        assert!(sorted.contains("- z-skill: desc"));
        // Verify a comes before m comes before z
        let a_pos = sorted.find("- a-skill:").unwrap();
        let m_pos = sorted.find("- m-skill:").unwrap();
        let z_pos = sorted.find("- z-skill:").unwrap();
        assert!(a_pos < m_pos);
        assert!(m_pos < z_pos);
    }

    #[test]
    fn test_sort_skills_block_no_match_returns_unchanged() {
        let input = "no skills block here";
        assert_eq!(sort_skills_block(input), input);
    }

    #[test]
    fn test_sort_deferred_tools_block_sorts_tools() {
        let input = concat!(
            "<system-reminder>\n",
            "The following deferred tools are now available:\n",
            "z-tool\n",
            "a-tool\n",
            "m-tool\n",
            "</system-reminder>\n"
        );
        let sorted = sort_deferred_tools_block(input);
        // The sorted tools should appear as: a-tool, m-tool, z-tool
        // Find them in the tools section (after header), checking the first "\na-" occurrence
        let tools_section_start = sorted.find("\na-tool").unwrap();
        let tools_section = &sorted[tools_section_start..];
        let a_pos = tools_section.find("a-tool").unwrap();
        let m_pos = tools_section.find("m-tool").unwrap();
        let z_pos = tools_section.find("z-tool").unwrap();
        assert!(a_pos < m_pos);
        assert!(m_pos < z_pos);
    }

    #[test]
    fn test_request_sorts_tools_by_name() {
        let ext = SortStabilization::new();
        let body = serde_json::json!({
            "tools": [
                {"name": "zebra", "description": "z"},
                {"name": "alpha", "description": "a"},
                {"name": "mike", "description": "m"}
            ]
        });
        let mut ctx = RequestContext {
            body,
            headers: axum::http::HeaderMap::new(),
            meta: ExtensionMeta::default(),
        };
        ext.on_request(&mut ctx).unwrap();
        let names: Vec<&str> = ctx.body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["alpha", "mike", "zebra"]);
    }
}
