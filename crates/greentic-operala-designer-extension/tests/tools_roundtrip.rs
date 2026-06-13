//! Roundtrip tests for the five OperaLa designer tools.
//!
//! These tests run on the native rlib (no wasm host).  LLM-backed tools are
//! tested via a locally defined `Scripted` ChatFn that plays back canned wire
//! responses — the lib's `ScriptedChat` is `cfg(test)` on the lib crate and
//! therefore not re-exported; we define our own minimal shim here.

use greentic_operala::inference::{ChatFn, WireChatError, WireChatRequest, WireChatResponse};
use greentic_operala_designer_extension::tools;
use serde_json::{Value, json};
use std::cell::RefCell;

// ---------------------------------------------------------------------------
// Local scripted-chat shim (mirrors lib's cfg(test) ScriptedChat)
// ---------------------------------------------------------------------------

struct Scripted(RefCell<Vec<WireChatResponse>>);

impl ChatFn for Scripted {
    fn chat(&self, _request: WireChatRequest) -> Result<WireChatResponse, WireChatError> {
        let mut queue = self.0.borrow_mut();
        assert!(!queue.is_empty(), "Scripted: no more responses");
        Ok(queue.remove(0))
    }

    fn tools_supported(&self) -> bool {
        false
    }
}

fn scripted(responses: Vec<WireChatResponse>) -> Scripted {
    Scripted(RefCell::new(responses))
}

/// A no-tools-path JSON-envelope response that emits capability answers.
fn emit_json(value: Value) -> WireChatResponse {
    WireChatResponse {
        content: serde_json::json!({ "emit_answers": value }).to_string(),
        tool_calls: vec![],
    }
}

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

static SORLA_YAML: &str =
    include_str!("../../../extensions/reconciliation/examples/tenancy/sorla.yaml");

static ANSWERS_JSON: &str =
    include_str!("../../../extensions/reconciliation/examples/tenancy/answers.json");

/// Parse the fixture answers and return the capability-answers object used to
/// drive the LLM-path scripted responses.
fn fixture_capability_answers() -> Value {
    let answers: greentic_operala::OperalaAnswers =
        serde_json::from_str(ANSWERS_JSON).expect("fixture answers parse");
    serde_json::to_value(answers.capability_answers.reconciliation.unwrap())
        .expect("serialize reconciliation answers")
}

/// Return the fixture OperalaAnswers with a non-empty `sorla.source.uri` so
/// that `validate_answers` does not reject the document.
fn fixture_answers_with_uri() -> Value {
    let mut answers: Value = serde_json::from_str(ANSWERS_JSON).expect("fixture answers parse");
    answers["sorla"]["source"]["uri"] = json!("designer://inline");
    answers
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn list_capabilities_returns_both_builtins() {
    let out = tools::handle("list_operala_capabilities", json!({})).unwrap();
    let caps = out["capabilities"].as_array().unwrap();
    let ids: Vec<&str> = caps.iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"reconciliation"), "missing reconciliation");
    assert!(ids.contains(&"bulk_ingest"), "missing bulk_ingest");
    // answers_schema must be a schema object with "type": "object"
    assert!(
        out["capabilities"][0]["answers_schema"].is_object(),
        "answers_schema must be an object"
    );
}

#[test]
fn generate_answers_via_scripted_llm() {
    let cap_answers = fixture_capability_answers();
    let chat = scripted(vec![emit_json(cap_answers)]);

    let out = tools::handle_with_llm(
        "generate_operala_answers",
        json!({
            "sorla_yaml": SORLA_YAML,
            "prompt": "reconcile rent payments",
            "tenant": "demo-tenant"
        }),
        Some(&chat),
    )
    .unwrap();

    // The fixture sorla has all required reconciliation records; with the
    // scripted LLM returning valid answers the pipeline must produce answers,
    // not a follow-up question.
    assert!(
        out["answers"].is_object(),
        "expected answers (not follow_up) for fixture sorla+llm path, got: {out}"
    );
}

#[test]
fn generate_answers_deterministic_keyword_path() {
    // "reconcile rent payments" hits the keyword fast-path — no LLM needed for
    // the sorla that has BankTransaction + RentObligation + Payment + ReconciliationCase.
    let out = tools::handle(
        "generate_operala_answers",
        json!({
            "sorla_yaml": SORLA_YAML,
            "prompt": "reconcile rent payments",
            "tenant": "demo-tenant"
        }),
    )
    .unwrap();

    // The fixture sorla contains all required records; the keyword path must
    // resolve deterministically to answers, not a follow-up.
    assert!(
        out["answers"].is_object(),
        "expected answers (not follow_up) for keyword path, got: {out}"
    );
}

#[test]
fn update_answers_returns_diff() {
    let mut cap_answers = fixture_capability_answers();
    cap_answers["matching"]["amount_tolerance"] = json!(5.0);

    let chat = scripted(vec![emit_json(cap_answers)]);
    let answers = fixture_answers_with_uri();

    let out = tools::handle_with_llm(
        "update_operala_answers",
        json!({
            "sorla_yaml": SORLA_YAML,
            "answers": answers,
            "instruction": "raise amount tolerance to 5"
        }),
        Some(&chat),
    )
    .unwrap();

    assert!(out["answers"].is_object(), "expected answers, got: {out}");
    assert!(out["diff"].is_array(), "expected diff array, got: {out}");
}

#[test]
fn update_answers_without_llm_is_a_clear_error() {
    let answers = fixture_answers_with_uri();
    let err = tools::handle(
        "update_operala_answers",
        json!({
            "sorla_yaml": SORLA_YAML,
            "answers": answers,
            "instruction": "raise amount tolerance to 5"
        }),
    )
    .unwrap_err();
    assert!(
        err.contains("LLM") || err.contains("llm"),
        "expected LLM requirement error, got: {err}"
    );
}

#[test]
fn validate_reports_readiness_and_patch() {
    let answers = fixture_answers_with_uri();

    let out = tools::handle(
        "validate_operala_answers",
        json!({
            "sorla_yaml": SORLA_YAML,
            "answers": answers
        }),
    )
    .unwrap();

    // Fixture answers + fixture sorla are consistent → should be valid.
    assert_eq!(out["valid"], json!(true), "got: {out}");
    assert!(
        out["readiness"].is_object(),
        "readiness must be present, got: {out}"
    );
    // readiness.status must be present
    let status = out["readiness"]["status"].as_str();
    assert!(
        status.is_some(),
        "readiness.status must be a string, got: {out}"
    );
}

#[test]
fn validate_reports_issues_when_sorla_has_missing_record() {
    // Use a minimal sorla that is missing the required reconciliation records.
    let minimal_sorla = r#"
package:
  name: minimal-system
  version: 0.1.0
records:
  - name: Tenant
    source: native
    fields:
      - name: id
        type: uuid
events: []
actions: []
agent_endpoints: []
"#;

    let answers = fixture_answers_with_uri();

    let out = tools::handle(
        "validate_operala_answers",
        json!({
            "sorla_yaml": minimal_sorla,
            "answers": answers
        }),
    )
    .unwrap();

    // With the minimal sorla the readiness check will find missing items.
    // The validate result may be valid (schema is fine) but readiness status
    // should not be "ready" when required records are absent.
    assert!(
        out["readiness"].is_object(),
        "readiness must be present, got: {out}"
    );
    let status = out["readiness"]["status"].as_str().unwrap_or("missing");
    assert_ne!(
        status, "ready",
        "readiness should not be ready for minimal sorla, got: {out}"
    );
}

#[test]
fn handoff_pack_returns_entries() {
    let answers = fixture_answers_with_uri();

    let out = tools::handle(
        "generate_handoff_pack",
        json!({
            "sorla_yaml": SORLA_YAML,
            "answers": answers
        }),
    )
    .unwrap();

    let entries = out["pack_entries"]
        .as_array()
        .expect("pack_entries must be array");
    assert!(!entries.is_empty(), "pack_entries must not be empty");
    assert_eq!(
        out["handoff"]["schema"],
        json!("greentic.operala.handoff.v1"),
        "handoff.schema mismatch, got: {out}"
    );
}

/// Verify that validate surfaces `valid:false` and a useful issue when answers
/// name an extension that does not exist in the built-in registry.
#[test]
fn validate_unknown_extension_is_invalid() {
    let out = tools::handle(
        "validate_operala_answers",
        json!({
            "sorla_yaml": SORLA_YAML,
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
        "expected valid:false for unknown extension `nope`, got: {out}"
    );
    let issues = out["issues"].as_array().expect("issues must be an array");
    assert!(!issues.is_empty(), "issues must not be empty, got: {out}");
    let first_issue = issues[0].as_str().unwrap_or("");
    assert!(
        first_issue.contains("nope"),
        "issue should name the unknown extension, got: {first_issue}"
    );
}

/// Verify that `generate_answers` surfaces a follow-up envelope when the
/// pipeline cannot determine the capability without operator input.
///
/// Uses a minimal SoRLa (no capability keywords, no LLM) so that
/// `detect_capability` falls through to the "ask the user" branch and emits
/// a follow-up-required error, which the tool must convert to `{"follow_up":
/// "..."}` instead of a hard `Err`.
#[test]
fn generate_answers_surfaces_follow_up() {
    // A sorla with no recognisable records — the keyword fast-path won't match
    // and without an LLM the pipeline must fall back to the follow-up branch.
    let minimal_sorla = r#"
package:
  name: unknown-system
  version: 0.1.0
records:
  - name: Widget
    source: native
    fields:
      - name: id
        type: uuid
events: []
actions: []
agent_endpoints: []
"#;

    let out = tools::handle(
        "generate_operala_answers",
        json!({
            "sorla_yaml": minimal_sorla,
            "prompt": "do something with widgets",
            "tenant": "demo-tenant"
        }),
    )
    .unwrap();

    let follow_up = out["follow_up"]
        .as_str()
        .expect("expected a follow_up question for ambiguous capability, got: {out}");
    assert!(
        !follow_up.is_empty(),
        "follow_up question must not be empty"
    );
}

#[test]
fn unknown_tool_is_error_envelope() {
    let err = tools::handle("nope", json!({})).unwrap_err();
    assert!(
        err.contains("nope"),
        "error should name the unknown tool, got: {err}"
    );
}
