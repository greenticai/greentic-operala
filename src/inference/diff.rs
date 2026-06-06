//! Structural diff between two JSON documents, for human review of LLM
//! update-mode output. Arrays are compared wholesale.

use serde_json::Value;

#[derive(Debug, PartialEq)]
pub struct DiffEntry {
    pub path: String,
    pub old: Option<Value>,
    pub new: Option<Value>,
}

fn walk(path: &str, old: &Value, new: &Value, entries: &mut Vec<DiffEntry>) {
    match (old, new) {
        (Value::Object(old_map), Value::Object(new_map)) => {
            for (key, old_value) in old_map {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                match new_map.get(key) {
                    Some(new_value) => walk(&child, old_value, new_value, entries),
                    None => entries.push(DiffEntry {
                        path: child,
                        old: Some(old_value.clone()),
                        new: None,
                    }),
                }
            }
            for (key, new_value) in new_map {
                if !old_map.contains_key(key) {
                    let child = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    entries.push(DiffEntry {
                        path: child,
                        old: None,
                        new: Some(new_value.clone()),
                    });
                }
            }
        }
        (old_value, new_value) => {
            if old_value != new_value {
                entries.push(DiffEntry {
                    path: path.to_string(),
                    old: Some(old_value.clone()),
                    new: Some(new_value.clone()),
                });
            }
        }
    }
}

pub fn diff_values(old: &Value, new: &Value) -> Vec<DiffEntry> {
    let mut entries = Vec::new();
    walk("", old, new, &mut entries);
    entries
}

fn render(value: &Option<Value>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "(absent)".to_string(),
    }
}

pub fn format_diff(entries: &[DiffEntry]) -> String {
    entries
        .iter()
        .map(|entry| {
            format!(
                "{}: {} → {}",
                entry.path,
                render(&entry.old),
                render(&entry.new)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_change_is_reported_with_dotted_path() {
        let old = serde_json::json!({"matching": {"amount_tolerance": 2.0, "date_window_days": 7}});
        let new = serde_json::json!({"matching": {"amount_tolerance": 5.0, "date_window_days": 7}});
        let entries = diff_values(&old, &new);
        assert_eq!(
            entries,
            vec![DiffEntry {
                path: "matching.amount_tolerance".into(),
                old: Some(serde_json::json!(2.0)),
                new: Some(serde_json::json!(5.0)),
            }]
        );
    }

    #[test]
    fn added_and_removed_keys_are_reported() {
        let old = serde_json::json!({"a": 1, "removed": true});
        let new = serde_json::json!({"a": 1, "added": "x"});
        let entries = diff_values(&old, &new);
        assert!(entries.contains(&DiffEntry {
            path: "removed".into(),
            old: Some(serde_json::json!(true)),
            new: None
        }));
        assert!(entries.contains(&DiffEntry {
            path: "added".into(),
            old: None,
            new: Some(serde_json::json!("x"))
        }));
    }

    #[test]
    fn array_change_is_reported_wholesale() {
        let old = serde_json::json!({"input_modes": ["single", "batch"]});
        let new = serde_json::json!({"input_modes": ["batch"]});
        let entries = diff_values(&old, &new);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "input_modes");
    }

    #[test]
    fn identical_documents_have_empty_diff() {
        let doc = serde_json::json!({"a": {"b": 1}});
        assert!(diff_values(&doc, &doc).is_empty());
    }

    #[test]
    fn format_renders_arrow_lines() {
        let entries = vec![
            DiffEntry {
                path: "matching.amount_tolerance".into(),
                old: Some(serde_json::json!(2.0)),
                new: Some(serde_json::json!(5.0)),
            },
            DiffEntry {
                path: "added".into(),
                old: None,
                new: Some(serde_json::json!("x")),
            },
        ];
        let rendered = format_diff(&entries);
        assert!(rendered.contains("matching.amount_tolerance: 2.0 → 5.0"));
        assert!(rendered.contains("added: (absent) → \"x\""));
    }
}
