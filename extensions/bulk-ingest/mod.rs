use crate::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::env;

pub struct BulkIngestExtension;

impl OperaLaExtension for BulkIngestExtension {
    fn id(&self) -> &'static str {
        EXTENSION_BULK_INGEST
    }

    fn capability(&self) -> &'static str {
        "bulk_ingest"
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn qa_schema(&self) -> Value {
        json!({
            "schema": "greentic.qa.schema.v1",
            "flow": "operala.bulk_ingest",
            "sections": [
                {
                    "id": "bulk_ingest.scope",
                    "title": "Bulk ingest scope",
                    "questions": [
                        {"id": "name", "kind": "text", "required": true},
                        {"id": "record_collections", "kind": "mapping", "required": true},
                        {"id": "actions", "kind": "mapping", "required": false},
                        {"id": "expected_counts", "kind": "mapping", "required": false}
                    ]
                },
                {
                    "id": "bulk_ingest.validation",
                    "title": "Bulk ingest validation",
                    "questions": [
                        {"id": "atomic", "kind": "boolean", "required": true},
                        {"id": "dry_run", "kind": "boolean", "required": true},
                        {"id": "require_unique_ids", "kind": "boolean", "required": true},
                        {"id": "validate_references", "kind": "boolean", "required": true}
                    ]
                }
            ]
        })
    }

    fn answers_schema(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["name", "input_modes", "record_collections", "actions", "validation"],
            "properties": {
                "name": { "type": "string", "pattern": "^[a-z][a-z0-9_]*$" },
                "input_modes": { "type": "array", "items": { "enum": ["batch"] }, "minItems": 1 },
                "record_collections": { "type": "object", "description": "collection name → SoRLa record id from catalog.records", "additionalProperties": { "type": "string", "minLength": 1 } },
                "actions": { "type": "object", "description": "operation name → SoRLa action id from catalog.actions", "additionalProperties": { "type": "string", "minLength": 1 } },
                "expected_counts": { "type": "object", "additionalProperties": { "type": "integer", "minimum": 0 } },
                "validation": {
                    "type": "object",
                    "required": ["atomic", "dry_run", "require_unique_ids", "validate_references"],
                    "properties": {
                        "atomic": { "type": "boolean" },
                        "dry_run": { "type": "boolean" },
                        "require_unique_ids": { "type": "boolean" },
                        "validate_references": { "type": "boolean" }
                    }
                }
            }
        })
    }

    fn analyse_sorla(
        &self,
        sorla: &SorlaContract,
        answers: &OperalaAnswers,
    ) -> OperalaResult<ReadinessReport> {
        let bulk = answers
            .capability_answers
            .bulk_ingest
            .as_ref()
            .ok_or_else(|| "missing capability_answers.bulk_ingest".to_string())?;
        let mut found = BTreeMap::new();
        let mut missing = Vec::new();
        let mut warnings = Vec::new();

        for (collection, record) in &bulk.record_collections {
            if sorla.records.iter().any(|candidate| candidate == record) {
                found.insert(
                    format!("record_collection.{collection}"),
                    Value::String(record.clone()),
                );
            } else {
                missing.push(format!("record `{record}` for collection `{collection}`"));
            }
        }

        for (operation, action) in &bulk.actions {
            if sorla.actions.iter().any(|candidate| candidate == action) {
                found.insert(format!("action.{operation}"), Value::String(action.clone()));
            } else {
                missing.push(format!("action `{action}` for operation `{operation}`"));
            }
        }

        if bulk.actions.is_empty() {
            warnings.push(
                "no SoRLa actions were bound; runner must use generic record create/update semantics"
                    .to_string(),
            );
        }
        if !bulk.validation.atomic {
            warnings.push("bulk ingest is not configured as atomic".to_string());
        }

        let status = if missing.is_empty() {
            ReadinessStatus::Ready
        } else {
            ReadinessStatus::NeedsSorlaChanges
        };
        let summary = match status {
            ReadinessStatus::Ready => {
                "Bulk ingest handoff can be generated for the referenced SoRLa contract".to_string()
            }
            ReadinessStatus::NeedsSorlaChanges => {
                "Bulk ingest needs missing SoRLa records or actions before it is ready".to_string()
            }
            ReadinessStatus::UnsafeOrAmbiguous => {
                "Bulk ingest has ambiguous SoRLa bindings".to_string()
            }
        };

        Ok(ReadinessReport {
            schema: READINESS_SCHEMA.to_string(),
            capability: "bulk_ingest".to_string(),
            status,
            found,
            missing,
            warnings,
            summary,
        })
    }

    fn build_handoff(
        &self,
        sorla: &SorlaContract,
        answers: &OperalaAnswers,
        readiness: &ReadinessReport,
    ) -> OperalaResult<OperaLaHandoff> {
        let bulk = answers
            .capability_answers
            .bulk_ingest
            .as_ref()
            .ok_or_else(|| "missing capability_answers.bulk_ingest".to_string())?;
        let mut schemas = BTreeMap::new();
        schemas.insert("bulk-upload".to_string(), bulk_upload_schema(bulk));

        Ok(OperaLaHandoff {
            schema: HANDOFF_SCHEMA.to_string(),
            capability: "bulk_ingest".to_string(),
            extension: self.id().to_string(),
            extension_version: self.version().to_string(),
            tenant_required: true,
            team_optional: true,
            sorla: HandoffSorla {
                source: sorla.source.clone(),
                source_digest: sorla.source_digest.clone(),
                parser: "greentic-sorla-lib".to_string(),
                required_schema: "greentic.sorla.v0.2".to_string(),
                package_name: sorla.package_name.clone(),
                package_version: sorla.package_version.clone(),
            },
            sorx: SorxBindingTemplate {
                transport: "http".to_string(),
                url: "runtime-provided".to_string(),
            },
            bindings: json!({
                "name": bulk.name,
                "record_collections": bulk.record_collections,
                "actions": bulk.actions,
                "expected_counts": bulk.expected_counts,
                "validation": bulk.validation,
                "source_digest": sorla.source_digest,
            }),
            input_modes: if bulk.input_modes.is_empty() {
                vec!["batch".to_string()]
            } else {
                bulk.input_modes.clone()
            },
            schemas,
            flows: vec!["bulk-upload.flow.yaml".to_string()],
            ui: vec!["bulk-upload-summary.card.json".to_string()],
            tests: vec!["bulk-upload.sample.json".to_string()],
            readiness: readiness.clone(),
        })
    }
}

pub fn infer_answers(sorla: &SorlaContract, prompt: &str) -> BulkIngestAnswers {
    let record_collections = sorla
        .records
        .iter()
        .map(|record| (collection_name(record), record.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut actions = sorla
        .records
        .iter()
        .filter_map(|record| {
            find_create_action_for_record(&sorla.actions, record)
                .map(|action| (format!("create_{}", collection_name(record)), action))
        })
        .collect::<BTreeMap<_, _>>();
    for action in sorla.actions.iter().filter(|action| {
        prompt
            .to_ascii_lowercase()
            .contains(&action.to_ascii_lowercase())
    }) {
        actions
            .entry(to_snake_case(action))
            .or_insert_with(|| action.clone());
    }
    let expected_counts = sorla
        .records
        .iter()
        .filter_map(|record| {
            infer_expected_count(prompt, record).map(|count| (record.clone(), count))
        })
        .collect::<BTreeMap<_, _>>();

    BulkIngestAnswers {
        name: infer_bulk_ingest_name(prompt),
        input_modes: vec!["batch".to_string()],
        record_collections,
        actions,
        expected_counts,
        validation: BulkIngestValidation {
            atomic: true,
            dry_run: true,
            require_unique_ids: true,
            validate_references: true,
        },
    }
}

fn infer_bulk_ingest_name(prompt: &str) -> String {
    let lower = prompt.to_ascii_lowercase();
    if lower.contains("landlord") && lower.contains("tenant") {
        "landlord_tenant_bulk_upload".to_string()
    } else {
        "generic_bulk_ingest".to_string()
    }
}

fn collection_name(record: &str) -> String {
    to_snake_case(record)
}

fn find_create_action_for_record(actions: &[String], record: &str) -> Option<String> {
    let record_lower = record.to_ascii_lowercase();
    actions
        .iter()
        .find(|action| {
            let action_lower = action.to_ascii_lowercase();
            action_lower.contains(&record_lower)
                && (action_lower.starts_with("create")
                    || action_lower.starts_with("add")
                    || action_lower.starts_with("record")
                    || action_lower.starts_with("assign"))
        })
        .cloned()
}

fn infer_expected_count(prompt: &str, record: &str) -> Option<u64> {
    let labels = count_label_variants(record);
    let words = prompt
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .map(|word| word.to_ascii_lowercase())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let mut count = None;
    for index in 0..words.len() {
        let Ok(parsed) = words[index].parse::<u64>() else {
            continue;
        };
        if labels.iter().any(|label| {
            words_match(&words, index + 1, label) || words_match(&words, index + 2, label)
        }) {
            count = Some(count.map_or(parsed, |current: u64| current.max(parsed)));
        }
    }
    count
}

fn count_label_variants(record: &str) -> Vec<Vec<String>> {
    let compact = record.to_ascii_lowercase();
    let snake = to_snake_case(record);
    let words = snake
        .split('_')
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let plural_words = if let Some((last, prefix)) = words.split_last() {
        let mut values = prefix.to_vec();
        values.push(pluralize_lower(last));
        values
    } else {
        Vec::new()
    };
    vec![
        vec![compact.clone()],
        vec![pluralize_lower(&compact)],
        words,
        plural_words,
    ]
}

fn words_match(words: &[String], start: usize, label: &[String]) -> bool {
    start + label.len() <= words.len() && words[start..start + label.len()] == *label
}

fn pluralize_lower(value: &str) -> String {
    if let Some(stem) = value.strip_suffix('y') {
        format!("{stem}ies")
    } else {
        format!("{value}s")
    }
}

fn bulk_upload_schema(bulk: &BulkIngestAnswers) -> Value {
    let properties = bulk
        .record_collections
        .keys()
        .map(|collection| {
            (
                collection.clone(),
                json!({
                    "type": "array",
                    "items": {"type": "object"}
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let required = bulk.record_collections.keys().cloned().collect::<Vec<_>>();

    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://greentic.ai/schemas/operala.bulk-ingest.batch.v1.json",
        "type": "object",
        "required": ["batch_id", "records"],
        "properties": {
            "batch_id": {"type": "string"},
            "dry_run": {"type": "boolean"},
            "records": {
                "type": "object",
                "required": required,
                "properties": properties,
                "additionalProperties": false
            }
        }
    })
}
