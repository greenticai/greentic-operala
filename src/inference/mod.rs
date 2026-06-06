//! LLM-backed inference for `prompt` — config resolution, SoRLa catalog,
//! chat session, validation gate. The deterministic keyword path stays in
//! `lib.rs`; this module is only entered when an LLM is configured.

pub mod catalog;

use crate::{OperalaResult, PromptArgs};
use greentic_llm::{CredentialSource, EnvCredentialSource, LlmProvider, ProviderKind, RigBackend};

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
            Some(raw) => Some(raw.parse::<ProviderKind>().map_err(|err| {
                format!("invalid GREENTIC_LLM_PROVIDER: {err}")
            })?),
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
        let resolved = resolve_llm_request(&args(Some(ProviderKind::Openai), Some("gpt-4o"), true), &|k| {
            match k {
                "GREENTIC_LLM_PROVIDER" => Some("openai".into()),
                "GREENTIC_LLM_MODEL" => Some("gpt-4o".into()),
                _ => None,
            }
        })
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
            &args(Some(ProviderKind::Anthropic), Some("claude-sonnet-4-6"), false),
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
