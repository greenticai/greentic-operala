# Repository Overview

## Purpose

`greentic-operala` is the OperaLa authoring workspace. OperaLa is intended to
describe operational business logic that consumes SoRLa system-of-record
contracts and emits deterministic operational handoff metadata for Greentic
tooling.

The repository now contains the PR 00 architecture baseline plus a runnable
pilot slice for later OperaLa/OperaX work. Later PR notes remain active until
their full acceptance criteria are implemented.

## Current Code

- `Cargo.toml`: single publishable Rust package, `greentic-operala`, with the
  `greentic-operala` authoring binary.
- `src/lib.rs`: reusable implementation for answers, local artifact
  resolution, SoRLa contract inspection, QA schema emission, reconciliation
  readiness, and operational handoff generation.
- `src/main.rs`: `greentic-operala` CLI entrypoint.
- `docs/architecture/operala-operax.md`: architecture decision for SoRLa,
  OperaLa, OperaX, SORX, and `gtc` ownership.
- `extensions/reconciliation/`: stable home for the built-in reconciliation
  extension and future templates/schemas/examples.
- `examples/tenancy/`: runnable tenancy reconciliation demo fixture.
- `ci/local_check.sh`: canonical local quality gate.
- `scripts/validate_codex_material.sh`: validates active `.codex` JSON schemas,
  examples, and legacy SoRLa-shape regressions.
- `scripts/e2e/customer-pilot-demo-smoke.sh`: smoke-validates the customer pilot
  planning fixture and, when `../greentic-sorla` is present, validates the
  example SoRLa YAML through the real SoRLa CLI.
- `tools/i18n.sh`: i18n lifecycle hook matching the SoRLa repo shape.
- `.github/workflows/`: CI, customer pilot demo smoke, nightly coverage,
  dependency review, CodeQL, and Dependabot configuration.

## PR Material

- `.codex/CODEX_MASTER_PROMPT.md`: implementation guardrails.
- `.codex/done/00-architecture-overview.md`: implemented architecture baseline.
- `.codex/done/01-operala-cli-minimal-surface.md`: implemented minimal CLI
  surface with localized help and local distributor-cache answers refs.
- `.codex/done/02-operala-answers-contract.md`: implemented answers structs,
  schema, validation, and tests for required fields.
- `.codex/done/03-operala-distributor-client-resolution.md`: implemented
  artifact resolver API, local distributor-cache resolution, digest validation,
  and build-lock digest metadata.
- `.codex/done/04-operala-qa-lib-state-machine.md`: implemented
  `greentic-qa-lib`-backed schema marker, deterministic wizard state file,
  resume detection, stage tracking, and unresolved-question summaries.
- `.codex/done/05-operala-i18n-locale.md`: implemented sidecar i18n catalogs,
  locale fallback, localized help/schema labels, RTL metadata, and i18n
  validation.
- `.codex/done/06-operala-extension-api.md`: implemented extension trait,
  built-in registry, reconciliation registration, and localized unknown
  extension errors.
- `.codex/done/07-operala-prompt-authoring.md`: implemented prompt inference
  for reconciliation language, follow-up-required errors, valid nested answers,
  and stable SoRLa action/endpoint id selection.
- `.codex/done/08-reconciliation-extension-scaffold.md`: implemented the
  built-in reconciliation extension scaffold, wizard schema registration,
  builder invocation tests, and explicit locked SoRLa bindings in generated
  handoff artifacts.
- `.codex/done/09-reconciliation-readiness-analyser.md`: implemented
  reconciliation readiness reporting for ready, missing-SoRLa, and ambiguous
  states, localized human summaries, stable machine fields, and non-blocking
  endpoint warnings.
- `.codex/done/10-reconciliation-sorla-patch-templates.md`: implemented
  additive SoRLa semantic patch proposals for missing reconciliation records,
  proposal-not-applied build summaries, internal patch validation, and a
  sibling SoRLa `design patch --dry-run` compatibility check.
- `.codex/done/11-reconciliation-answers-schema.md`: implemented the
  reconciliation answers contract with required mapping fields, currency,
  single/batch input modes, bounded percentage thresholds, and threshold-order
  validation.
- `.codex/done/12-reconciliation-gtpack-builder.md`: implemented deterministic
  reconciliation handoff output, build lock metadata, single and batch assets,
  and a `greentic-pack` `.gtpack` artifact that the installed
  `greentic-operax` binary can run.
- `.codex/done/20-customer-pilot-tenancy-demo.md`: implemented the tenancy
  reconciliation demo, mock SORX server, dry-run and mock-write OperaX flows,
  call/state assertions, and demo README commands.
- `.codex/done/21-ci-customer-pilot-quality-gates.md`: implemented CI/local
  quality gates for fmt, clippy, tests, schemas, golden demo assertions,
  dry-run and mock-write smoke, i18n, handoff/build-lock validation, generated
  artifact safety, coverage, and failure artifact upload.
- `schemas/`: runtime schemas for OperaLa answers.
- `examples/tenancy/`: tenancy reconciliation pilot inputs, including
  `answers.json`.
- `.codex/audit-operala-generated-material.md`: audit record of changes made
  after comparing against `../greentic-sorla`.
- `.codex/greentic-sorla-summary.md`: local summary of the sibling SoRLa repo.

## Boundaries

- SoRLa owns system-of-record source contracts, canonical IR, SoRLa semantic
  patches, and SORX handoff metadata.
- OperaLa should consume SoRLa through `greentic-sorla-lib` or stable SoRLa
  artifacts, not a local parser or duplicated model.
- OperaLa owns operational answers, readiness analysis, operational handoff
  metadata, and built-in operational extensions such as reconciliation.
- OperaX is the installed `greentic-operax` local/pilot runner over SORX HTTP.
- `gtc` owns production extension orchestration and final assembly.

## Required Checks

Run:

```bash
bash ci/local_check.sh
```

Current checks include Rust fmt/clippy/test/build/doc, `.codex` schema and
fixture validation, customer pilot fixture smoke, cross-repo SoRLa validation
when `../greentic-sorla` is available, and optional `greentic-dev coverage`
when installed.
