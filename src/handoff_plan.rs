//! In-memory handoff plan: every pack file as a verifiable entry, mirroring
//! the designer's sorla plan-entry contract. Wasm-safe — no filesystem.
//!
//! The caller (designer) verifies hashes and zips the entries into a `.gtpack`
//! archive using the `sorla_pack::PlanEntry` contract:
//! `{ path, sha256, content_base64 }`.

use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// A single file entry in the in-memory handoff plan.
///
/// Mirrors the `sorla_pack::PlanEntry` contract expected by the designer:
/// every field is present and the hash covers exactly `content_base64`-decoded
/// bytes so the designer can verify before zipping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanEntry {
    /// Relative path inside the pack archive (no leading `/`, no `..`).
    pub path: String,
    /// Lowercase hex SHA-256 of the decoded file bytes.
    pub sha256: String,
    /// Standard base64 encoding of the file bytes.
    pub content_base64: String,
}

/// The complete in-memory result of planning an OperaLa handoff.
///
/// `handoff` is the full `OperaLaHandoff` serialized to JSON so that
/// `handoff["schema"]` equals `"greentic.operala.handoff.v1"`. `pack_entries`
/// holds every asset file that goes into the `.gtpack` archive as verifiable
/// entries ready for the designer to zip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffPlan {
    /// Full handoff document as a JSON value.
    ///
    /// `handoff["schema"]` is always `"greentic.operala.handoff.v1"`.
    pub handoff: serde_json::Value,
    /// All pack asset entries. Each entry's `sha256` covers the decoded bytes.
    pub pack_entries: Vec<PlanEntry>,
}

/// Validate answers against the contract, build the capability handoff, and
/// return every pack asset file as an in-memory entry. The caller (designer)
/// verifies hashes and zips.
///
/// This function is wasm-safe: it performs no filesystem I/O.
///
/// # Pipeline
/// 1. `validate_answers` — schema + capability field checks
/// 2. `ExtensionRegistry::get` — resolve the declared extension
/// 3. `extension.analyse_sorla` — readiness report
/// 4. `extension.build_handoff` — capability handoff document
/// 5. Assemble pack asset entries from the same byte-sources the native wizard
///    uses in `write_operala_gtpack` / `write_handoff_assets`.
///
/// # Errors
/// Returns `Err(String)` for any validation or build failure.
pub fn build_handoff_plan(
    sorla: &crate::SorlaContract,
    answers: &crate::OperalaAnswers,
) -> crate::OperalaResult<HandoffPlan> {
    crate::validate_answers(answers)?;

    let registry = crate::ExtensionRegistry::built_in();
    let extension = registry
        .get(&answers.extension)
        .ok_or_else(|| format!("unknown OperaLa extension: `{}`", answers.extension))?;

    let readiness = extension.analyse_sorla(sorla, answers)?;
    let handoff = extension.build_handoff(sorla, answers, &readiness)?;

    let handoff_json_bytes = serde_json::to_vec_pretty(&handoff).map_err(|err| err.to_string())?;
    let handoff_value: serde_json::Value =
        serde_json::from_slice(&handoff_json_bytes).map_err(|err| err.to_string())?;

    let handoff_yaml_bytes = serde_yaml::to_string(&handoff)
        .map_err(|err| err.to_string())?
        .into_bytes();

    let mut entries = Vec::new();

    // operala/operala-handoff.json — primary handoff document
    entries.push(plan_entry(
        "operala/operala-handoff.json",
        &handoff_json_bytes,
    ));

    // operala/operala.yaml — YAML rendition of the handoff
    entries.push(plan_entry("operala/operala.yaml", &handoff_yaml_bytes));

    // operala/flows/<flow>.flow.yaml — one stub per declared flow
    for flow in &handoff.flows {
        let flow_id = flow.trim_end_matches(".flow.yaml");
        let flow_yaml = format!(
            "id: {flow_id}\ntype: messaging\nschema_version: 2\nstart: operala_handoff\nnodes:\n  operala_handoff:\n    op: {{}}\n    routing: out\n"
        );
        entries.push(plan_entry(
            &format!("operala/flows/{flow}"),
            flow_yaml.as_bytes(),
        ));
    }

    // operala/schemas/<name>.schema.json — one JSON schema per declared schema
    for (name, schema) in &handoff.schemas {
        let schema_bytes = serde_json::to_vec_pretty(schema).map_err(|err| err.to_string())?;
        entries.push(plan_entry(
            &format!("operala/schemas/{name}.schema.json"),
            &schema_bytes,
        ));
    }

    Ok(HandoffPlan {
        handoff: handoff_value,
        pack_entries: entries,
    })
}

/// Build a single [`PlanEntry`] from a path and raw bytes.
///
/// Computes the SHA-256 of `bytes` and base64-encodes the content.
fn plan_entry(path: &str, bytes: &[u8]) -> PlanEntry {
    let digest = crate::sha256_hex(bytes);
    let content_base64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    PlanEntry {
        path: path.to_string(),
        sha256: digest,
        content_base64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_plan_produces_verifiable_entries() {
        let yaml = include_str!("../extensions/reconciliation/examples/tenancy/sorla.yaml");
        let sorla = crate::parse_sorla_contract(yaml).unwrap();
        let answers: crate::OperalaAnswers = serde_json::from_str(include_str!(
            "../extensions/reconciliation/examples/tenancy/answers.json"
        ))
        .unwrap();
        let plan = build_handoff_plan(&sorla, &answers).unwrap();
        assert_eq!(plan.handoff["schema"], "greentic.operala.handoff.v1");
        assert!(!plan.pack_entries.is_empty());
        for entry in &plan.pack_entries {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&entry.content_base64)
                .unwrap();
            let digest = crate::sha256_hex(&bytes);
            assert_eq!(digest, entry.sha256, "entry {} hash mismatch", entry.path);
            assert!(
                !entry.path.starts_with('/') && !entry.path.contains(".."),
                "entry path must be relative and safe: {}",
                entry.path
            );
        }
        assert!(
            plan.pack_entries
                .iter()
                .any(|e| e.path.ends_with("operala-handoff.json")),
            "pack_entries must include the handoff JSON"
        );
    }

    #[test]
    fn handoff_plan_entry_paths_are_safe_and_relative() {
        let yaml = include_str!("../extensions/reconciliation/examples/tenancy/sorla.yaml");
        let sorla = crate::parse_sorla_contract(yaml).unwrap();
        let answers: crate::OperalaAnswers = serde_json::from_str(include_str!(
            "../extensions/reconciliation/examples/tenancy/answers.json"
        ))
        .unwrap();
        let plan = build_handoff_plan(&sorla, &answers).unwrap();
        for entry in &plan.pack_entries {
            assert!(!entry.path.starts_with('/'), "path must be relative");
            assert!(!entry.path.contains(".."), "path must not traverse");
        }
    }

    #[test]
    fn handoff_plan_includes_all_expected_asset_kinds() {
        let yaml = include_str!("../extensions/reconciliation/examples/tenancy/sorla.yaml");
        let sorla = crate::parse_sorla_contract(yaml).unwrap();
        let answers: crate::OperalaAnswers = serde_json::from_str(include_str!(
            "../extensions/reconciliation/examples/tenancy/answers.json"
        ))
        .unwrap();
        let plan = build_handoff_plan(&sorla, &answers).unwrap();
        let paths: Vec<&str> = plan.pack_entries.iter().map(|e| e.path.as_str()).collect();

        // Handoff document
        assert!(paths.contains(&"operala/operala-handoff.json"));
        // YAML rendition
        assert!(paths.contains(&"operala/operala.yaml"));
        // At least one flow stub
        assert!(
            paths.iter().any(|p| p.starts_with("operala/flows/")),
            "must have at least one flow entry"
        );
        // At least one schema
        assert!(
            paths.iter().any(|p| p.starts_with("operala/schemas/")),
            "must have at least one schema entry"
        );
    }

    #[test]
    fn plan_entry_helper_produces_consistent_hash() {
        let content = b"hello operala";
        let entry = plan_entry("test/hello.txt", content);
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&entry.content_base64)
            .expect("base64 decodes");
        assert_eq!(decoded, content);
        let expected = crate::sha256_hex(content);
        assert_eq!(entry.sha256, expected);
        assert_eq!(entry.path, "test/hello.txt");
    }
}
