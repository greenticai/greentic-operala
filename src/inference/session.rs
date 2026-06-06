//! Chat orchestration: builds requests, parses `emit_answers` / `follow_up`
//! outcomes from tool calls (tools-capable providers) or JSON content (the
//! fallback for providers with `capabilities().tools == false`).

use greentic_llm::{ChatMessage, ChatRequest, ChatResponse, MessageRole, ToolDef};
use serde_json::Value;

use crate::OperalaResult;

#[derive(Debug, PartialEq)]
pub enum InferenceOutcome {
    Answers(Value),
    FollowUp(String),
}

/// Strip an optional markdown code fence and parse the content as JSON.
fn content_as_json(content: &str) -> Option<Value> {
    let trimmed = content.trim();
    let inner = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|rest| rest.strip_suffix("```"))
        .map(str::trim)
        .unwrap_or(trimmed);
    serde_json::from_str(inner).ok()
}

/// Parse one chat response into an inference outcome. Tool calls win; JSON
/// content is the fallback for non-tools providers.
pub fn parse_outcome(response: &ChatResponse) -> OperalaResult<InferenceOutcome> {
    if let Some(call) = response.tool_calls.first() {
        return match call.name.as_str() {
            "emit_answers" => Ok(InferenceOutcome::Answers(call.arguments.clone())),
            "follow_up" => {
                let question = call.arguments["question"]
                    .as_str()
                    .unwrap_or("Which SoRLa bindings should be used?")
                    .to_string();
                Ok(InferenceOutcome::FollowUp(question))
            }
            other => Err(format!(
                "LLM called unknown tool '{other}'; expected emit_answers or follow_up"
            )),
        };
    }
    if let Some(json) = content_as_json(&response.content) {
        if let Some(answers) = json.get("emit_answers") {
            return Ok(InferenceOutcome::Answers(answers.clone()));
        }
        if let Some(question) = json.get("follow_up").and_then(Value::as_str) {
            return Ok(InferenceOutcome::FollowUp(question.to_string()));
        }
    }
    Err(format!(
        "LLM response had neither an emit_answers/follow_up tool call nor a JSON object with emit_answers/follow_up; content was: {}",
        response.content.chars().take(200).collect::<String>()
    ))
}

/// Parse the capability-classification response: `{"capability": "<id>"}`.
/// Returns None for "unknown" (caller falls back to follow_up_required).
pub fn parse_classification(response: &ChatResponse) -> OperalaResult<Option<String>> {
    let json = content_as_json(&response.content).ok_or_else(|| {
        format!(
            "LLM classification was not JSON; content was: {}",
            response.content.chars().take(200).collect::<String>()
        )
    })?;
    match json.get("capability").and_then(Value::as_str) {
        Some("unknown") | None => Ok(None),
        Some(capability) => Ok(Some(capability.to_string())),
    }
}

/// System prompt shared by all inference calls.
pub const SYSTEM_PROMPT: &str = "You are OperaLa, an authoring assistant for operational business logic on the Greentic platform. You bind operational capabilities to a SoRLa (system-of-record) contract. Bind ONLY to identifiers present in the provided SoRLa catalog — never invent record, event, action, or endpoint names. If the intent is ambiguous or a required binding has no plausible catalog match, use follow_up to ask ONE clarifying question.";

pub fn emit_answers_tool(schema: Value) -> ToolDef {
    ToolDef {
        name: "emit_answers".into(),
        description: "Emit the complete capability answers object, binding every field to catalog identifiers.".into(),
        schema,
    }
}

pub fn follow_up_tool() -> ToolDef {
    ToolDef {
        name: "follow_up".into(),
        description:
            "Ask the user one clarifying question when the intent or a binding is ambiguous.".into(),
        schema: serde_json::json!({
            "type": "object",
            "required": ["question"],
            "properties": { "question": { "type": "string" } }
        }),
    }
}

/// Build the initial message list for new-mode or update-mode inference.
pub fn inference_messages(
    catalog: &Value,
    intent: &str,
    existing_answers: Option<&Value>,
) -> Vec<ChatMessage> {
    let mut user = format!(
        "SoRLa catalog (the ONLY identifiers you may bind to):\n{}\n\n",
        serde_json::to_string_pretty(catalog).unwrap_or_default()
    );
    match existing_answers {
        Some(existing) => {
            user.push_str(&format!(
                "Existing capability answers:\n{}\n\nChange instruction: {intent}\n\nApply the instruction to the existing answers. Change ONLY what the instruction asks for; keep every other field exactly as it is. Emit the COMPLETE updated answers object via emit_answers.",
                serde_json::to_string_pretty(existing).unwrap_or_default()
            ));
        }
        None => {
            user.push_str(&format!(
                "Operator intent: {intent}\n\nInfer the complete capability answers object and emit it via emit_answers."
            ));
        }
    }
    vec![
        ChatMessage {
            role: MessageRole::System,
            content: SYSTEM_PROMPT.into(),
            images: vec![],
        },
        ChatMessage {
            role: MessageRole::User,
            content: user,
            images: vec![],
        },
    ]
}

/// Build a ChatRequest. Tools-capable providers get real tools; others get a
/// JSON-mode instruction appended to the system message.
pub fn build_request(
    messages: Vec<ChatMessage>,
    answers_schema: Value,
    tools_supported: bool,
) -> ChatRequest {
    if tools_supported {
        ChatRequest {
            messages,
            tools: vec![emit_answers_tool(answers_schema), follow_up_tool()],
            tool_choice: Some("required".into()),
            max_tokens: Some(4096),
            temperature: Some(0.0),
        }
    } else {
        let mut messages = messages;
        if let Some(system) = messages.first_mut() {
            system.content.push_str(&format!(
                "\n\nRespond with ONLY a JSON object, no prose: either {{\"emit_answers\": <object matching this schema: {}>}} or {{\"follow_up\": \"<one question>\"}}.",
                answers_schema
            ));
        }
        ChatRequest {
            messages,
            tools: vec![],
            tool_choice: None,
            max_tokens: Some(4096),
            temperature: Some(0.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use greentic_llm::{FinishReason, ToolCall};

    fn tool_response(name: &str, arguments: Value) -> ChatResponse {
        ChatResponse {
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: name.into(),
                arguments,
            }],
            finish_reason: FinishReason::ToolCalls,
        }
    }

    fn text_response(content: &str) -> ChatResponse {
        ChatResponse {
            content: content.into(),
            tool_calls: vec![],
            finish_reason: FinishReason::Stop,
        }
    }

    #[test]
    fn emit_answers_tool_call_parses_to_answers() {
        let outcome = parse_outcome(&tool_response(
            "emit_answers",
            serde_json::json!({"name": "x"}),
        ))
        .unwrap();
        assert_eq!(
            outcome,
            InferenceOutcome::Answers(serde_json::json!({"name": "x"}))
        );
    }

    #[test]
    fn follow_up_tool_call_parses_to_follow_up() {
        let outcome = parse_outcome(&tool_response(
            "follow_up",
            serde_json::json!({"question": "Which event?"}),
        ))
        .unwrap();
        assert_eq!(outcome, InferenceOutcome::FollowUp("Which event?".into()));
    }

    #[test]
    fn json_content_emit_answers_parses() {
        let outcome =
            parse_outcome(&text_response(r#"{"emit_answers": {"name": "from_json"}}"#)).unwrap();
        assert_eq!(
            outcome,
            InferenceOutcome::Answers(serde_json::json!({"name": "from_json"}))
        );
    }

    #[test]
    fn json_content_follow_up_parses() {
        let outcome = parse_outcome(&text_response(r#"{"follow_up": "Which record?"}"#)).unwrap();
        assert_eq!(outcome, InferenceOutcome::FollowUp("Which record?".into()));
    }

    #[test]
    fn json_content_wrapped_in_code_fence_parses() {
        let outcome = parse_outcome(&text_response(
            "```json\n{\"emit_answers\": {\"name\": \"fenced\"}}\n```",
        ))
        .unwrap();
        assert_eq!(
            outcome,
            InferenceOutcome::Answers(serde_json::json!({"name": "fenced"}))
        );
    }

    #[test]
    fn garbage_content_is_a_parse_error() {
        let err =
            parse_outcome(&text_response("I think you should use BankTransaction")).unwrap_err();
        assert!(err.contains("emit_answers"), "got: {err}");
    }

    #[test]
    fn unknown_tool_is_a_parse_error() {
        let err =
            parse_outcome(&tool_response("delete_everything", serde_json::json!({}))).unwrap_err();
        assert!(err.contains("delete_everything"), "got: {err}");
    }

    #[test]
    fn classification_parses_known_capability() {
        assert_eq!(
            parse_classification(&text_response(r#"{"capability": "reconciliation"}"#)).unwrap(),
            Some("reconciliation".to_string())
        );
        assert_eq!(
            parse_classification(&text_response(r#"{"capability": "unknown"}"#)).unwrap(),
            None
        );
    }

    #[test]
    fn classification_passes_through_unexpected_capability_for_caller_to_guard() {
        assert_eq!(
            parse_classification(&text_response(r#"{"capability": "reporting"}"#)).unwrap(),
            Some("reporting".to_string())
        );
    }
}
