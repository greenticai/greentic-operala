# greentic-sorla Summary

`../greentic-sorla` is the wizard-first home for the SoRLa authoring language,
canonical IR, reusable authoring library, Designer extension surface, and
deterministic handoff artifacts for the Greentic stack.

It deliberately does not own final runtime assembly. `gtc` owns production
extension orchestration, final pack and bundle assembly, setup handoff, start
handoff, and extension registry resolution. SORX, provider repos, and runtime
components own execution concerns such as HTTP/MCP serving, provider calls,
OAuth, secrets, databases, runtime policy, and deployment.

## Public Product Surface

- `greentic-sorla wizard --schema`
- `greentic-sorla wizard --answers <answers.json>`
- `greentic-sorla wizard --answers <answers.json> --pack-out <file.gtpack>`
- `greentic-sorla prompt --answers-out <answers.json> --llm-provider <provider>`
- `greentic-sorla pack <sorla.yaml> --name <name> --version <version> --out <file.gtpack>`
- `greentic-sorla pack doctor|inspect|validation-inspect <file.gtpack>`
- `greentic-sorla pack schema validation|exposure-policy|compatibility|ontology|retrieval-bindings`

Production composition should be thought of as `gtc wizard --extensions ...`;
the direct `greentic-sorla` CLI is primarily for local authoring, fixtures,
schema work, pack handoff checks, and extension development.

## Workspace Shape

- `crates/greentic-sorla-cli`: thin installed binary wrapper over the reusable
  library facade.
- `crates/greentic-sorla-lib`: main reusable facade for schema emission,
  answers application, prompt sessions, YAML parsing, validation, concept views,
  semantic patches, previews, pack bytes/entries, doctor, and inspect.
- `crates/greentic-sorla-lang`: SoRLa AST and YAML parser/validator.
- `crates/greentic-sorla-ir`: canonical deterministic IR lowering,
  serialization, inspection, and hashing.
- `crates/greentic-sorla-pack`: handoff artifact and `.gtpack` compatibility
  generation, including SORX validation, exposure, compatibility, ontology,
  retrieval, metrics, Designer node type, and agent endpoint catalog assets.
- `crates/greentic-sorla-wizard`: wizard schema model adapters.
- `crates/greentic-sorla-designer-extension`: JSON/WASM-friendly Designer SDK
  adapter exposing SoRLa parse, view, patch, prompt, validation, and gtpack
  artifact tools.
- `crates/greentic-sorla-e2e`: opt-in provider-backed landlord/tenant e2e
  harness, excluded from default workspace members.
- `xtask`: repo task runner for scenarios such as `cargo xtask e2e landlord-tenant --provider foundationdb`.

The repo is currently versioned as workspace package `0.1.16` and uses Rust
edition 2024.

## Language And Artifact Model

SoRLa v0.2 models records, roles, events, projections, views, actions, policies,
approvals, migrations, provider requirements, metrics, ontology declarations,
retrieval bindings, operational indexes, and agent endpoints.

Important semantics include:

- records can be `native`, `external`, or `hybrid`;
- hybrid records require field-level `authority: local|external`;
- external and hybrid records use `external_ref` for authoritative system/key
  linkage;
- fields support semantic scalar types and validation rules;
- events and projections are first-class, event-native declarations;
- migrations express compatibility intent, idempotence, backfills, projection
  evolution, and typed operations;
- provider requirements stay abstract and provider-agnostic;
- ontology, semantic aliases, entity linking, retrieval scopes, and operational
  indexes lower into deterministic handoff metadata;
- agent endpoints describe agent-facing business actions but are not runtime
  routes.

Generated `.gtpack` archives are deterministic handoff contracts. They include
canonical SoRLa model assets, executable contracts, agent gateway metadata,
generic Greentic stack-pack metadata, SORX start/runtime/provider templates,
validation/exposure/compatibility assets, and optional ontology, retrieval,
metrics, Designer node type, and agent endpoint catalog artifacts.

## Designer And Agent Endpoint Integration

Designer workflows treat `sorla.yaml` as source of truth:

```text
sorla.yaml -> ConceptViewModel -> Designer/CLI -> semantic patch -> sorla.yaml
```

The Designer extension exposes YAML-first tools such as:

- `parse_sorla_yaml`
- `generate_concept_view`
- `apply_sorla_patch`
- `propose_patch_from_instruction`
- `validate_sorla_yaml`
- `generate_gtpack_from_sorla_yaml`

Agent endpoints lower into canonical IR and can produce deterministic handoff
artifacts for downstream OpenAPI overlays, Arazzo workflows, MCP tool metadata,
`llms.txt` fragments, locked Designer node types, and a design-time action
catalog. Runtime binding should use locked endpoint references and contract
hashes, not free-text labels.

## Checks And Release Flow

The main local verification command is:

```bash
bash ci/local_check.sh
```

That script checks metadata, `cargo fmt`, `cargo clippy`, `cargo test`, pack
schema commands, validation-enabled gtpack generation, ontology handoff smoke,
`cargo build`, optional WASM facade build, `cargo doc`, packaging,
`cargo publish --dry-run`, and i18n JSON/translation checks.

Release automation tags version bumps on `main`, builds platform binaries,
validates packaging, and publishes crates after release artifacts succeed.

## Useful Orientation Docs

- `README.md`: current product surface and ownership boundaries.
- `docs/architecture.md`: canonical responsibility split between SoRLa, `gtc`,
  SORX, providers, and runtimes.
- `docs/product-shape.md`: wizard-first UX contract.
- `docs/spec/v0.2.md`: language semantics.
- `docs/library-api.md` and `docs/sorla-lib.md`: reusable facade boundaries.
- `docs/sorla-gtpack.md`: deterministic `.gtpack` handoff contract.
- `docs/agent-endpoints.md`: agent endpoint authoring and safety model.
- `docs/designer-sdk-extension.md`: Designer extension tool boundary.
- `.codex/repo_overview.md` inside `../greentic-sorla`: very detailed
  milestone and component inventory.

## Current Local Notes

- The sibling repo had one pre-existing dirty file when inspected:
  `.i18n/translator-state.json`.
- Latest local commit inspected: `17cac23 Expand SoRLa records, roles, i18n, and metrics`.
