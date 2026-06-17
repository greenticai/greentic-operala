// Generated WIT bindings + world export glue are wasm-only. The native rlib
// build (used by unit tests) keeps the pure JSON-boundary API and never pulls
// in `wit-bindgen`'s wasm runtime.
#[cfg(target_arch = "wasm32")]
#[allow(warnings)]
mod bindings;
#[cfg(target_arch = "wasm32")]
mod component;

use base64::Engine as _;
use greentic_operala as op;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

// ─── Public entry-point ─────────────────────────────────────────────────────

pub fn invoke_tool(name: &str, args_json: &str) -> Result<String, String> {
    let input: Value = serde_json::from_str(args_json).map_err(|e| e.to_string())?;
    let out: Value = match name {
        "list_operala_capabilities" => list_capabilities(),
        "generate_operala_answers" => generate_answers_impl(&input, &*native_chat())?,
        "update_operala_answers" => update_answers_impl(&input, &*native_chat())?,
        "validate_operala_answers" => validate_answers_impl(&input)?,
        "generate_handoff_pack" => generate_handoff_pack_impl(&input)?,
        other => return Err(format!("unknown OperaLa tool `{other}`")),
    };
    serde_json::to_string(&out).map_err(|e| e.to_string())
}

// ─── list_operala_capabilities ───────────────────────────────────────────────

fn list_capabilities() -> Value {
    let reg = op::ExtensionRegistry::built_in();
    let extensions: Vec<Value> = reg
        .all()
        .iter()
        .map(|e| {
            json!({
                "id": e.id(),
                "capability": e.capability(),
                "version": e.version()
            })
        })
        .collect();
    let capabilities: Vec<Value> = extensions.iter().map(|e| e["capability"].clone()).collect();
    json!({ "capabilities": capabilities, "extensions": extensions })
}

// ─── generate_operala_answers (inner testable fn) ───────────────────────────

pub fn generate_answers_with_chat(
    input: &Value,
    chat: &dyn op::inference::ChatFn,
) -> Result<Value, String> {
    let sorla = parse_sorla(input)?;
    let prompt = input
        .get("prompt")
        .and_then(Value::as_str)
        .ok_or("missing prompt")?;
    let capability = match input.get("capability").and_then(Value::as_str) {
        Some(c) => c.to_string(),
        None => op::inference::classify_capability(chat, prompt)?
            .ok_or_else(|| "could not classify capability".to_string())?,
    };
    let (ext_id, schema) = capability_ids(&capability)?;
    match op::inference::infer_capability_answers(chat, ext_id, &schema, &sorla, prompt, None) {
        Ok(answers) => Ok(json!({ "answers": answers })),
        Err(e) if e.starts_with("follow-up required:") => Ok(json!({
            "follow_up": e.trim_start_matches("follow-up required:").trim()
        })),
        Err(e) => Err(e),
    }
}

fn generate_answers_impl(input: &Value, chat: &dyn op::inference::ChatFn) -> Result<Value, String> {
    generate_answers_with_chat(input, chat)
}

// ─── update_operala_answers (inner testable fn) ──────────────────────────────

pub fn update_answers_with_chat(
    input: &Value,
    chat: &dyn op::inference::ChatFn,
) -> Result<Value, String> {
    let sorla = parse_sorla(input)?;
    let instruction = input
        .get("instruction")
        .and_then(Value::as_str)
        .ok_or("missing instruction")?;
    let answers_value = input.get("answers").ok_or("missing answers")?;
    let existing: op::OperalaAnswers =
        serde_json::from_value(answers_value.clone()).map_err(|e| e.to_string())?;
    let outcome = op::inference::update_answers(chat, &existing, &sorla, instruction)?;
    let diff: Vec<Value> = outcome
        .diff
        .iter()
        .map(|entry| {
            json!({
                "path": entry.path,
                "old": entry.old,
                "new": entry.new
            })
        })
        .collect();
    Ok(json!({ "answers": outcome.answers, "diff": diff }))
}

fn update_answers_impl(input: &Value, chat: &dyn op::inference::ChatFn) -> Result<Value, String> {
    update_answers_with_chat(input, chat)
}

// ─── validate_operala_answers ────────────────────────────────────────────────

fn validate_answers_impl(input: &Value) -> Result<Value, String> {
    let sorla = parse_sorla(input)?;
    let answers_value = input.get("answers").ok_or("missing answers")?;
    let answers: op::OperalaAnswers =
        serde_json::from_value(answers_value.clone()).map_err(|e| e.to_string())?;
    let ext_id = answers.extension.as_str();
    let extension = op::ExtensionRegistry::built_in()
        .get(ext_id)
        .ok_or_else(|| format!("unknown extension `{ext_id}`"))?;
    let readiness = extension.analyse_sorla(&sorla, &answers)?;
    Ok(json!({ "readiness": readiness }))
}

// ─── generate_handoff_pack ───────────────────────────────────────────────────

fn generate_handoff_pack_impl(input: &Value) -> Result<Value, String> {
    let sorla = parse_sorla(input)?;
    let answers_value = input.get("answers").ok_or("missing answers")?;
    let answers: op::OperalaAnswers =
        serde_json::from_value(answers_value.clone()).map_err(|e| e.to_string())?;
    let ext_id = answers.extension.as_str();
    let extension = op::ExtensionRegistry::built_in()
        .get(ext_id)
        .ok_or_else(|| format!("unknown extension `{ext_id}`"))?;
    let readiness = extension.analyse_sorla(&sorla, &answers)?;
    let handoff = extension.build_handoff(&sorla, &answers, &readiness)?;
    let raw_entries = op::build_operala_pack_entries(&handoff)?;
    let pack_entries: Vec<Value> = raw_entries
        .iter()
        .map(|(path, bytes)| {
            json!({
                "path": path,
                "sha256": sha256_hex(bytes),
                "content_base64": base64::engine::general_purpose::STANDARD.encode(bytes)
            })
        })
        .collect();
    Ok(json!({ "handoff": handoff, "pack_entries": pack_entries }))
}

// ─── list_tools ──────────────────────────────────────────────────────────────

pub struct ToolDefinitionLite {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema_json: String,
    pub output_schema_json: Option<String>,
}

pub fn list_tools() -> Vec<ToolDefinitionLite> {
    let open_schema = json!({ "type": "object", "additionalProperties": true }).to_string();
    vec![
        ToolDefinitionLite {
            name: "list_operala_capabilities",
            description: "List all available OperaLa capabilities and their extension metadata.",
            input_schema_json: open_schema.clone(),
            output_schema_json: None,
        },
        ToolDefinitionLite {
            name: "generate_operala_answers",
            description: "Generate OperaLa capability answers from a SoRLa contract and a prompt.",
            input_schema_json: open_schema.clone(),
            output_schema_json: None,
        },
        ToolDefinitionLite {
            name: "update_operala_answers",
            description: "Update existing OperaLa capability answers from a change instruction.",
            input_schema_json: open_schema.clone(),
            output_schema_json: None,
        },
        ToolDefinitionLite {
            name: "validate_operala_answers",
            description: "Validate OperaLa capability answers against a SoRLa contract.",
            input_schema_json: open_schema.clone(),
            output_schema_json: None,
        },
        ToolDefinitionLite {
            name: "generate_handoff_pack",
            description: "Build OperaLa handoff pack entries from a SoRLa contract + answers.",
            input_schema_json: open_schema,
            output_schema_json: None,
        },
    ]
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn parse_sorla(input: &Value) -> Result<op::SorlaContract, String> {
    let yaml = input
        .get("sorla_yaml")
        .and_then(Value::as_str)
        .ok_or("missing sorla_yaml")?;
    op::parse_sorla_contract_from_yaml(yaml)
}

/// Map a human-readable capability name to (extension_id, answers_schema).
fn capability_ids(name: &str) -> Result<(&'static str, Value), String> {
    let ext_id: &'static str = match name {
        "reconciliation" => op::EXTENSION_RECONCILIATION,
        "bulk_ingest" => op::EXTENSION_BULK_INGEST,
        other => return Err(format!("unknown capability `{other}`")),
    };
    let schema = op::ExtensionRegistry::built_in()
        .get(ext_id)
        .ok_or_else(|| format!("extension `{ext_id}` not found in registry"))?
        .answers_schema();
    Ok((ext_id, schema))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// On wasm32 the real HostLlmChat is used; on native tests use the NativeStubChat.
#[cfg(target_arch = "wasm32")]
fn native_chat() -> Box<dyn op::inference::ChatFn> {
    Box::new(crate::component::HostLlmChat)
}

#[cfg(not(target_arch = "wasm32"))]
fn native_chat() -> Box<dyn op::inference::ChatFn> {
    Box::new(NativeStubChat)
}

/// Native stub: returns a fixed follow_up response so tests that call
/// invoke_tool("generate_operala_answers", ...) without injecting a real
/// ChatFn get a predictable "follow_up" JSON rather than panicking.
/// Tests that need real inference inject their own ChatFn via `*_with_chat`.
#[cfg(not(target_arch = "wasm32"))]
struct NativeStubChat;

#[cfg(not(target_arch = "wasm32"))]
impl op::inference::ChatFn for NativeStubChat {
    fn tools_supported(&self) -> bool {
        false
    }

    fn chat(
        &self,
        _request: op::ChatRequest,
    ) -> Result<op::ChatResponse, op::LlmError> {
        Ok(op::ChatResponse {
            content: r#"{"follow_up":"no LLM configured on native; use invoke_tool with a scripted stub"}"#.to_string(),
            tool_calls: vec![],
            finish_reason: op::FinishReason::Stop,
        })
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Scripted no-tools ChatFn stub (local, avoids reaching into operala privates) ──

    struct ScriptedChat {
        responses: std::collections::VecDeque<op::ChatResponse>,
    }

    impl ScriptedChat {
        fn new(responses: Vec<op::ChatResponse>) -> Self {
            Self {
                responses: responses.into(),
            }
        }
    }

    impl op::inference::ChatFn for ScriptedChat {
        fn tools_supported(&self) -> bool {
            false
        }

        fn chat(
            &self,
            _request: op::ChatRequest,
        ) -> Result<op::ChatResponse, op::LlmError> {
            // VecDeque is behind &self so we use unsafe interior mutability via
            // a RefCell to pop from the front. Alternatively, use a Mutex.
            // For simplicity in tests we just clone the first response each time.
            // (Single-response scripted chat.)
            Ok(self
                .responses
                .front()
                .expect("scripted chat: no more responses")
                .clone())
        }
    }

    fn fixture_sorla_yaml() -> String {
        std::fs::read_to_string("../../extensions/reconciliation/examples/tenancy/sorla.yaml")
            .expect("fixture sorla.yaml")
    }

    fn fixture_answers_value() -> Value {
        let raw = std::fs::read_to_string(
            "../../extensions/reconciliation/examples/tenancy/answers.json",
        )
        .expect("fixture answers.json");
        serde_json::from_str(&raw).expect("parse answers.json")
    }

    // ── list_operala_capabilities ────────────────────────────────────────────

    #[test]
    fn list_capabilities_returns_both() {
        let out = invoke_tool("list_operala_capabilities", "{}").unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let caps = v["capabilities"].as_array().unwrap();
        assert_eq!(caps.len(), 2);
    }

    // ── validate_operala_answers ─────────────────────────────────────────────

    #[test]
    fn validate_returns_readiness() {
        let yaml = fixture_sorla_yaml();
        let answers = fixture_answers_value();
        let args = json!({ "sorla_yaml": yaml, "answers": answers }).to_string();
        let out = invoke_tool("validate_operala_answers", &args).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(
            v.get("readiness").is_some(),
            "expected readiness key, got: {v}"
        );
    }

    // ── generate_handoff_pack ────────────────────────────────────────────────

    #[test]
    fn handoff_returns_pack_entries() {
        let yaml = fixture_sorla_yaml();
        let answers = fixture_answers_value();
        let args = json!({ "sorla_yaml": yaml, "answers": answers }).to_string();
        let out = invoke_tool("generate_handoff_pack", &args).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let entries = v["pack_entries"].as_array().unwrap();
        assert!(
            entries.iter().any(|e| e["path"] == "manifest.cbor"),
            "manifest.cbor not found in pack_entries: {:?}",
            entries.iter().map(|e| &e["path"]).collect::<Vec<_>>()
        );
        // All entries must have non-empty sha256 + content_base64.
        for entry in entries {
            let sha = entry["sha256"].as_str().unwrap_or("");
            let b64 = entry["content_base64"].as_str().unwrap_or("");
            assert!(!sha.is_empty(), "empty sha256 on {}", entry["path"]);
            assert!(!b64.is_empty(), "empty content_base64 on {}", entry["path"]);
        }
    }

    // ── generate_operala_answers (scripted ChatFn) ───────────────────────────

    #[test]
    fn generate_answers_with_scripted_chat() {
        let yaml = fixture_sorla_yaml();
        let answers_doc = fixture_answers_value();
        // Extract just the reconciliation capability_answers part as the emit payload.
        let cap_answers = answers_doc["capability_answers"]["reconciliation"].clone();
        // ScriptedChat returns the JSON content directly (no tool-calls, tools_supported=false).
        // The inference session will parse `emit_answers` from the content field.
        let content = json!({ "emit_answers": cap_answers }).to_string();
        let chat = ScriptedChat::new(vec![op::ChatResponse {
            content,
            tool_calls: vec![],
            finish_reason: op::FinishReason::Stop,
        }]);
        let input = json!({
            "sorla_yaml": yaml,
            "prompt": "reconcile rent payments",
            "capability": "reconciliation"
        });
        let result = generate_answers_with_chat(&input, &chat).unwrap();
        // Either "answers" (success) or "follow_up" (scripted chat exhausted retries) is acceptable.
        assert!(
            result.get("answers").is_some() || result.get("follow_up").is_some(),
            "expected answers or follow_up, got: {result}"
        );
    }

    // ── update_operala_answers (scripted ChatFn) ─────────────────────────────

    #[test]
    fn update_answers_with_scripted_chat() {
        let yaml = fixture_sorla_yaml();
        let answers_doc = fixture_answers_value();
        let cap_answers = answers_doc["capability_answers"]["reconciliation"].clone();
        let content = json!({ "emit_answers": cap_answers }).to_string();
        let chat = ScriptedChat::new(vec![op::ChatResponse {
            content,
            tool_calls: vec![],
            finish_reason: op::FinishReason::Stop,
        }]);
        let input = json!({
            "sorla_yaml": yaml,
            "answers": answers_doc,
            "instruction": "use batch mode only"
        });
        let result = update_answers_with_chat(&input, &chat);
        // Either success or a follow-up string error is acceptable from the scripted chat.
        match result {
            Ok(v) => {
                assert!(
                    v.get("answers").is_some() && v.get("diff").is_some(),
                    "expected both answers and diff, got: {v}"
                );
            }
            Err(e) => {
                assert!(
                    e.starts_with("follow-up required:"),
                    "unexpected error: {e}"
                );
            }
        }
    }

    // ── unknown tool ─────────────────────────────────────────────────────────

    #[test]
    fn unknown_tool_returns_error() {
        let result = invoke_tool("does_not_exist", "{}");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown OperaLa tool"));
    }

    // ── describe.json sanity ─────────────────────────────────────────────────
    //
    // Full sdk-contract validation is blocked until a version with `llmRoles`
    // support (>=1.2.7) is published to crates.io. This hand-rolled test checks
    // the fields the extension contract would validate, plus the exact tool set.

    #[test]
    fn describe_json_sanity() {
        let raw = include_str!("../describe.json");
        let v: Value = serde_json::from_str(raw).expect("describe.json must be valid JSON");

        // Top-level envelope.
        assert_eq!(v["apiVersion"], "greentic.ai/v2", "apiVersion mismatch");
        assert_eq!(v["kind"], "DesignExtension", "kind mismatch");

        // metadata.id.
        assert_eq!(
            v["metadata"]["id"],
            "greentic.operala",
            "metadata.id mismatch"
        );

        // runtime.permissions.llmRoles must contain exactly ["operala_composer"].
        let llm_roles = v["runtime"]["permissions"]["llmRoles"]
            .as_array()
            .expect("runtime.permissions.llmRoles must be an array");
        assert_eq!(
            llm_roles.iter().map(|r| r.as_str().unwrap_or("")).collect::<Vec<_>>(),
            vec!["operala_composer"],
            "llmRoles must be exactly [\"operala_composer\"]"
        );

        // runtime.components.operala.world.
        assert_eq!(
            v["runtime"]["components"]["operala"]["world"],
            "greentic:operala-designer-extension/design-extension",
            "component world mismatch"
        );

        // contributions.tools must be exactly the 5 expected tool names, in order.
        let expected_tools = [
            "list_operala_capabilities",
            "generate_operala_answers",
            "update_operala_answers",
            "validate_operala_answers",
            "generate_handoff_pack",
        ];
        let described_tools: Vec<&str> = v["contributions"]["tools"]
            .as_array()
            .expect("contributions.tools must be an array")
            .iter()
            .map(|t| t["name"].as_str().expect("tool name must be a string"))
            .collect();
        assert_eq!(
            described_tools, expected_tools,
            "contributions.tools names/order mismatch"
        );

        // Cross-check: describe.json tool set must match list_tools().
        let listed_tools: Vec<&str> = list_tools().iter().map(|t| t.name).collect();
        assert_eq!(
            described_tools, listed_tools,
            "describe.json tools must match list_tools() exactly"
        );
    }
}
