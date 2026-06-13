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

/// Design-time tool surface: five OperaLa authoring tools with a stateless
/// JSON-envelope contract matching the sorla extension convention.
pub mod tools;
