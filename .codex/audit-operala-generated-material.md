# Audit: OperaLa Generated Material

This audit adapts the generated OperaLa PR notes, schemas, and tenancy example
against the observed `../greentic-sorla` architecture.

## Main Corrections

- OperaLa is now framed as operational business-logic authoring that consumes
  SoRLa system-of-record contracts, not as a second system-of-record layer.
- SoRLa source handling is delegated to `greentic-sorla-lib` or stable SoRLa
  artifacts. OperaLa should not define a local SoRLa parser, AST, patch engine,
  or legacy `entities`/`functions` model.
- The primary OperaLa output is deterministic operational handoff metadata.
  Pilot `.gtpack` compatibility output is allowed for local OperaX/demo runs,
  but `gtc` remains the owner of production extension orchestration and final
  assembly.
- Prompt authoring remains answers-only. It must not build packs, call SORX,
  apply SoRLa patches, or mutate production resources.
- Reconciliation readiness now binds to SoRLa v0.2 `records`, `events`,
  `actions`, and optional `agent_endpoints`, with locked IDs and source digests
  as the machine contract.
- Patch proposals now refer to `greentic.sorla.patch.v1` and explicitly avoid
  inventing an OperaLa-specific SoRLa mutation format.
- Runtime execution is kept in OperaX and talks to SORX over HTTP. Generated
  artifacts must not contain plaintext secrets, provider credentials, or
  concrete customer SORX URLs.

## Files Updated

- `.codex/CODEX_MASTER_PROMPT.md`
- `.codex/done/00-architecture-overview.md`
- `.codex/done/01-operala-cli-minimal-surface.md`
- `.codex/02-operala-answers-contract.md`
- `.codex/03-operala-distributor-client-resolution.md`
- `.codex/04-operala-qa-lib-state-machine.md`
- `.codex/05-operala-i18n-locale.md`
- `.codex/06-operala-extension-api.md`
- `.codex/07-operala-prompt-authoring.md`
- `.codex/08-reconciliation-extension-scaffold.md`
- `.codex/09-reconciliation-readiness-analyser.md`
- `.codex/10-reconciliation-sorla-patch-templates.md`
- `.codex/11-reconciliation-answers-schema.md`
- `.codex/12-reconciliation-gtpack-builder.md`
- `.codex/20-customer-pilot-tenancy-demo.md`
- `.codex/21-ci-customer-pilot-quality-gates.md`
- `schemas/operala.answers.schema.json`
- `schemas/operax.decision.schema.json`
- `examples/tenancy/sorla.yaml`
- `examples/tenancy/answers.json`

## Validation Performed

- JSON syntax validation with `jq` for both schemas and the example
  `answers.json`.
- SoRLa validation of `examples/tenancy/sorla.yaml` using:

```bash
cargo run -p greentic-sorla -- design validate ../greentic-operala/examples/tenancy/sorla.yaml
```

Result: `Validation: OK`.

## Remaining Implementation Notes

- The JSON schemas are planning schemas. Once Rust structs exist, generate or
  round-trip schemas from the actual types to prevent drift.
- `auto_match_threshold >= review_threshold` is documented but not expressible
  in the current simple JSON Schema without a custom validator.
- The illustrative SoRLa patch example should be reconciled with the exact
  semantic patch operation names exposed by `greentic-sorla-lib` before
  implementation.
