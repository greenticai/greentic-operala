# PR 21 — CI and pilot quality gates

## Goal

Add CI checks needed before first customer pilot.

## Required checks

- `cargo fmt`
- `cargo clippy --all-targets --all-features`
- unit tests
- schema validation tests
- golden fixture tests
- end-to-end demo dry-run
- end-to-end demo against mock SORX HTTP server
- i18n key coverage
- handoff validation
- optional Greentic pack validation
- SoRLa fixture validation through `greentic-sorla-lib`

## Required workflows

```text
.github/workflows/operala.yml
.github/workflows/operax.yml
.github/workflows/customer-pilot-demo.yml
```

## Acceptance criteria

- PR cannot merge if demo breaks.
- Golden files must be intentionally updated.
- CI uploads demo artifacts on failure.
- CI rejects generated artifacts containing plaintext secrets, concrete SORX
  customer URLs, or legacy SoRLa `entities`/`functions` examples.
