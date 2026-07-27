//! Deterministic gate over LLM output: serde-deserializes the answers value
//! and checks every binding against the SoRLa catalog. The LLM proposes;
//! this module disposes.

use crate::{
    BulkIngestAnswers, BusinessEventsAnswers, EXTENSION_BULK_INGEST, EXTENSION_BUSINESS_EVENTS,
    EXTENSION_RECONCILIATION, ReconciliationAnswers, SorlaContract,
};
use serde_json::Value;
use std::collections::BTreeSet;

fn check_membership(
    errors: &mut Vec<String>,
    path: &str,
    value: &str,
    catalog: &[String],
    kind: &str,
) {
    if !catalog.iter().any(|item| item == value) {
        errors.push(format!(
            "{path}: '{value}' is not a SoRLa {kind} (catalog: {})",
            catalog.join(", ")
        ));
    }
}

/// Validate an LLM-produced capability answers value for `extension_id`
/// against the SoRLa catalog. Returns ALL problems so the retry loop can feed
/// the complete list back to the model.
pub fn validate_capability_answers(
    extension_id: &str,
    value: &Value,
    sorla: &SorlaContract,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    match extension_id {
        EXTENSION_RECONCILIATION => {
            let answers: ReconciliationAnswers = match serde_json::from_value(value.clone()) {
                Ok(answers) => answers,
                Err(err) => return Err(vec![format!("answers shape invalid: {err}")]),
            };
            check_membership(
                &mut errors,
                "source_event",
                &answers.source_event,
                &sorla.events,
                "event",
            );
            check_membership(
                &mut errors,
                "expected_record",
                &answers.expected_record,
                &sorla.records,
                "record",
            );
            check_membership(
                &mut errors,
                "settlement_record",
                &answers.settlement_record,
                &sorla.records,
                "record",
            );
            check_membership(
                &mut errors,
                "exception_record",
                &answers.exception_record,
                &sorla.records,
                "record",
            );
            for (operation, action) in &answers.actions {
                check_membership(
                    &mut errors,
                    &format!("actions.{operation}"),
                    action,
                    &sorla.actions,
                    "action",
                );
            }
            for (operation, endpoint) in &answers.agent_endpoints {
                check_membership(
                    &mut errors,
                    &format!("agent_endpoints.{operation}"),
                    endpoint,
                    &sorla.agent_endpoints,
                    "agent endpoint",
                );
            }
        }
        EXTENSION_BULK_INGEST => {
            let answers: BulkIngestAnswers = match serde_json::from_value(value.clone()) {
                Ok(answers) => answers,
                Err(err) => return Err(vec![format!("answers shape invalid: {err}")]),
            };
            for (collection, record) in &answers.record_collections {
                check_membership(
                    &mut errors,
                    &format!("record_collections.{collection}"),
                    record,
                    &sorla.records,
                    "record",
                );
            }
            for (operation, action) in &answers.actions {
                check_membership(
                    &mut errors,
                    &format!("actions.{operation}"),
                    action,
                    &sorla.actions,
                    "action",
                );
            }
        }
        EXTENSION_BUSINESS_EVENTS => {
            let answers: BusinessEventsAnswers = match serde_json::from_value(value.clone()) {
                Ok(answers) => answers,
                Err(err) => return Err(vec![format!("answers shape invalid: {err}")]),
            };

            // Declared events, keyed on the dotted `domain.name` form so
            // trigger `emits` bindings can be checked against them below.
            let declared: BTreeSet<String> = answers
                .events
                .iter()
                .map(|event| format!("{}.{}", event.domain, event.name))
                .collect();
            for event in &answers.events {
                if event.domain.trim().is_empty() || event.name.trim().is_empty() {
                    errors.push(format!(
                        "event '{}.{}' must have non-empty domain and name",
                        event.domain, event.name
                    ));
                }
                if event.schema_version.trim().is_empty() {
                    errors.push(format!(
                        "event '{}.{}' missing schema_version",
                        event.domain, event.name
                    ));
                }
            }

            let mut seen_trigger_ids = BTreeSet::new();
            for trigger in &answers.triggers {
                if !seen_trigger_ids.insert(trigger.id.clone()) {
                    errors.push(format!("duplicate trigger id '{}'", trigger.id));
                }
                // Range checks via the vendored schedule validator.
                errors.extend(crate::business_events::schedule::validate_schedule(
                    &trigger.schedule,
                ));
                // `emits` must resolve to a declared event.
                let emits_declared = crate::resolve_business_event_ref(&trigger.emits)
                    .is_some_and(|dotted| declared.contains(&dotted));
                if !emits_declared {
                    errors.push(format!(
                        "trigger '{}' emits '{}' which is not a declared event",
                        trigger.id, trigger.emits
                    ));
                }
            }
        }
        other => errors.push(format!("unknown extension '{other}'")),
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SourceKind, SourceRef, load_sorla_contract};

    fn fixture_sorla() -> SorlaContract {
        load_sorla_contract(&SourceRef {
            kind: SourceKind::File,
            uri: "extensions/reconciliation/examples/tenancy/sorla.yaml".into(),
            digest: None,
        })
        .expect("fixture sorla loads")
    }

    fn valid_reconciliation_value() -> Value {
        let answers: crate::OperalaAnswers = serde_json::from_str(include_str!(
            "../../extensions/reconciliation/examples/tenancy/answers.json"
        ))
        .expect("fixture answers parse");
        serde_json::to_value(answers.capability_answers.reconciliation.expect("nested"))
            .expect("serializes")
    }

    #[test]
    fn valid_reconciliation_bindings_pass() {
        let sorla = fixture_sorla();
        validate_capability_answers(
            EXTENSION_RECONCILIATION,
            &valid_reconciliation_value(),
            &sorla,
        )
        .expect("fixture bindings validate");
    }

    #[test]
    fn hallucinated_event_is_rejected_with_path() {
        let sorla = fixture_sorla();
        let mut value = valid_reconciliation_value();
        value["source_event"] = Value::String("ImaginaryEvent".into());
        let errors =
            validate_capability_answers(EXTENSION_RECONCILIATION, &value, &sorla).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.contains("source_event") && e.contains("ImaginaryEvent")),
            "got: {errors:?}"
        );
    }

    #[test]
    fn hallucinated_action_is_rejected() {
        let sorla = fixture_sorla();
        let mut value = valid_reconciliation_value();
        value["actions"]["create_settlement"] = Value::String("not_an_action".into());
        let errors =
            validate_capability_answers(EXTENSION_RECONCILIATION, &value, &sorla).unwrap_err();
        assert!(
            errors.iter().any(|e| e.contains("not_an_action")),
            "got: {errors:?}"
        );
    }

    #[test]
    fn shape_mismatch_is_rejected_as_serde_error() {
        let sorla = fixture_sorla();
        let errors = validate_capability_answers(
            EXTENSION_RECONCILIATION,
            &serde_json::json!({"name": "missing everything"}),
            &sorla,
        )
        .unwrap_err();
        assert!(!errors.is_empty());
    }

    #[test]
    fn unknown_extension_is_rejected() {
        let sorla = fixture_sorla();
        let errors = validate_capability_answers("greentic.operala.nope.v1", &Value::Null, &sorla)
            .unwrap_err();
        assert!(errors[0].contains("unknown extension"), "got: {errors:?}");
    }

    #[test]
    fn business_events_reports_duplicate_id_bad_minute_and_dangling_emits() {
        let sorla = fixture_sorla();
        let value = serde_json::json!({
            "name": "invoicing",
            "events": [
                { "domain": "invoice", "name": "created", "schema_version": "1" }
            ],
            "triggers": [
                {
                    "id": "",
                    "schedule": { "kind": "hourly", "minute": 90 },
                    "emits": "invoice.created"
                },
                {
                    "id": "",
                    "schedule": { "kind": "every_minute" },
                    "emits": "invoice.nonexistent"
                }
            ]
        });
        let errors =
            validate_capability_answers(EXTENSION_BUSINESS_EVENTS, &value, &sorla).unwrap_err();
        let joined = errors.join(" | ");
        assert!(joined.contains("id"), "got: {errors:?}");
        assert!(joined.contains("minute"), "got: {errors:?}");
        assert!(joined.contains("emits"), "got: {errors:?}");
    }
}
