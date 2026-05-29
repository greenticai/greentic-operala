# PR 06 — OperaLa extension API

## Goal

Define the extension API for built-in and future external operational capability
extensions. Extensions consume normalized SoRLa design/canonical information and
produce OperaLa handoff plans; they do not own SoRLa parsing or final runtime
assembly.

## Required trait

```rust
#[async_trait]
pub trait OperaLaExtension {
    fn id(&self) -> &'static str;
    fn capability(&self) -> &'static str;
    fn version(&self) -> &'static str;

    fn qa_schema(&self, ctx: &QaContext) -> Result<QaSchema>;

    async fn analyse_sorla(
        &self,
        ctx: &ExtensionContext,
        sorla: &SorlaContract,
        answers: &OperaLaAnswers
    ) -> Result<ReadinessReport>;

    async fn build_handoff(
        &self,
        ctx: &BuildContext,
        sorla: &SorlaContract,
        answers: &OperaLaAnswers,
        readiness: &ReadinessReport
    ) -> Result<OperaLaHandoffResult>;
}
```

`SorlaContract` should be a thin wrapper around outputs from
`greentic-sorla-lib`, such as parsed design model, canonical IR hash, agent
endpoint catalog, and source reference metadata. Do not duplicate SoRLa AST
types locally unless they are narrow view types derived from the library.

## Built-in extensions directory

```text
greentic-operala/extensions/
  reconciliation/
```

## Extension discovery

Pilot version:

- compile built-in extensions statically
- registry maps extension ID to implementation

Later:

- support OCI/store extension discovery

## Acceptance criteria

- Reconciliation extension implements this trait.
- Core OperaLa knows nothing about reconciliation-specific fields.
- Unknown extension ID gives a localized error.
- Extension outputs include deterministic handoff entries and optional pilot
  `.gtpack` entries, not direct provider credentials or runtime URLs.
