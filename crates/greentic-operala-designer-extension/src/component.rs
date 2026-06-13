//! Wasm component glue for the OperaLa Designer extension.
//!
//! This module is compiled only for `wasm32` (gated by the `#[cfg]` on the
//! `mod component` declaration in `lib.rs`). It wires the generated WIT world
//! exports to the pure-Rust tool surface in `crate::tools`, and provides the
//! [`HostLlm`] adapter that backs OperaLa's inference pipeline with the
//! designer host's per-tenant LLM import.

use crate::bindings::exports::greentic::extension_base::{lifecycle, manifest};
use crate::bindings::exports::greentic::extension_design::{
    knowledge, prompting, tools, validation,
};
use crate::bindings::greentic::extension_base::types;
use crate::bindings::greentic::extension_host::llm as host_llm;
use greentic_operala::inference::{
    ChatFn, WireChatError, WireChatRequest, WireChatResponse, WireRole,
};

/// Marker type the generated `bindings::export!` macro attaches Guest impls to.
pub(crate) struct Component;

// --- extension-base: manifest + lifecycle ----------------------------------

impl manifest::Guest for Component {
    fn get_identity() -> types::ExtensionIdentity {
        types::ExtensionIdentity {
            id: "greentic.operala".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            kind: types::Kind::Design,
        }
    }

    fn get_offered() -> Vec<types::CapabilityRef> {
        // No capabilities are offered until Task 5 lands the OperaLa tools;
        // keep this in sync with describe.json when it appears.
        Vec::new()
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

// --- extension-design: tools ------------------------------------------------

impl tools::Guest for Component {
    fn list_tools() -> Vec<tools::ToolDefinition> {
        crate::tools::list_tools()
            .into_iter()
            .map(|tool| tools::ToolDefinition {
                name: tool.name.to_string(),
                description: tool.description.to_string(),
                input_schema_json: tool.input_schema_json,
                output_schema_json: tool.output_schema_json,
                // Legacy `none` defaults to `["flow"]` host-side; Task 5
                // decides the chat-vs-studio split per tool.
                capabilities: None,
                agentic_worker_metadata: None,
            })
            .collect()
    }

    fn invoke_tool(name: String, args_json: String) -> Result<String, types::ExtensionError> {
        // Reuse the native JSON dispatch; map the stringly error into the WIT
        // variant the same way the reference sorla extension does.
        crate::tools::invoke_tool(&name, &args_json).map_err(types::ExtensionError::InvalidInput)
    }
}

// --- extension-design: validation -------------------------------------------

impl validation::Guest for Component {
    fn validate_content(content_type: String, _content_json: String) -> validation::ValidateResult {
        // No OperaLa content validators are wired yet (Task 5); reject every
        // content type explicitly rather than silently passing content.
        validation::ValidateResult {
            valid: false,
            diagnostics: vec![types::Diagnostic {
                severity: types::Severity::Error,
                code: "designer.content.unsupported".to_string(),
                message: format!("unsupported content type `{content_type}`"),
                path: None,
            }],
        }
    }
}

// --- extension-design: prompting --------------------------------------------

impl prompting::Guest for Component {
    fn system_prompt_fragments() -> Vec<prompting::PromptFragment> {
        // OperaLa prompt fragments land with the Task 5 tool surface.
        Vec::new()
    }
}

// --- extension-design: knowledge --------------------------------------------

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

// --- HostLlm adapter --------------------------------------------------------

/// [`ChatFn`] backed by the designer host's `llm` import. Credentials and
/// provider/model selection are host-owned, resolved per tenant from the
/// extension's declared `operala_composer` LLM role.
///
/// The host contract (`greentic:extension-host/llm@0.1.0`) carries no tool
/// specs or tool-call results, so [`ChatFn::tools_supported`] reports `false`
/// and the inference pipeline takes its no-tools fallback. `max_tokens` and
/// `temperature` likewise have no host-side counterpart and are dropped.
// Not referenced yet: Task 5 routes the inference-backed tools through this
// adapter (mirroring sorla's `prompt_llm()` seam).
#[allow(dead_code)]
pub struct HostLlm;

impl ChatFn for HostLlm {
    fn chat(&self, request: WireChatRequest) -> Result<WireChatResponse, WireChatError> {
        // The host request separates the system prompt from the message list,
        // while the wire request carries system text as ordinary messages:
        // fold system messages into the dedicated field and forward the rest.
        let mut system_sections: Vec<String> = Vec::new();
        let mut messages: Vec<host_llm::LlmMessage> = Vec::new();
        for message in request.messages {
            let role = match message.role {
                WireRole::System => {
                    system_sections.push(message.content);
                    continue;
                }
                WireRole::User => "user",
                WireRole::Assistant => "assistant",
                // The host contract only knows system/user/assistant. Tool
                // result messages only occur on the native tool-calling path,
                // which this adapter does not advertise (`tools_supported` is
                // `false`); map any stray tool message to "user" so its
                // content is never silently dropped.
                WireRole::Tool => "user",
            };
            messages.push(host_llm::LlmMessage {
                role: role.to_string(),
                content: message.content,
            });
        }

        let host_request = host_llm::LlmRequest {
            // Wire name of the LLM role declared in the extension's
            // `describe.runtime.permissions.llmRoles` list.
            role_hint: Some("operala_composer".to_string()),
            system_prompt: system_sections.join("\n\n"),
            messages,
            response_format: None,
        };

        match host_llm::complete(&host_request) {
            // The host contract has no tool-call channel, so responses are
            // always plain content.
            Ok(response) => Ok(WireChatResponse {
                content: response.content,
                tool_calls: Vec::new(),
            }),
            // Surface the host failure verbatim with a stable prefix so
            // callers can distinguish host LLM errors.
            Err(message) => Err(WireChatError {
                message: format!("host LLM completion failed: {message}"),
            }),
        }
    }

    fn tools_supported(&self) -> bool {
        // `greentic:extension-host/llm@0.1.0` requests carry only role-hint,
        // system-prompt, messages, and response-format — no tool specs — and
        // responses carry only content and token usage — no tool calls. The
        // inference pipeline must take its no-tools fallback.
        false
    }
}

crate::bindings::export!(Component with_types_in crate::bindings);
