//! Wasm component glue for the OperaLa Designer extension.
//!
//! Compiled only for `wasm32` (gated by the `#[cfg]` on the `mod component`
//! declaration in `lib.rs`). Wires the generated WIT world exports to the
//! pure-Rust JSON-boundary functions in `crate::*`, and provides the
//! [`HostLlmChat`] adapter that backs the operala inference engine with the
//! designer host's per-tenant LLM import.

use crate::bindings::exports::greentic::extension_base::{lifecycle, manifest};
use crate::bindings::exports::greentic::extension_design::{
    knowledge, prompting, tools, validation,
};
use crate::bindings::greentic::extension_base::types;
use crate::bindings::greentic::extension_host::llm as host_llm;
use greentic_operala as op;

/// Marker type the generated `bindings::export!` macro attaches Guest impls to.
pub(crate) struct Component;

// --- extension-base: manifest + lifecycle ------------------------------------

impl manifest::Guest for Component {
    fn get_identity() -> types::ExtensionIdentity {
        types::ExtensionIdentity {
            id: "greentic.operala".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            kind: types::Kind::Design,
        }
    }

    fn get_offered() -> Vec<types::CapabilityRef> {
        vec![types::CapabilityRef {
            id: "greentic:operala/composition".to_string(),
            version: "1.0.0".to_string(),
        }]
    }

    fn get_required() -> Vec<types::CapabilityRef> {
        Vec::new()
    }
}

impl lifecycle::Guest for Component {
    fn init(_config_json: String) -> Result<(), types::ExtensionError> {
        Ok(())
    }

    fn shutdown() {}
}

// --- extension-design: tools -------------------------------------------------

impl tools::Guest for Component {
    fn list_tools() -> Vec<tools::ToolDefinition> {
        crate::list_tools()
            .into_iter()
            .map(|t| tools::ToolDefinition {
                name: t.name.to_string(),
                description: t.description.to_string(),
                input_schema_json: t.input_schema_json,
                output_schema_json: t.output_schema_json,
                capabilities: None,
                agentic_worker_metadata: None,
            })
            .collect()
    }

    fn invoke_tool(name: String, args_json: String) -> Result<String, types::ExtensionError> {
        crate::invoke_tool(&name, &args_json).map_err(types::ExtensionError::InvalidInput)
    }
}

// --- extension-design: validation --------------------------------------------

impl validation::Guest for Component {
    fn validate_content(
        _content_type: String,
        _content_json: String,
    ) -> validation::ValidateResult {
        validation::ValidateResult {
            valid: true,
            diagnostics: vec![],
        }
    }
}

// --- extension-design: prompting ---------------------------------------------

impl prompting::Guest for Component {
    fn system_prompt_fragments() -> Vec<prompting::PromptFragment> {
        vec![prompting::PromptFragment {
            section: "operala.principles".to_string(),
            content_markdown: "When using OperaLa tools, select the capability that best matches \
                the operator's intent (reconciliation or bulk_ingest), then generate answers \
                anchored to SoRLa catalog identifiers. Produce a handoff pack only after the \
                answers have been validated."
                .to_string(),
            priority: 10,
        }]
    }
}

// --- extension-design: knowledge ---------------------------------------------

impl knowledge::Guest for Component {
    fn list_entries(_category_filter: Option<String>) -> Vec<knowledge::EntrySummary> {
        Vec::new()
    }

    fn get_entry(id: String) -> Result<knowledge::Entry, types::ExtensionError> {
        Err(types::ExtensionError::InvalidInput(format!(
            "knowledge entry '{id}' not found"
        )))
    }

    fn suggest_entries(_query: String, _limit: u32) -> Vec<knowledge::EntrySummary> {
        Vec::new()
    }
}

// --- HostLlmChat adapter -----------------------------------------------------

/// `op::inference::ChatFn` backed by the designer host's `llm` import.
/// Credentials and provider/model selection are host-owned (resolved from the
/// extension's declared `operala_composer` role per tenant).
pub struct HostLlmChat;

impl op::inference::ChatFn for HostLlmChat {
    fn tools_supported(&self) -> bool {
        false
    }

    fn chat(&self, request: op::ChatRequest) -> Result<op::ChatResponse, op::LlmError> {
        let mut system = String::new();
        let mut messages = Vec::new();

        for m in &request.messages {
            match m.role {
                op::MessageRole::System => {
                    if !system.is_empty() {
                        system.push('\n');
                    }
                    system.push_str(&m.content);
                }
                op::MessageRole::User => {
                    messages.push(host_llm::LlmMessage {
                        role: "user".to_string(),
                        content: m.content.clone(),
                    });
                }
                op::MessageRole::Assistant => {
                    messages.push(host_llm::LlmMessage {
                        role: "assistant".to_string(),
                        content: m.content.clone(),
                    });
                }
                op::MessageRole::Tool => {
                    messages.push(host_llm::LlmMessage {
                        role: "user".to_string(),
                        content: m.content.clone(),
                    });
                }
            }
        }

        let req = host_llm::LlmRequest {
            role_hint: Some("operala_composer".to_string()),
            system_prompt: system,
            messages,
            response_format: Some(host_llm::ResponseFormat::Json),
        };

        match host_llm::complete(&req) {
            Ok(r) => Ok(op::ChatResponse {
                content: r.content,
                tool_calls: vec![],
                finish_reason: op::FinishReason::Stop,
            }),
            Err(e) => Err(op::LlmError::Transport(format!(
                "host LLM completion failed: {e}"
            ))),
        }
    }
}

crate::bindings::export!(Component with_types_in crate::bindings);
