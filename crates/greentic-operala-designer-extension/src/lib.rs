//! Designer extension adapter for Greentic OperaLa.
//!
//! Mirrors the greentic-sorla designer-extension layout: a thin wasm component
//! shell (`component.rs`) over a pure-Rust JSON-boundary tool surface
//! (`tools`). The wasm shell also provides the [`component::HostLlm`] adapter
//! that backs OperaLa's inference pipeline with the designer host's per-tenant
//! LLM import.

// Generated WIT bindings + world export glue are wasm-only. The native rlib
// build (used by the unit tests) keeps the pure JSON-boundary API and never
// pulls in `wit-bindgen`'s wasm runtime.
#[cfg(target_arch = "wasm32")]
#[allow(warnings)]
mod bindings;
#[cfg(target_arch = "wasm32")]
mod component;

/// Design-time tool surface. Task 5 fills in the real OperaLa tools; until
/// then the registry is empty and invocation reports not-yet-implemented so
/// the component compiles and loads whole.
pub mod tools {
    /// Native-side tool definition mirrored into the WIT `tool-definition`
    /// record by the wasm component shell.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct DesignerTool {
        pub name: &'static str,
        pub description: &'static str,
        pub input_schema_json: String,
        pub output_schema_json: Option<String>,
    }

    /// Tools advertised to the designer host. Empty until Task 5 lands the
    /// OperaLa authoring tools.
    pub fn list_tools() -> Vec<DesignerTool> {
        Vec::new()
    }

    /// Dispatch a tool invocation by name. Every name errors until Task 5
    /// wires the OperaLa tool implementations in.
    pub fn invoke_tool(name: &str, _args_json: &str) -> Result<String, String> {
        Err(format!(
            "OperaLa designer tool `{name}` is not yet implemented"
        ))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn list_tools_is_empty_until_tools_land() {
            assert!(list_tools().is_empty());
        }

        #[test]
        fn invoke_tool_reports_not_yet_implemented() {
            let error = invoke_tool("generate_operala_answers", "{}").unwrap_err();
            assert!(error.contains("generate_operala_answers"));
            assert!(error.contains("not yet implemented"));
        }
    }
}
