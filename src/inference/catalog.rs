//! Serializes the SoRLa contract surface the LLM is allowed to bind to.

use crate::SorlaContract;
use serde_json::Value;

/// The complete identifier surface the LLM may bind to. Anything outside this
/// catalog is rejected by the validation gate.
pub fn sorla_catalog(sorla: &SorlaContract) -> Value {
    serde_json::json!({
        "package": {
            "name": sorla.package_name,
            "version": sorla.package_version,
        },
        "records": sorla.records,
        "events": sorla.events,
        "actions": sorla.actions,
        "agent_endpoints": sorla.agent_endpoints,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SourceKind, SourceRef, load_sorla_contract};

    #[test]
    fn catalog_lists_all_bindable_identifiers() {
        let sorla = load_sorla_contract(&SourceRef {
            kind: SourceKind::File,
            uri: "extensions/reconciliation/examples/tenancy/sorla.yaml".into(),
            digest: None,
        })
        .expect("fixture sorla loads");
        let catalog = sorla_catalog(&sorla);
        let records = catalog["records"].as_array().expect("records array");
        assert!(records.iter().any(|r| r == "RentObligation"));
        let events = catalog["events"].as_array().expect("events array");
        assert!(events.iter().any(|e| e == "BankTransaction"));
        assert!(catalog["actions"].as_array().is_some());
        assert!(catalog["agent_endpoints"].as_array().is_some());
        assert_eq!(catalog["package"]["name"], sorla.package_name.as_str());
    }
}
