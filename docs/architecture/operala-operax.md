# OperaLa and OperaX Architecture

OperaLa is the authoring layer for operational business logic. It consumes
SoRLa system-of-record contracts and emits deterministic operational handoff
metadata for Greentic tooling.

OperaX is a local and pilot runner for OperaLa handoff artifacts. It loads a
handoff directory or a real `greentic-pack` `.gtpack` containing OperaLa assets,
accepts tenant/team runtime context, and calls SORX over HTTP. It does not run
SORX in process, mutate `sorla.yaml`, resolve provider credentials, or own
production deployment.

Production extension orchestration remains owned by `gtc`. Local `.gtpack`
output from this repository is built with `greentic-pack` and is intended for
OperaX pilot/demo execution, not a transfer of production orchestration
ownership to OperaLa.

## Ownership

- SoRLa owns system-of-record source contracts, canonical IR, semantic patches,
  agent endpoint contracts, and SORX handoff metadata.
- OperaLa owns operational answers, readiness analysis, operational handoff
  metadata, and built-in operational extensions such as reconciliation.
- OperaX owns local/pilot execution over SORX HTTP.
- SORX owns system-of-record runtime behavior.
- `gtc` owns production extension orchestration and final assembly.

## First Built-In Extension

The first built-in OperaLa extension is reconciliation. It lives under
`extensions/reconciliation` and binds banking transaction inputs to SoRLa
records, actions, and agent endpoints for payment reconciliation workflows.
