# Codex master prompt

# Codex master prompt

Implemented PR notes are moved to `.codex/done/`; active PR notes remain at the
top level of `.codex/`. This repo is the OperaLa
authoring workspace: operational business logic that binds to SoRLa
system-of-record contracts and emits deterministic handoff artifacts for
downstream Greentic tooling.

Important architecture constraints:

- Reconciliation is a built-in OperaLa extension under `greentic-operala/extensions/reconciliation`.
- Do not create separate `reconcila` or `reconcilax` products.
- Use current SoRLa v0.2 source contracts: `package`, `records`, `events`,
  `actions`, `agent_endpoints`, `provider_requirements`, `migrations`, and
  canonical SoRLa IR. Do not invent legacy `entities` or `functions` schemas.
- OperaLa must consume SoRLa through `greentic-sorla-lib` or its stable exported
  artifacts instead of redefining SoRLa parser, AST, patch, pack, or model
  semantics.
- `greentic-operala prompt --sorla sorla.yaml "..."` outputs only `answers.json`.
- `greentic-operala wizard --answers answers.json` deterministically emits an OperaLa
  operational handoff artifact. Local `.gtpack` output must be built with
  `greentic-pack` for OperaX pilot/demo tooling, while final production
  orchestration remains owned by `gtc`.
- OperaLa may propose `greentic.sorla.patch.v1` patches, but prompt/wizard must
  not apply them directly. Application belongs to the SoRLa/Designer/gtc
  authoring flow after explicit approval.
- OperaX, if implemented here for pilot validation, is a local operational
  runner for OperaLa handoff artifacts. It calls SORX over HTTP and must not run
  SORX in process, mutate `sorla.yaml`, or resolve provider credentials itself.
- Use `greentic-qa-lib` for wizard schemas and answer state machine.
- Use `greentic-i18n-lib` for all user-facing strings and locale support.
- Use `greentic-distributor-client` to resolve `--answers`, SoRLa refs, and
  extension refs from file/OCI/store/repo.
- OperaX requires tenant and supports optional team.
- Support both single banking transaction JSON and daily batch JSON.
- Make the demo run against a mock SORX HTTP endpoint.
