# PR 04 — Greentic QA wizard state machine integration

## Goal

Make OperaLa wizard use `greentic-qa-lib` rather than ad-hoc questions.

## Behaviour

```bash
greentic-operala wizard --schema
```

emits a QA schema.

```bash
greentic-operala wizard --answers answers.json
```

runs a deterministic QA answers state machine.

## State machine stages

```text
load_answers
resolve_sorla
load_extension
validate_answers_schema
run_extension_readiness
maybe_generate_sorla_patch
build_operala_handoff
write_reports
```

## Required outputs

```text
target/operala/<capability-name>/
  readiness.report.yaml
  sorla.patch.json
  operala.build.lock
  operala.handoff.preview.json
  build.summary.md
```

If answers request a Greentic pack, the state machine may also write
`target/gtpacks/<name>.gtpack`, but the deterministic source of truth is the
OperaLa handoff metadata and build lock. Final production assembly is delegated
to `gtc`.

## Acceptance criteria

- Wizard schema is generated using `greentic-qa-lib`.
- Wizard execution is deterministic and resumable.
- Partial state can be saved and resumed.
- Build summary includes unresolved questions when answers are incomplete.
- State machine records SoRLa source digest, SoRLa parser/schema version,
  extension ID/version, answers digest, and output digest.
