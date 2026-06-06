//! LLM-backed inference for `prompt` — config resolution, SoRLa catalog,
//! chat session, validation gate. The deterministic keyword path stays in
//! `lib.rs`; this module is only entered when an LLM is configured.

pub mod catalog;
pub mod session;
pub mod validate;

use crate::{OperalaResult, PromptArgs, SorlaContract, follow_up_required};
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
                        messages.push(greentic_llm::ChatMessage {
                            role: greentic_llm::MessageRole::User,
                            content: format!(
                                "Your answers failed validation:\n- {}\nEmit a corrected complete answers object via emit_answers, binding only to catalog identifiers.",
                                errors.join("\n- ")
                            ),
                            images: vec![],
                        });
                        last_errors = errors;
                    }
                }
            }
            Err(parse_error) => {
                messages.push(greentic_llm::ChatMessage {
                    role: greentic_llm::MessageRole::User,
                    content: format!(
                        "{parse_error}\nRespond again using the emit_answers or follow_up tool."
                    ),
                    images: vec![],
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

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedLlm {
    pub provider: ProviderKind,
    pub model: String,
}

/// Resolve whether this invocation uses an LLM. Precedence: `--no-llm` >
/// flags > `GREENTIC_LLM_PROVIDER`/`GREENTIC_LLM_MODEL` env > unset (None →
/// deterministic keyword path). Env access is injected for testability.
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
pub fn resolve_llm_request_from_process_env(
    args: &PromptArgs,
) -> OperalaResult<Option<ResolvedLlm>> {
    resolve_llm_request(args, &|key| std::env::var(key).ok())
}

/// Owns the tokio runtime + provider backend for one CLI invocation.
/// Operala is a sync binary; all async crate calls are `block_on`'d here.
pub struct LlmRuntime {
    runtime: tokio::runtime::Runtime,
    backend: RigBackend,
}

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

/// Test seam: session functions take `&dyn ChatFn` so tests inject the
/// scripted mock without a real backend or runtime.
pub trait ChatFn {
    fn chat(
        &self,
        request: greentic_llm::ChatRequest,
    ) -> Result<greentic_llm::ChatResponse, greentic_llm::LlmError>;

    /// Whether the underlying provider supports native tool calling.
    /// Defaults to true; `LlmRuntime` reports the real capability.
    fn tools_supported(&self) -> bool {
        true
    }
}

impl ChatFn for LlmRuntime {
    fn chat(
        &self,
        request: greentic_llm::ChatRequest,
    ) -> Result<greentic_llm::ChatResponse, greentic_llm::LlmError> {
        LlmRuntime::chat(self, request)
    }

    fn tools_supported(&self) -> bool {
        self.backend.capabilities().tools
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::ChatFn;
    use greentic_llm::mock::{TestLlmProvider, TestLlmProviderBuilder};
    use greentic_llm::{ChatResponse, FinishReason, ToolCall};

    /// Test ChatFn backed by the scripted mock. Sync-only: owns its own current-thread runtime, so it must not be driven from inside an existing tokio runtime.
    pub struct ScriptedChat(TestLlmProvider, tokio::runtime::Runtime);

    impl ChatFn for ScriptedChat {
        fn chat(
            &self,
            request: greentic_llm::ChatRequest,
        ) -> Result<ChatResponse, greentic_llm::LlmError> {
            self.1
                .block_on(greentic_llm::LlmProvider::chat(&self.0, request))
        }
    }

    pub fn scripted_chat(responses: Vec<ChatResponse>) -> ScriptedChat {
        let mut builder = TestLlmProviderBuilder::new();
        for response in responses {
            builder = builder.script_response(response);
        }
        ScriptedChat(
            builder.build(),
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
        )
    }

    pub fn emit(value: serde_json::Value) -> ChatResponse {
        ChatResponse {
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: "c".into(),
                name: "emit_answers".into(),
                arguments: value,
            }],
            finish_reason: FinishReason::ToolCalls,
        }
    }

    pub fn follow_up(question: &str) -> ChatResponse {
        ChatResponse {
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: "c".into(),
                name: "follow_up".into(),
                arguments: serde_json::json!({ "question": question }),
            }],
            finish_reason: FinishReason::ToolCalls,
        }
    }
}

#[cfg(test)]
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

#[cfg(test)]
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
