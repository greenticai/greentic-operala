# PR 08 — Built-in reconciliation extension scaffold

## Goal

Add reconciliation as the first built-in OperaLa extension.

## Location

```text
greentic-operala/
  extensions/
    reconciliation/
      crates/
        operala-reconciliation/
      schemas/
      templates/
      examples/
```

## Extension ID

```text
greentic.operala.reconciliation.v1
```

## Responsibilities

The extension knows how to:

- inspect SoRLa v0.2 records, events, actions, agent endpoints, and provider
  requirements for reconciliation readiness
- ask reconciliation-specific QA questions
- propose minimal `greentic.sorla.patch.v1` patches
- generate reconciliation operational handoff metadata and optional pilot
  `.gtpack` compatibility entries
- include flow templates for single transaction and daily batch transaction input
- include exception UI templates

## Non-goals

- No separate `reconcila` CLI.
- No separate `reconcilax`.
- No runtime LLM dependency.
- No local SoRLa parser, legacy `entities`, or legacy `functions` model.
- No direct provider credentials or concrete SORX URL in generated artifacts.

## Acceptance criteria

- Extension compiles as part of `greentic-operala`.
- `greentic-operala wizard --schema` includes reconciliation questions when extension is selected in answers.
- `greentic-operala wizard --answers` can invoke the extension builder.
- Generated artifacts reference locked SoRLa action/agent endpoint IDs and
  source digests.
