//! LLM-backed inference for `prompt` — config resolution, SoRLa catalog,
//! chat session, validation gate. The deterministic keyword path stays in
//! `lib.rs`; this module is only entered when an LLM is configured.

use crate::{OperalaResult, PromptArgs};
use greentic_llm::ProviderKind;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
}
