//! Backend-agnostic LLM wire types. `ChatFn` speaks these so the inference
//! pipeline compiles without greentic-llm. The native `LlmRuntime` converts
//! to/from greentic-llm types at its own boundary; a future designer-extension
//! will convert to/from WIT host-llm types instead.
//!
//! Field inventory (derived from actual inference pipeline usage):
//! - `WireRole`: System + User (inference messages); Assistant is unused today
//!   but included so the seam stays complete for tool-call round-trips.
//! - `WireChatMessage`: `role` + `content` (String). `images` is always empty
//!   in the pipeline; omitted (YAGNI).
//! - `WireToolSpec`: `name`, `description`, `parameters` (JSON schema Value).
//!   Corresponds to `ToolDef` on the greentic-llm side.
//! - `WireChatRequest`: `messages`, `tools`, `tool_choice` (Some("required")/None),
//!   `max_tokens` (Some(4096)/Some(64)), `temperature` (Some(0.0)).
//! - `WireToolCall`: `name` + `arguments` (Value). `id` is never read by the
//!   inference pipeline (only constructed in test helpers); omitted.
//! - `WireChatResponse`: `content` (Option<String>) + `tool_calls`. `finish_reason`
//!   is never accessed by `parse_outcome`/`parse_classification`; omitted.
//! - `WireChatError`: wraps a message string. `thiserror` is NOT a dep; impl'd
//!   by hand.

use serde::{Deserialize, Serialize};

/// Role of a chat participant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireRole {
    System,
    User,
    Assistant,
    Tool,
}

/// A single chat message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireChatMessage {
    pub role: WireRole,
    pub content: String,
}

/// Tool/function spec passed to the LLM.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Request sent through the `ChatFn` seam.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireChatRequest {
    pub messages: Vec<WireChatMessage>,
    #[serde(default)]
    pub tools: Vec<WireToolSpec>,
    /// `Some("required")` forces a tool call; `None` lets the model choose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
    /// Maximum tokens to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

/// A tool/function call emitted by the model in a response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Response received through the `ChatFn` seam.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireChatResponse {
    /// Text content; empty string when the model only calls tools.
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tool_calls: Vec<WireToolCall>,
}

/// Error returned by a `ChatFn` implementation.
#[derive(Debug)]
pub struct WireChatError {
    pub message: String,
}

impl std::fmt::Display for WireChatError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for WireChatError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_request_roundtrips_serde() {
        let request = WireChatRequest {
            messages: vec![WireChatMessage {
                role: WireRole::System,
                content: "be brief".into(),
            }],
            tools: vec![WireToolSpec {
                name: "emit_answers".into(),
                description: "emit".into(),
                parameters: serde_json::json!({"type": "object"}),
            }],
            temperature: Some(0.0),
            tool_choice: None,
            max_tokens: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        let back: WireChatRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.messages[0].content, "be brief");
        assert_eq!(back.tools[0].name, "emit_answers");
    }

    #[test]
    fn wire_response_roundtrips_serde() {
        let response = WireChatResponse {
            content: String::new(),
            tool_calls: vec![WireToolCall {
                name: "emit_answers".into(),
                arguments: serde_json::json!({"source_event": "BankTransaction"}),
            }],
        };
        let json = serde_json::to_string(&response).unwrap();
        let back: WireChatResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tool_calls[0].name, "emit_answers");
        assert_eq!(
            back.tool_calls[0].arguments["source_event"],
            "BankTransaction"
        );
    }

    #[test]
    fn wire_error_displays_message() {
        let error = WireChatError {
            message: "backend offline".into(),
        };
        assert_eq!(error.to_string(), "backend offline");
    }

    #[test]
    fn wire_role_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&WireRole::System).unwrap(),
            "\"system\""
        );
        assert_eq!(serde_json::to_string(&WireRole::User).unwrap(), "\"user\"");
    }
}
