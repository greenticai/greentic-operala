//! Contract test: validate `describe.json` against the pinned
//! `greentic-extension-sdk-contract` (v1.2.7-research) and cross-check that
//! the `contributions.tools` names match exactly what `tools::list_tools()`
//! exposes, preventing describe/code drift.

use greentic_operala_designer_extension::tools;

/// Deserialising against `DescribeJson` validates:
/// - The schema shape (deny_unknown_fields on every nested struct)
/// - `runtime.memoryLimitMB` in [1, 1024]
/// - `runtime.components` has at least one entry
/// - every `tool.runtime_ref` resolves to a known component key
///
/// Additionally we assert the semantic invariants that matter to the designer
/// host: the correct extension id, the declared LLM role, and that the tool
/// list advertised in `describe.json` is exactly in sync with the code.
#[test]
fn describe_json_parses_against_contract() {
    let raw = include_str!("../describe.json");
    let parsed: greentic_extension_sdk_contract::DescribeJson =
        serde_json::from_str(raw).expect("describe.json must match the contract schema");

    // --- top-level discriminators ---
    assert_eq!(parsed.api_version, "greentic.ai/v2");
    assert_eq!(
        parsed.metadata.id, "greentic.operala",
        "metadata.id must be `greentic.operala`"
    );
    assert_eq!(
        parsed.metadata.version, "0.1.1",
        "metadata.version must match the crate version"
    );

    // --- LLM role ---
    // The host LLM import is the only privileged seam this extension uses.
    // The sole declared permission is the `operala_composer` LLM role
    // (round-tripped through the camelCase `llmRoles` wire key).
    assert_eq!(
        parsed.runtime.permissions.llm_roles,
        vec!["operala_composer".to_string()],
        "llmRoles must be [\"operala_composer\"]"
    );
    assert!(
        parsed.runtime.permissions.network.is_empty(),
        "OperaLa designer extension must not declare network permissions"
    );
    assert!(
        parsed.runtime.permissions.secrets.is_empty(),
        "OperaLa designer extension must not declare secrets permissions"
    );

    // --- capabilities ---
    let offered_ids: Vec<&str> = parsed
        .capabilities
        .offered
        .iter()
        .map(|cap| cap.id.as_str())
        .collect();
    assert!(
        offered_ids.contains(&"greentic:operala/design"),
        "capabilities.offered must include `greentic:operala/design`, got: {offered_ids:?}"
    );

    // --- describe.json tools vs code cross-check ---
    // This assertion prevents describe/code drift: any tool added to
    // `tools::list_tools()` that is absent from `describe.json` (or vice
    // versa) will break this test immediately.
    let described_tool_names: Vec<&str> = parsed
        .contributions
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();

    let code_tool_names: Vec<&str> = tools::list_tools()
        .into_iter()
        .map(|tool| tool.name)
        .collect();

    assert_eq!(
        described_tool_names, code_tool_names,
        "contributions.tools in describe.json must match tools::list_tools() exactly (same names, same order)"
    );
}

/// Every tool listed in `contributions.tools` must use `"invoke-tool"` as the
/// export — this is the single dispatch surface the ext-runtime calls.
#[test]
fn all_described_tools_use_invoke_tool_export() {
    let raw = include_str!("../describe.json");
    let parsed: greentic_extension_sdk_contract::DescribeJson =
        serde_json::from_str(raw).expect("describe.json must parse");

    for tool in &parsed.contributions.tools {
        assert_eq!(
            tool.export, "invoke-tool",
            "tool `{}` must export `invoke-tool`, got `{}`",
            tool.name, tool.export
        );
    }
}
