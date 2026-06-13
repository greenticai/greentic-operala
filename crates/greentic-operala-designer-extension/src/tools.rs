//! Design-time tool surface for the OperaLa designer extension.
//!
//! All five tools share the same stateless JSON-envelope contract used by the
//! sorla designer extension:
//!
//! * **Success** — the function returns `Ok(serde_json::Value)` where the shape
//!   is documented per-tool.
//! * **Recoverable follow-up** — the function returns `Ok(json!({"follow_up":
//!   "question text"}))`. The designer chat loop shows the question to the
//!   operator and the operator's next message re-invokes `generate_answers`.
//! * **Hard error** — the function returns `Err(message)`. The component shell
//!   maps this into `types::ExtensionError::InvalidInput(message)` which the
//!   designer's tool-bridge surfaces as a diagnostic.
//!
//! The envelope convention is intentionally identical to the sorla extension
//! (greentic-sorla-designer-extension/src/lib.rs `yaml_tool_error` shape):
//! errors are `Err(message_string)`; follow-ups surface as a plain `{"follow_up":
//! "..."}` value rather than a hard error so the chat loop can continue.

use greentic_operala::inference::ChatFn;
use greentic_operala::{
    ExtensionRegistry, OperalaAnswers, SorlaContract, as_follow_up, build_handoff_plan,
    parse_sorla_contract, prompt_answers_for_contract, sorla_patch_proposal, validate_answers,
};
use serde_json::{Value, json};

/// Native tool definition mirrored into the WIT `tool-definition` record by
/// the wasm component shell.
#[derive(Debug, Clone)]
pub struct DesignerTool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema_json: String,
    pub output_schema_json: Option<String>,
}

/// Return the five OperaLa authoring tools advertised to the designer host.
pub fn list_tools() -> Vec<DesignerTool> {
    vec![
        tool(
            "list_operala_capabilities",
            "List all built-in OperaLa capabilities with their answers schemas.",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        ),
        tool(
            "generate_operala_answers",
            "Generate OperaLa answers from a SoRLa YAML string and a natural-language prompt.",
            json!({
                "type": "object",
                "required": ["sorla_yaml", "prompt"],
                "properties": {
                    "sorla_yaml":  { "type": "string" },
                    "prompt":      { "type": "string" },
                    "capability":  { "type": "string" },
                    "locale":      { "type": "string" },
                    "tenant":      { "type": "string" },
                    "team":        { "type": "string" }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "update_operala_answers",
            "Update existing OperaLa answers from a change instruction using the host LLM.",
            json!({
                "type": "object",
                "required": ["sorla_yaml", "answers", "instruction"],
                "properties": {
                    "sorla_yaml":   { "type": "string" },
                    "answers":      { "type": "object" },
                    "instruction":  { "type": "string" }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "validate_operala_answers",
            "Validate OperaLa answers against a SoRLa contract and return readiness plus a patch proposal.",
            json!({
                "type": "object",
                "required": ["sorla_yaml", "answers"],
                "properties": {
                    "sorla_yaml": { "type": "string" },
                    "answers":    { "type": "object" }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "generate_handoff_pack",
            "Build the OperaLa handoff plan and return in-memory pack entries.",
            // Identity derives from answers; `package` is accepted for designer
            // forward-compatibility and silently ignored — the answers document
            // is the authoritative source for all handoff metadata.
            json!({
                "type": "object",
                "required": ["sorla_yaml", "answers"],
                "properties": {
                    "sorla_yaml": { "type": "string" },
                    "answers":    { "type": "object" },
                    "package":    { "type": "object" }
                }
            }),
        ),
    ]
}

fn tool(name: &'static str, description: &'static str, input_schema: Value) -> DesignerTool {
    DesignerTool {
        name,
        description,
        input_schema_json: input_schema.to_string(),
        output_schema_json: None,
    }
}

// ---------------------------------------------------------------------------
// Public dispatch surface
// ---------------------------------------------------------------------------

/// Invoke a tool by name with JSON args, using no LLM backend.
///
/// Tools that require an LLM (`update_operala_answers`) return `Err` with a
/// descriptive message when called through this entry point.
pub fn handle(name: &str, args: Value) -> Result<Value, String> {
    handle_with_llm(name, args, None)
}

/// Invoke a tool by name, optionally providing an LLM backend for inference.
pub fn handle_with_llm(name: &str, args: Value, llm: Option<&dyn ChatFn>) -> Result<Value, String> {
    match name {
        "list_operala_capabilities" => list_capabilities(args),
        "generate_operala_answers" => generate_answers(args, llm),
        "update_operala_answers" => update_answers(args, llm),
        "validate_operala_answers" => validate(args),
        "generate_handoff_pack" => handoff(args),
        other => Err(format!("unknown OperaLa Designer tool `{other}`")),
    }
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

/// `list_operala_capabilities` — list all built-in extensions with their
/// `answers_schema` so the designer can surface the wizard form.
fn list_capabilities(_args: Value) -> Result<Value, String> {
    let registry = ExtensionRegistry::built_in();
    let capabilities: Vec<Value> = registry
        .all()
        .into_iter()
        .map(|ext| {
            json!({
                "id": ext.capability(),
                "extension_id": ext.id(),
                "version": ext.version(),
                "answers_schema": ext.answers_schema()
            })
        })
        .collect();
    Ok(json!({ "capabilities": capabilities }))
}

/// `generate_operala_answers` — parse the SoRLa YAML, run the capability
/// detection + answers inference pipeline, and return the answers document.
///
/// When the pipeline surfaces a "follow-up required" error the tool converts
/// it to a recoverable `{"follow_up": "question"}` envelope so the designer
/// chat loop can continue. Hard parse/build failures remain `Err`.
fn generate_answers(args: Value, llm: Option<&dyn ChatFn>) -> Result<Value, String> {
    let sorla_yaml = require_str(&args, "sorla_yaml")?;
    let prompt = require_str(&args, "prompt")?;
    let capability = args["capability"].as_str();
    let locale = args["locale"].as_str();
    let tenant = args["tenant"].as_str();
    let team = args["team"].as_str();

    let sorla = parse_sorla_with_uri(sorla_yaml)?;

    match prompt_answers_for_contract(&sorla, prompt, capability, locale, tenant, team, llm) {
        Ok(answers) => Ok(json!({ "answers": answers })),
        Err(err) => match as_follow_up(&err) {
            Some(question) => Ok(json!({ "follow_up": question })),
            None => Err(err),
        },
    }
}

/// `update_operala_answers` — regenerate capability answers from a change
/// instruction via the host LLM. Requires an LLM; returns a hard error when
/// called through the no-LLM path.
fn update_answers(args: Value, llm: Option<&dyn ChatFn>) -> Result<Value, String> {
    let chat = llm.ok_or_else(|| {
        "update_operala_answers requires an LLM; the host LLM is not available in this context"
            .to_string()
    })?;

    let sorla_yaml = require_str(&args, "sorla_yaml")?;
    let instruction = require_str(&args, "instruction")?;
    let answers_value = require_object(&args, "answers")?;

    let sorla = parse_sorla_with_uri(sorla_yaml)?;
    let existing: OperalaAnswers = serde_json::from_value(answers_value.clone())
        .map_err(|err| format!("answers is not a valid OperalaAnswers document: {err}"))?;

    match greentic_operala::inference::update_answers_for_contract(
        chat,
        &sorla,
        &existing,
        instruction,
    ) {
        Ok(outcome) => {
            let diff_values: Vec<Value> = outcome
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
            Ok(json!({
                "answers": outcome.answers,
                "diff": diff_values
            }))
        }
        Err(err) => match as_follow_up(&err) {
            Some(question) => Ok(json!({ "follow_up": question })),
            None => Err(err),
        },
    }
}

/// `validate_operala_answers` — validate answers against the schema and the
/// SoRLa contract's readiness checks, and attach a patch proposal when the
/// contract is missing required records.
///
/// Unlike the previous `.ok()` silencing, an unknown `answers.extension` (registry
/// miss) and `analyse_sorla` failures are now surfaced as `valid: false` with a
/// descriptive `issues` entry. This matches `run_wizard`'s hard-error behaviour so
/// the designer can surface a useful diagnostic instead of silently returning
/// `{valid:true, readiness:null}`.
fn validate(args: Value) -> Result<Value, String> {
    let sorla_yaml = require_str(&args, "sorla_yaml")?;
    let answers_value = require_object(&args, "answers")?;

    let sorla = parse_sorla_with_uri(sorla_yaml)?;
    let answers: OperalaAnswers = serde_json::from_value(answers_value.clone())
        .map_err(|err| format!("answers is not a valid OperalaAnswers document: {err}"))?;

    let schema_result = validate_answers(&answers);

    let registry = ExtensionRegistry::built_in();

    // Treat an unknown extension as a validation issue rather than silently
    // yielding `{valid:true, readiness:null}`.
    let extension_lookup = registry.get(&answers.extension).ok_or_else(|| {
        format!(
            "unknown extension `{}`; available: reconciliation, bulk_ingest",
            answers.extension
        )
    });

    // Resolve readiness and patch proposal, collecting any errors as issues.
    let (readiness_opt, readiness_error_opt) = match extension_lookup {
        Ok(ext) => match ext.analyse_sorla(&sorla, &answers) {
            Ok(readiness) => (Some(readiness), None),
            Err(err) => (None, Some(format!("readiness analysis failed: {err}"))),
        },
        Err(err) => (None, Some(err)),
    };

    // Patch proposal is best-effort and does not affect validity.
    // `.ok()` is intentional — a failed proposal does not invalidate answers
    // that are otherwise schema-correct; the issue will be surfaced via readiness.
    let patch_proposal_opt = readiness_opt
        .as_ref()
        .and_then(|readiness| sorla_patch_proposal(readiness, &sorla).ok());

    // Collect all issues in priority order: readiness/registry first, then schema.
    let mut issues: Vec<String> = Vec::new();
    if let Some(err) = readiness_error_opt {
        issues.push(err);
    }
    if let Err(err) = &schema_result {
        issues.push(err.clone());
    }

    let valid = issues.is_empty();
    let readiness_json = readiness_opt
        .as_ref()
        .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
        .unwrap_or(Value::Null);
    let patch_json = patch_proposal_opt
        .map(|p| serde_json::to_value(p).unwrap_or(Value::Null))
        .unwrap_or(Value::Null);

    Ok(json!({
        "valid": valid,
        "issues": issues,
        "readiness": readiness_json,
        "patch_proposal": patch_json
    }))
}

/// `generate_handoff_pack` — build the in-memory handoff plan and return the
/// handoff document plus all pack entries.
fn handoff(args: Value) -> Result<Value, String> {
    let sorla_yaml = require_str(&args, "sorla_yaml")?;
    let answers_value = require_object(&args, "answers")?;

    let sorla = parse_sorla_with_uri(sorla_yaml)?;
    let answers: OperalaAnswers = serde_json::from_value(answers_value.clone())
        .map_err(|err| format!("answers is not a valid OperalaAnswers document: {err}"))?;

    let plan = build_handoff_plan(&sorla, &answers)?;

    let pack_entries: Vec<Value> = plan
        .pack_entries
        .iter()
        .map(|entry| {
            json!({
                "path": entry.path,
                "sha256": entry.sha256,
                "content_base64": entry.content_base64
            })
        })
        .collect();

    Ok(json!({
        "handoff": plan.handoff,
        "pack_entries": pack_entries
    }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a SoRLa YAML string and stamp a non-empty URI so that `validate_answers`
/// does not fail on the `sorla.source.uri is empty` check.
fn parse_sorla_with_uri(yaml_text: &str) -> Result<SorlaContract, String> {
    let mut sorla = parse_sorla_contract(yaml_text)?;
    if sorla.source.uri.is_empty() {
        sorla.source.uri = "designer://inline".to_string();
    }
    Ok(sorla)
}

fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("required argument `{key}` is missing or empty"))
}

fn require_object(args: &Value, key: &str) -> Result<Value, String> {
    let value = &args[key];
    if value.is_object() {
        Ok(value.clone())
    } else {
        Err(format!("required argument `{key}` must be a JSON object"))
    }
}

// ---------------------------------------------------------------------------
// Unit tests (native rlib)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_tools_returns_five_entries() {
        let tools = list_tools();
        assert_eq!(tools.len(), 5);
        let names: Vec<&str> = tools.iter().map(|t| t.name).collect();
        assert!(names.contains(&"list_operala_capabilities"));
        assert!(names.contains(&"generate_operala_answers"));
        assert!(names.contains(&"update_operala_answers"));
        assert!(names.contains(&"validate_operala_answers"));
        assert!(names.contains(&"generate_handoff_pack"));
    }

    #[test]
    fn unknown_tool_is_error() {
        let err = handle("nope", json!({})).unwrap_err();
        assert!(err.contains("nope"));
    }

    #[test]
    fn validate_unknown_extension_is_invalid() {
        // Answers that name a non-existent extension must yield valid:false with
        // an issue mentioning the unknown extension name — not valid:true with null readiness.
        static MINIMAL_SORLA: &str = r#"
package:
  name: test-system
  version: 0.1.0
records:
  - name: Foo
    source: native
    fields:
      - name: id
        type: uuid
events: []
actions: []
agent_endpoints: []
"#;
        let out = handle(
            "validate_operala_answers",
            json!({
                "sorla_yaml": MINIMAL_SORLA,
                "answers": {
                    "schema": "greentic.operala.answers.v1",
                    "intent": "test intent",
                    "extension": "nope",
                    "sorla": {
                        "source": { "kind": "file", "uri": "designer://inline" }
                    },
                    "outputs": { "work_dir": "./target" },
                    "capability_answers": {}
                }
            }),
        )
        .unwrap();

        assert_eq!(
            out["valid"],
            json!(false),
            "expected valid:false for unknown extension, got: {out}"
        );
        let issues = out["issues"].as_array().expect("issues must be an array");
        assert!(
            !issues.is_empty(),
            "issues must not be empty for unknown extension, got: {out}"
        );
        let first_issue = issues[0].as_str().unwrap_or("");
        assert!(
            first_issue.contains("nope"),
            "issue should mention the unknown extension name, got: {first_issue}"
        );
    }

    #[test]
    fn list_capabilities_inline() {
        let out = handle("list_operala_capabilities", json!({})).unwrap();
        let caps = out["capabilities"].as_array().unwrap();
        let ids: Vec<&str> = caps.iter().map(|c| c["id"].as_str().unwrap()).collect();
        assert!(ids.contains(&"reconciliation"));
        assert!(ids.contains(&"bulk_ingest"));
        assert!(caps[0]["answers_schema"].is_object());
    }
}
