//! LLM-backed inference for `prompt` — config resolution, SoRLa catalog,
//! chat session, validation gate. The deterministic keyword path stays in
//! `lib.rs`; this module is only entered when an LLM is configured.

pub mod catalog;
pub mod diff;
pub mod session;
pub mod validate;
pub mod wire;

pub use wire::{
    WireChatError, WireChatMessage, WireChatRequest, WireChatResponse, WireRole, WireToolCall,
    WireToolSpec,
};

#[cfg(feature = "native")]
use crate::PromptArgs;
use crate::{OperalaResult, SorlaContract, follow_up_required};
#[cfg(feature = "native")]
use greentic_llm::{CredentialSource, EnvCredentialSource, LlmProvider, ProviderKind, RigBackend};
use serde_json::Value;

use session::{InferenceOutcome, build_request, inference_messages, parse_outcome};
use validate::validate_capability_answers;

/// Total LLM attempts before giving up and surfacing a follow-up question.
const MAX_ATTEMPTS: usize = 3;

/// Run LLM inference for one extension and gate the result deterministically.
/// `existing_answers` switches update mode on. Returns the validated
/// capability-answers JSON value, or `Err(follow_up_required(...))` when the
/// model asks for clarification or exhausts its retries.
pub fn infer_capability_answers(
    chat: &dyn ChatFn,
    extension_id: &str,
    answers_schema: &Value,
    sorla: &SorlaContract,
    intent: &str,
    existing_answers: Option<&Value>,
) -> OperalaResult<Value> {
    let catalog = catalog::sorla_catalog(sorla);
    let mut messages = inference_messages(&catalog, intent, existing_answers);
    let tools_supported = chat.tools_supported();
    let mut last_errors: Vec<String> = Vec::new();
    for _ in 0..MAX_ATTEMPTS {
        let request = build_request(messages.clone(), answers_schema.clone(), tools_supported);
        let response = chat.chat(request).map_err(|err| {
            format!("LLM request failed: {err}; use --no-llm for the deterministic path")
        })?;
        match parse_outcome(&response) {
            Ok(InferenceOutcome::FollowUp(question)) => return Err(follow_up_required(&question)),
            Ok(InferenceOutcome::Answers(value)) => {
                match validate_capability_answers(extension_id, &value, sorla) {
                    Ok(()) => return Ok(value),
                    Err(errors) => {
                        messages.push(WireChatMessage {
                            role: WireRole::User,
                            content: format!(
                                "Your answers failed validation:\n- {}\nEmit a corrected complete answers object via emit_answers, binding only to catalog identifiers.",
                                errors.join("\n- ")
                            ),
                        });
                        last_errors = errors;
                    }
                }
            }
            Err(parse_error) => {
                messages.push(WireChatMessage {
                    role: WireRole::User,
                    content: format!(
                        "{parse_error}\nRespond again using the emit_answers or follow_up tool."
                    ),
                });
                last_errors = vec![parse_error];
            }
        }
    }
    Err(follow_up_required(&format!(
        "the LLM could not produce valid bindings after {} attempts ({}); please answer directly",
        MAX_ATTEMPTS,
        last_errors.join("; ")
    )))
}

/// One-shot capability classification for prompts the keyword fast-path could
/// not place. Returns None for "unknown". Deliberately does NOT include the
/// SoRLa catalog — the classification is between named capabilities and the
/// catalog only adds noise and prompt-injection surface.
pub fn classify_capability(chat: &dyn ChatFn, intent: &str) -> OperalaResult<Option<String>> {
    let request = WireChatRequest {
        messages: vec![
            WireChatMessage {
                role: WireRole::System,
                content: "Classify the operator intent into one operational capability. Respond with ONLY a JSON object: {\"capability\": \"reconciliation\"} or {\"capability\": \"bulk_ingest\"} or {\"capability\": \"unknown\"}.".into(),
            },
            WireChatMessage {
                role: WireRole::User,
                content: format!("Operator intent: {intent}"),
            },
        ],
        tools: vec![],
        tool_choice: None,
        max_tokens: Some(64),
        temperature: Some(0.0),
    };
    let response = chat
        .chat(request)
        .map_err(|err| format!("LLM classification failed: {err}"))?;
    session::parse_classification(&response)
}

pub struct UpdateOutcome {
    pub answers: crate::OperalaAnswers,
    pub diff: Vec<diff::DiffEntry>,
}

/// Contract-first update: regenerate capability answers from a change instruction.
///
/// Wasm-safe — the SoRLa contract is already resident in memory; no filesystem
/// access occurs here. The native [`update_answers`] loads the contract from
/// disk and then delegates to this function.
///
/// Returns a validated [`UpdateOutcome`] containing the updated answers document
/// and a structural diff for human review.
pub fn update_answers_for_contract(
    chat: &dyn ChatFn,
    sorla: &crate::SorlaContract,
    existing: &crate::OperalaAnswers,
    instruction: &str,
) -> OperalaResult<UpdateOutcome> {
    use crate::OperaLaExtension;

    let (extension_id, schema, existing_value) = match (
        &existing.capability_answers.reconciliation,
        &existing.capability_answers.bulk_ingest,
    ) {
        (Some(reconciliation), _) => (
            crate::EXTENSION_RECONCILIATION,
            crate::RECONCILIATION_EXTENSION.answers_schema(),
            serde_json::to_value(reconciliation).map_err(crate::to_string)?,
        ),
        (None, Some(bulk)) => (
            crate::EXTENSION_BULK_INGEST,
            crate::BULK_INGEST_EXTENSION.answers_schema(),
            serde_json::to_value(bulk).map_err(crate::to_string)?,
        ),
        (None, None) => {
            return Err("existing answers contain no capability answers to update".to_string());
        }
    };

    let updated_value = infer_capability_answers(
        chat,
        extension_id,
        &schema,
        sorla,
        instruction,
        Some(&existing_value),
    )?;

    let mut updated = existing.clone();
    updated.intent = instruction.to_string();
    match extension_id {
        crate::EXTENSION_RECONCILIATION => {
            updated.capability_answers.reconciliation =
                Some(serde_json::from_value(updated_value).map_err(crate::to_string)?);
        }
        crate::EXTENSION_BULK_INGEST => {
            updated.capability_answers.bulk_ingest =
                Some(serde_json::from_value(updated_value).map_err(crate::to_string)?);
        }
        other => {
            return Err(format!(
                "internal: unhandled capability '{other}' in update_answers_for_contract"
            ));
        }
    }
    crate::validate_answers(&updated)?;

    let old_doc = serde_json::to_value(existing).map_err(crate::to_string)?;
    let new_doc = serde_json::to_value(&updated).map_err(crate::to_string)?;
    Ok(UpdateOutcome {
        answers: updated,
        diff: diff::diff_values(&old_doc, &new_doc),
    })
}

/// Update mode: regenerate the capability answers via LLM from the existing
/// document + change instruction; preserve the outer envelope; return the
/// validated document plus a structural diff for human review.
///
/// Requires `native` feature (reads SoRLa from the filesystem).
/// The LLM-regeneration and structural-diff core lives in the wasm-safe
/// [`update_answers_for_contract`]; this function only adds the filesystem
/// contract load.
#[cfg(feature = "native")]
pub fn update_answers(
    chat: &dyn ChatFn,
    existing: &crate::OperalaAnswers,
    sorla_path: &str,
    instruction: &str,
) -> OperalaResult<UpdateOutcome> {
    let sorla = crate::load_sorla_contract(&crate::SourceRef {
        kind: crate::SourceKind::File,
        uri: sorla_path.to_string(),
        digest: None,
    })?;
    update_answers_for_contract(chat, &sorla, existing, instruction)
}

/// Test seam: session functions take `&dyn ChatFn` so tests inject the
/// scripted mock without a real backend or runtime. Implementors receive
/// backend-agnostic wire types; `LlmRuntime` converts to/from greentic-llm
/// at its own boundary.
pub trait ChatFn {
    fn chat(&self, request: WireChatRequest) -> Result<WireChatResponse, WireChatError>;

    /// Whether the underlying provider supports native tool calling.
    /// Defaults to true; `LlmRuntime` reports the real capability.
    fn tools_supported(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Native-only: LLM runtime, resolver, and greentic-llm conversion helpers.
// ---------------------------------------------------------------------------

#[cfg(feature = "native")]
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedLlm {
    pub provider: ProviderKind,
    pub model: String,
}

/// Resolve whether this invocation uses an LLM. Precedence: `--no-llm` >
/// flags > `GREENTIC_LLM_PROVIDER`/`GREENTIC_LLM_MODEL` env > unset (None →
/// deterministic keyword path). Env access is injected for testability.
#[cfg(feature = "native")]
pub fn resolve_llm_request(
    args: &PromptArgs,
    env: &dyn Fn(&str) -> Option<String>,
) -> OperalaResult<Option<ResolvedLlm>> {
    if args.no_llm {
        return Ok(None);
    }
    let provider = match args.llm_provider {
        Some(provider) => Some(provider),
        None => match env("GREENTIC_LLM_PROVIDER") {
            Some(raw) => Some(
                raw.parse::<ProviderKind>()
                    .map_err(|err| format!("invalid GREENTIC_LLM_PROVIDER: {err}"))?,
            ),
            None => None,
        },
    };
    let Some(provider) = provider else {
        return Ok(None);
    };
    let model = args
        .llm_model
        .clone()
        .or_else(|| env("GREENTIC_LLM_MODEL"))
        .ok_or_else(|| {
            format!(
                "LLM provider '{}' is configured but no model is set; pass --llm-model or set GREENTIC_LLM_MODEL",
                provider.as_str()
            )
        })?;
    Ok(Some(ResolvedLlm { provider, model }))
}

/// Production wrapper over [`resolve_llm_request`] reading real process env.
#[cfg(feature = "native")]
pub fn resolve_llm_request_from_process_env(
    args: &PromptArgs,
) -> OperalaResult<Option<ResolvedLlm>> {
    resolve_llm_request(args, &|key| std::env::var(key).ok())
}

/// Owns the tokio runtime + provider backend for one CLI invocation.
/// Operala is a sync binary; all async crate calls are `block_on`'d here.
#[cfg(feature = "native")]
pub struct LlmRuntime {
    runtime: tokio::runtime::Runtime,
    backend: RigBackend,
}

#[cfg(feature = "native")]
impl LlmRuntime {
    pub fn build(resolved: &ResolvedLlm) -> OperalaResult<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("failed to start async runtime: {err}"))?;
        let credential = runtime
            .block_on(EnvCredentialSource.get_credential(resolved.provider))
            .map_err(|err| {
                format!(
                    "LLM credential error for '{}': {err}; set GREENTIC_LLM_PROVIDER and GREENTIC_LLM_API_KEY",
                    resolved.provider.as_str()
                )
            })?;
        let backend = RigBackend::new(resolved.provider, &resolved.model, &credential)
            .map_err(|err| format!("failed to initialise LLM backend: {err}"))?;
        Ok(Self { runtime, backend })
    }

    pub fn chat(
        &self,
        request: greentic_llm::ChatRequest,
    ) -> Result<greentic_llm::ChatResponse, greentic_llm::LlmError> {
        self.runtime.block_on(self.backend.chat(request))
    }
}

#[cfg(feature = "native")]
impl ChatFn for LlmRuntime {
    fn chat(&self, request: WireChatRequest) -> Result<WireChatResponse, WireChatError> {
        let llm_request = wire_request_to_llm(request);
        let llm_response = LlmRuntime::chat(self, llm_request).map_err(|err| WireChatError {
            message: err.to_string(),
        })?;
        Ok(llm_response_to_wire(llm_response))
    }

    fn tools_supported(&self) -> bool {
        self.backend.capabilities().tools
    }
}

// ---------------------------------------------------------------------------
// Conversion helpers — live next to LlmRuntime, stay native-only.
// ---------------------------------------------------------------------------

#[cfg(feature = "native")]
fn wire_request_to_llm(request: WireChatRequest) -> greentic_llm::ChatRequest {
    greentic_llm::ChatRequest {
        messages: request
            .messages
            .into_iter()
            .map(|message| greentic_llm::ChatMessage {
                role: wire_role_to_llm(message.role),
                content: message.content,
                images: vec![],
            })
            .collect(),
        tools: request
            .tools
            .into_iter()
            .map(|tool| greentic_llm::ToolDef {
                name: tool.name,
                description: tool.description,
                schema: tool.parameters,
            })
            .collect(),
        tool_choice: request.tool_choice,
        max_tokens: request.max_tokens,
        temperature: request.temperature,
    }
}

#[cfg(feature = "native")]
fn wire_role_to_llm(role: WireRole) -> greentic_llm::MessageRole {
    match role {
        WireRole::System => greentic_llm::MessageRole::System,
        WireRole::User => greentic_llm::MessageRole::User,
        WireRole::Assistant => greentic_llm::MessageRole::Assistant,
        WireRole::Tool => greentic_llm::MessageRole::Tool,
    }
}

#[cfg(feature = "native")]
fn llm_response_to_wire(response: greentic_llm::ChatResponse) -> WireChatResponse {
    WireChatResponse {
        content: response.content,
        tool_calls: response
            .tool_calls
            .into_iter()
            .map(|call| WireToolCall {
                name: call.name,
                arguments: call.arguments,
            })
            .collect(),
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::{ChatFn, WireChatError, WireChatRequest, WireChatResponse, WireToolCall};
    use std::cell::RefCell;

    /// Pure wire-native scripted chat mock. No greentic-llm dependency —
    /// works in all feature configurations including no-default-features / wasm.
    ///
    /// Responses are consumed in order; each `chat()` call pops the next one.
    /// Panics if the script is exhausted (test bug: too many chat calls).
    pub struct ScriptedChat(RefCell<Vec<WireChatResponse>>);

    impl ChatFn for ScriptedChat {
        fn chat(&self, _request: WireChatRequest) -> Result<WireChatResponse, WireChatError> {
            let mut queue = self.0.borrow_mut();
            if queue.is_empty() {
                panic!("ScriptedChat: no more scripted responses (test exhausted the script)");
            }
            Ok(queue.remove(0))
        }
    }

    /// Build a `ScriptedChat` from a list of wire responses.
    pub fn scripted_chat(responses: Vec<WireChatResponse>) -> ScriptedChat {
        ScriptedChat(RefCell::new(responses))
    }

    /// Build a wire response that calls the `emit_answers` tool.
    pub fn emit(value: serde_json::Value) -> WireChatResponse {
        WireChatResponse {
            content: String::new(),
            tool_calls: vec![WireToolCall {
                name: "emit_answers".into(),
                arguments: value,
            }],
        }
    }

    /// Build a wire response that calls the `follow_up` tool.
    // Used by native-gated driver_tests; allowed dead-code in wasm-safe builds.
    #[allow(dead_code)]
    pub fn follow_up(question: &str) -> WireChatResponse {
        WireChatResponse {
            content: String::new(),
            tool_calls: vec![WireToolCall {
                name: "follow_up".into(),
                arguments: serde_json::json!({ "question": question }),
            }],
        }
    }
}

#[cfg(test)]
#[cfg(feature = "native")]
mod driver_tests {
    use super::*;
    use crate::OperaLaExtension;
    use tests_support::{emit, follow_up, scripted_chat};

    fn fixture_sorla() -> crate::SorlaContract {
        crate::load_sorla_contract(&crate::SourceRef {
            kind: crate::SourceKind::File,
            uri: "extensions/reconciliation/examples/tenancy/sorla.yaml".into(),
            digest: None,
        })
        .unwrap()
    }

    fn fixture_reconciliation_value() -> serde_json::Value {
        let answers: crate::OperalaAnswers = serde_json::from_str(include_str!(
            "../../extensions/reconciliation/examples/tenancy/answers.json"
        ))
        .unwrap();
        serde_json::to_value(answers.capability_answers.reconciliation.unwrap()).unwrap()
    }

    #[test]
    fn driver_accepts_valid_answers_first_try() {
        let chat = scripted_chat(vec![emit(fixture_reconciliation_value())]);
        let value = infer_capability_answers(
            &chat,
            crate::EXTENSION_RECONCILIATION,
            &crate::RECONCILIATION_EXTENSION.answers_schema(),
            &fixture_sorla(),
            "reconcile rent payments",
            None,
        )
        .expect("inference succeeds");
        assert_eq!(value["source_event"], "BankTransaction");
    }

    #[test]
    fn driver_retries_on_hallucinated_binding_then_succeeds() {
        let mut bad = fixture_reconciliation_value();
        bad["source_event"] = serde_json::Value::String("ImaginaryEvent".into());
        let chat = scripted_chat(vec![emit(bad), emit(fixture_reconciliation_value())]);
        let value = infer_capability_answers(
            &chat,
            crate::EXTENSION_RECONCILIATION,
            &crate::RECONCILIATION_EXTENSION.answers_schema(),
            &fixture_sorla(),
            "reconcile rent payments",
            None,
        )
        .expect("retry succeeds");
        assert_eq!(value["source_event"], "BankTransaction");
    }

    #[test]
    fn driver_exhausts_retries_into_follow_up() {
        let mut bad = fixture_reconciliation_value();
        bad["source_event"] = serde_json::Value::String("ImaginaryEvent".into());
        let chat = scripted_chat(vec![emit(bad.clone()), emit(bad.clone()), emit(bad)]);
        let err = infer_capability_answers(
            &chat,
            crate::EXTENSION_RECONCILIATION,
            &crate::RECONCILIATION_EXTENSION.answers_schema(),
            &fixture_sorla(),
            "reconcile rent payments",
            None,
        )
        .unwrap_err();
        assert!(err.starts_with("follow-up required:"), "got: {err}");
        assert!(err.contains("ImaginaryEvent"), "got: {err}");
    }

    #[test]
    fn driver_surfaces_follow_up_immediately() {
        let chat = scripted_chat(vec![follow_up("Which event is the payment source?")]);
        let err = infer_capability_answers(
            &chat,
            crate::EXTENSION_RECONCILIATION,
            &crate::RECONCILIATION_EXTENSION.answers_schema(),
            &fixture_sorla(),
            "do something",
            None,
        )
        .unwrap_err();
        assert_eq!(
            err,
            "follow-up required: Which event is the payment source?"
        );
    }
}

/// Wasm-safe tests: no filesystem access; exercise `update_answers_for_contract`
/// directly using `parse_sorla_contract` + `include_str!` fixtures. These tests
/// compile and run under `--no-default-features` and `wasm32-wasip2`.
#[cfg(test)]
mod wasm_safe_update_tests {
    use super::*;
    use tests_support::{emit, scripted_chat};

    fn fixture_sorla_contract() -> crate::SorlaContract {
        crate::parse_sorla_contract(include_str!(
            "../../extensions/reconciliation/examples/tenancy/sorla.yaml"
        ))
        .expect("fixture sorla parses")
    }

    fn fixture_existing_answers() -> crate::OperalaAnswers {
        serde_json::from_str(include_str!(
            "../../extensions/reconciliation/examples/tenancy/answers.json"
        ))
        .expect("fixture answers parse")
    }

    /// Mirror of `update_mode_changes_only_the_instructed_field` in lib.rs, but
    /// calls the wasm-safe `update_answers_for_contract` directly — no native
    /// feature, no filesystem contract load.
    #[test]
    fn update_answers_for_contract_changes_only_the_instructed_field() {
        let sorla = fixture_sorla_contract();
        let existing = fixture_existing_answers();

        let mut updated_value =
            serde_json::to_value(existing.capability_answers.reconciliation.clone().unwrap())
                .unwrap();
        updated_value["matching"]["amount_tolerance"] = serde_json::json!(5.0);
        let chat = scripted_chat(vec![emit(updated_value)]);

        let outcome = update_answers_for_contract(
            &chat,
            &sorla,
            &existing,
            "raise the amount tolerance to 5",
        )
        .expect("wasm-safe update succeeds");

        let updated_reconciliation = outcome
            .answers
            .capability_answers
            .reconciliation
            .as_ref()
            .expect("reconciliation capability answers present");
        assert_eq!(updated_reconciliation.matching.amount_tolerance, 5.0);
        // Outer envelope must be preserved.
        assert_eq!(outcome.answers.tenant, existing.tenant);
        assert_eq!(outcome.answers.outputs.work_dir, existing.outputs.work_dir);
        // Structural diff must name the changed field.
        assert!(
            outcome
                .diff
                .iter()
                .any(|e| e.path == "capability_answers.reconciliation.matching.amount_tolerance"),
            "expected diff entry for amount_tolerance, got: {:?}",
            outcome.diff
        );
    }
}

#[cfg(test)]
#[cfg(feature = "native")]
mod tests {
    use super::*;

    fn args(provider: Option<ProviderKind>, model: Option<&str>, no_llm: bool) -> PromptArgs {
        PromptArgs {
            sorla: "s.yaml".into(),
            locale: None,
            output: None,
            tenant: None,
            team: None,
            llm_provider: provider,
            llm_model: model.map(str::to_string),
            no_llm,
            existing: None,
            in_place: false,
            prompt: "p".into(),
        }
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn no_llm_flag_wins_over_everything() {
        let resolved = resolve_llm_request(
            &args(Some(ProviderKind::Openai), Some("gpt-4o"), true),
            &|k| match k {
                "GREENTIC_LLM_PROVIDER" => Some("openai".into()),
                "GREENTIC_LLM_MODEL" => Some("gpt-4o".into()),
                _ => None,
            },
        )
        .unwrap();
        assert_eq!(resolved, None);
    }

    #[test]
    fn unconfigured_resolves_to_none() {
        assert_eq!(
            resolve_llm_request(&args(None, None, false), &no_env).unwrap(),
            None
        );
    }

    #[test]
    fn flag_beats_env() {
        let resolved = resolve_llm_request(
            &args(
                Some(ProviderKind::Anthropic),
                Some("claude-sonnet-4-6"),
                false,
            ),
            &|k| match k {
                "GREENTIC_LLM_PROVIDER" => Some("openai".into()),
                "GREENTIC_LLM_MODEL" => Some("gpt-4o".into()),
                _ => None,
            },
        )
        .unwrap()
        .expect("resolved");
        assert_eq!(resolved.provider, ProviderKind::Anthropic);
        assert_eq!(resolved.model, "claude-sonnet-4-6");
    }

    #[test]
    fn env_only_resolves() {
        let resolved = resolve_llm_request(&args(None, None, false), &|k| match k {
            "GREENTIC_LLM_PROVIDER" => Some("ollama".into()),
            "GREENTIC_LLM_MODEL" => Some("llama3:8b".into()),
            _ => None,
        })
        .unwrap()
        .expect("resolved");
        assert_eq!(resolved.provider, ProviderKind::Ollama);
        assert_eq!(resolved.model, "llama3:8b");
    }

    #[test]
    fn provider_without_model_is_a_clear_error() {
        let err = resolve_llm_request(&args(Some(ProviderKind::Openai), None, false), &no_env)
            .unwrap_err();
        assert!(err.contains("--llm-model"), "got: {err}");
        assert!(err.contains("GREENTIC_LLM_MODEL"), "got: {err}");
    }

    #[test]
    fn invalid_env_provider_is_a_clear_error() {
        let err = resolve_llm_request(&args(None, None, false), &|k| match k {
            "GREENTIC_LLM_PROVIDER" => Some("not-a-provider".into()),
            _ => None,
        })
        .unwrap_err();
        assert!(err.contains("GREENTIC_LLM_PROVIDER"), "got: {err}");
    }

    #[test]
    fn build_backend_without_api_key_is_a_clear_error() {
        // GREENTIC_LLM_API_KEY deliberately not set for provider "openai" in
        // this resolver run; EnvCredentialSource (real env) will miss unless
        // the outer environment leaks one — guard against that by checking
        // both outcomes explicitly.
        let resolved = ResolvedLlm {
            provider: ProviderKind::Openai,
            model: "gpt-4o".into(),
        };
        match LlmRuntime::build(&resolved) {
            Err(err) => {
                assert!(err.contains("GREENTIC_LLM_API_KEY"), "got: {err}");
            }
            Ok(_) => {
                // Only reachable when the developer machine has real env vars set.
                assert_eq!(
                    std::env::var("GREENTIC_LLM_PROVIDER").as_deref(),
                    Ok("openai")
                );
            }
        }
    }
}
