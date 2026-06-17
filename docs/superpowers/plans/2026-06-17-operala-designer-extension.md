# OperaLa Designer Extension Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the `greentic.operala` WASM design-extension so the greentic-designer's already-wired `/api/operala/*` routes become functional, by wrapping operala's existing inference engine and packaging logic — no new authoring semantics.

**Architecture:** A new `cdylib` crate (`greentic-operala-designer-extension`) compiles to `wasm32-wasip2`, exports the 5 tools the designer's `dispatch_operala` calls, and implements operala's `ChatFn` seam over the host `llm` WIT import (no-tools / JSON-in-content mode). The native CLI runtime (`RigBackend`/tokio) is `cfg`-excluded from the wasm build. Two shared crates get a small wasm-enabling change: `greentic-llm` (rig behind a feature) and `greentic-pack` (in-memory `entries()` + native-only machinery behind a feature).

**Tech Stack:** Rust 1.95 (1.94 for some crates), `wasm32-wasip2`, `cargo-component` + `wit-bindgen` 0.44, `greentic-llm` types, `greentic-pack` `PackBuilder`, `serde`/`serde_json`/`serde_yaml`, `base64`, `sha2`, `blake3`, `ciborium` (via greentic-types canonical cbor).

## Global Constraints

- English only in source/tests/comments/commits. `#![forbid(unsafe_code)]` where already present.
- No `unwrap()`/`panic!()`/`expect()` on production paths — operala uses `OperalaResult<T> = Result<T, String>`; keep that convention.
- No Claude attribution / Co-Authored-By trailers in any commit or PR body.
- Conventional Commits (`feat:`/`fix:`/`refactor:`/`docs:`/`chore:`).
- `Cargo.lock` committed; CI uses `--locked`. Run `bash ci/local_check.sh` before declaring a repo's work done.
- Extension id is exactly `greentic.operala`; LLM role exactly `operala_composer`.
- The 5 tool names are fixed by the designer: `list_operala_capabilities`, `generate_operala_answers`, `update_operala_answers`, `validate_operala_answers`, `generate_handoff_pack`.
- No invented product names in public artifacts — only `OperaLa` / `greentic.operala`.
- `gts_`-style Store token: NEVER write into any tracked file. The `GREENTIC_STORE_TOKEN` lives only as a GitHub Actions secret (user-provisioned). The token pasted in chat is unresolved — do not use it.
- Versions: `greentic-llm` → `1.0.1` (main line); extension + crate → `0.1.0-research`.
- wasm compatibility of a dependency is decided EMPIRICALLY (`cargo build --target wasm32-wasip2`), not by assumption — `wasm32-wasip2` has `std::fs`/clock unlike `wasm32-unknown-unknown`.

---

## Repo: greentic-llm (PR 1)

### Task 1: Feature-gate `rig-core` so the type layer builds on wasm

**Files:**
- Modify: `/home/bima-pangestu/Works/greentic/greentic-llm/Cargo.toml`
- Modify: `/home/bima-pangestu/Works/greentic/greentic-llm/src/lib.rs:13,21`
- Modify (maybe): `Cargo.toml` `version` → `1.0.1`

**Interfaces:**
- Produces: `greentic-llm` `1.0.1` with `default = ["rig"]`; building `--no-default-features` yields the type layer (`ChatRequest`, `ChatResponse`, `ChatMessage`, `MessageRole`, `ToolDef`, `ToolCall`, `FinishReason`, `LlmError`, `ProviderKind`, `Capabilities`, `LlmProvider`, `CredentialSource`, `EnvCredentialSource`) with NO `RigBackend`.

- [ ] **Step 1: Confirm the rig coupling is doc-comment-only**

Run: `cd /home/bima-pangestu/Works/greentic/greentic-llm && grep -rnE 'rig' src/provider.rs src/capabilities.rs src/mock.rs src/credentials`
Expected: only doc-comment (`//!`, `///`) hits in `provider.rs`; zero in the others. If any non-comment hit appears, STOP and reassess.

- [ ] **Step 2: Make `rig-core` optional + add `rig` feature**

Edit `Cargo.toml`:
```toml
rig-core = { version = "0.35", optional = true }
```
```toml
[features]
default = ["rig"]
clap = ["dep:clap"]
rig = ["dep:rig-core"]
test-mock = []
```
Also bump `version = "1.0.1"`.

- [ ] **Step 3: Gate the `rig_backend` module + re-export**

Edit `src/lib.rs`:
```rust
#[cfg(feature = "rig")]
pub mod rig_backend;
```
```rust
#[cfg(feature = "rig")]
pub use rig_backend::RigBackend;
```

- [ ] **Step 4: Native build + tests unchanged**

Run: `cargo build && cargo test`
Expected: PASS (default enables `rig`, identical to before).

- [ ] **Step 5: Prove the wasm types-only build**

Run: `rustup target add wasm32-wasip2; cargo build --no-default-features --target wasm32-wasip2`
Expected: PASS. If `chrono`'s `clock` feature (pulled via `features=["clock","serde"]`) fails, change the dep to `chrono = { version = "0.4", default-features = false, features = ["serde"] }` and, only if a `now()` call exists (grep `now()` in `src/`), re-add `clock` gated behind `rig`. Re-run until PASS.

- [ ] **Step 6: local_check + commit**

Run: `bash ci/local_check.sh` (if present) — else `cargo fmt --all && cargo clippy --all-targets -- -D warnings`
```bash
git checkout -b feat/wasm-feature-gate-rig
git add Cargo.toml Cargo.lock src/lib.rs
git commit -m "feat: gate rig-core behind default rig feature for wasm builds"
```

- [ ] **Step 7: Publish 1.0.1 (after PR merge)**

Trigger the crates.io publish workflow (`workflow_dispatch`; org `CARGO_REGISTRY_TOKEN`). During dev, downstream repos may path-patch `greentic-llm` instead of waiting for publish.

---

## Repo: greentic-pack (PR 2)

### Task 2: Extract `PackBuilder::entries()` (in-memory file set), native build unchanged

**Files:**
- Modify: `/home/bima-pangestu/Works/greentic/greentic-pack/crates/greentic-pack/src/builder.rs:404-643` (`build`) and add `entries`
- Test: `crates/greentic-pack/src/builder.rs` (`#[cfg(test)]` module) or `tests/`

**Interfaces:**
- Produces: `impl PackBuilder { pub fn entries(&self) -> Result<std::collections::BTreeMap<String, Vec<u8>>> }` returning every in-archive file (flows yaml+json, assets, `manifest.cbor`, `manifest.json`, `provenance.json`, `sbom.json`, and signature files when signing ≠ None), deterministically keyed by archive path.
- Consumes (native `build`): now calls `entries()` then zips.

- [ ] **Step 1: Write a failing test for `entries()`**

Add to the builder test module:
```rust
#[test]
fn entries_contains_manifest_and_assets_without_fs() {
    let meta = sample_meta(); // reuse existing test helper or construct a minimal PackMeta
    let builder = PackBuilder::new(meta)
        .with_signing(Signing::None)
        .with_asset_bytes("assets/x.json", b"{}".to_vec());
    let entries = builder.entries().expect("entries");
    assert!(entries.contains_key("manifest.cbor"));
    assert!(entries.contains_key("manifest.json"));
    assert!(entries.contains_key("assets/x.json"));
    // deterministic ordering
    let keys: Vec<_> = entries.keys().cloned().collect();
    let mut sorted = keys.clone(); sorted.sort();
    assert_eq!(keys, sorted);
}
```

- [ ] **Step 2: Run it — fails (no `entries` method)**

Run: `cargo test -p greentic-pack-lib entries_contains_manifest -- --nocapture`
Expected: FAIL (method not found).

- [ ] **Step 3: Implement `entries()` by extracting build's assembly**

Move the in-memory assembly (current `build()` lines ~405–628: validation, flow processing, component processing, asset processing, manifest cbor+json, provenance.json, sbom.json, optional signature files, final sort) into:
```rust
pub fn entries(&self) -> Result<std::collections::BTreeMap<String, Vec<u8>>> {
    // ... existing assembly, but borrow &self (clone what build() consumed) ...
    // collect each PendingFile into BTreeMap<path, bytes> (BTreeMap gives the sort for free)
    Ok(map)
}
```
Then refactor `build(self, out_path)` to:
```rust
pub fn build(self, out_path: impl AsRef<Path>) -> Result<BuildResult> {
    let entries = self.entries()?;
    let out_path = out_path.as_ref();
    if let Some(parent) = out_path.parent() { fs::create_dir_all(parent)?; }
    let pending: Vec<PendingFile> = entries.iter()
        .map(|(p, b)| PendingFile { path: p.clone(), media_type: media_type_for(p), bytes: b.clone() })
        .collect();
    write_zip(out_path, &pending)?;
    // derive manifest_hash / BuildResult fields from `entries` as today
}
```
Note: `build()` currently consumes `self` (moves flows/assets). `entries(&self)` must borrow — adjust any `std::mem::take`/move to clone, or have `build()` call `self.entries()` before any move. Keep `Signing::Dev` default behavior intact for native.

- [ ] **Step 4: Run the new test + full suite**

Run: `cargo test -p greentic-pack-lib`
Expected: PASS (new test + all existing — byte-identical packs since assembly logic is unchanged, only relocated).

- [ ] **Step 5: Commit**

```bash
git checkout -b feat/packbuilder-entries
git add crates/greentic-pack/src/builder.rs
git commit -m "feat: add PackBuilder::entries() in-memory file set; build() reuses it"
```

### Task 3: Feature-gate native-only machinery; prove `entries()` builds on wasm

**Files:**
- Modify: `/home/bima-pangestu/Works/greentic/greentic-pack/crates/greentic-pack/Cargo.toml`
- Modify: `crates/greentic-pack/src/builder.rs` (cfg-gate zip/fs/signing-dev/component-fs)

**Interfaces:**
- Produces: `greentic-pack-lib` builds on `wasm32-wasip2` with `--no-default-features`, exposing `PackBuilder::new/with_*/entries` + `Signing::None`. Native default feature `native` keeps `build()`, `Signing::Dev`, zip.

- [ ] **Step 1: Add a `native` default feature; make heavy deps optional**

Edit `Cargo.toml`:
```toml
[features]
default = ["native"]
native = ["dep:zip", "dep:getrandom", "dep:ed25519-dalek", "dep:rcgen", "dep:x509-parser", "dep:time"]
```
Mark `zip`, `getrandom`, `ed25519-dalek`, `rcgen`, `x509-parser`, `time` as `optional = true`. (Keep whichever of these `entries()`+`Signing::None` does NOT need outside the gate; verify by compile.)

- [ ] **Step 2: cfg/feature-gate the native-only code paths**

- `use std::fs;`, `use zip::...`, `write_zip`, `build()` → `#[cfg(feature = "native")]`.
- `dev_signature()` + `Signing::Dev` arm + `getrandom`/`ed25519`/`rcgen`/`x509` uses → `#[cfg(feature = "native")]`.
- The component branch in `entries()` that does `fs::read(&component.wasm_path)` → `#[cfg(feature = "native")]` (operala packs add no components, so the feature-free `entries()` handles flows+assets+manifest only).
- `time::OffsetDateTime::now_utc()` in `finalize_provenance()` (line ~779): keep it `#[cfg(feature = "native")]`; when the feature is off, REQUIRE the caller-supplied `Provenance.built_at_utc` (operala already passes a fixed timestamp). If `provenance` is `None` under no-native, default `built_at_utc` to `"1970-01-01T00:00:00Z"`.

- [ ] **Step 3: Native build + tests still pass**

Run: `cargo build && cargo test -p greentic-pack-lib`
Expected: PASS (default = native).

- [ ] **Step 4: Prove wasm build of entries-only**

Run: `cargo build -p greentic-pack-lib --no-default-features --target wasm32-wasip2`
Expected: PASS. Iterate: any remaining wasm-failing dep (e.g. a transitive `ring` via rcgen that leaked outside the gate) → push it behind `native`. Stop when green.

- [ ] **Step 5: local_check + commit**

```bash
bash ci/local_check.sh
git add crates/greentic-pack/src/builder.rs crates/greentic-pack/Cargo.toml Cargo.lock
git commit -m "feat: feature-gate native pack machinery; entries() builds on wasm32-wasip2"
```

---

## Repo: greentic-operala (PR 3) — branch `feat/operala-designer-extension` (already created off `feat/llm-prompting`)

### Task 4: Workspace + extension crate scaffold (native compiles)

**Files:**
- Modify: `/home/bima-pangestu/Works/greentic/greentic-operala/Cargo.toml` (add `[workspace]`)
- Create: `crates/greentic-operala-designer-extension/Cargo.toml`
- Create: `crates/greentic-operala-designer-extension/src/lib.rs` (stub)

**Interfaces:**
- Produces: a buildable `rlib` crate `greentic-operala-designer-extension` depending on `greentic-operala` (path).

- [ ] **Step 1: Add workspace table to root `Cargo.toml`**

Append:
```toml
[workspace]
members = ["crates/greentic-operala-designer-extension"]
```
(Root remains the `greentic-operala` package + workspace root.)

- [ ] **Step 2: Create the extension crate Cargo.toml**

`crates/greentic-operala-designer-extension/Cargo.toml`:
```toml
[package]
name = "greentic-operala-designer-extension"
version = "0.1.0-research"
edition = "2024"
rust-version = "1.95"
license = "MIT"
repository = "https://github.com/greenticai/greentic-operala"
description = "Designer extension adapter for Greentic OperaLa."
publish = false

[lib]
name = "greentic_operala_designer_extension"
crate-type = ["rlib", "cdylib"]

[dependencies]
greentic-operala = { path = "../..", default-features = false }
base64 = "0.22"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
wit-bindgen = "0.44"
wit-bindgen-rt = "0.44"

[dev-dependencies]
greentic-extension-sdk-contract = "=1.2.7-research"

[package.metadata.component]
package = "greentic:operala-designer-extension"

[package.metadata.component.target]
path = "wit"
world = "design-extension"

[package.metadata.component.target.dependencies]
"greentic:extension-base"   = { path = "wit-deps/extension-base.wit" }
"greentic:extension-host"   = { path = "wit-deps/extension-host.wit" }
"greentic:extension-design" = { path = "wit-deps/extension-design.wit" }
```
NOTE: `greentic-operala` must expose a `default` feature set such that `default-features = false` drops the native-only deps (Task 5 introduces this).

- [ ] **Step 3: Stub lib**

`src/lib.rs`:
```rust
//! Designer extension adapter for Greentic OperaLa.
pub fn invoke_tool(name: &str, args_json: &str) -> Result<String, String> {
    let _ = (name, args_json);
    Err("not yet implemented".to_string())
}
```

- [ ] **Step 4: Native build**

Run: `cargo build -p greentic-operala-designer-extension`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/greentic-operala-designer-extension
git commit -m "feat: scaffold greentic-operala-designer-extension crate + workspace"
```

### Task 5: cfg-split operala lib so its wasm build drops the native runtime

**Files:**
- Modify: `/home/bima-pangestu/Works/greentic/greentic-operala/Cargo.toml` (features + target-gated deps)
- Modify: `src/inference/mod.rs` (gate `LlmRuntime`, `RigBackend`, `resolve_llm_request*`, the `greentic_llm` native imports)
- Modify: `src/lib.rs` (gate `greentic_pack` import + `write_operala_gtpack` + any `std::fs`/`std::env` reachable from the wasm surface)

**Interfaces:**
- Produces: `cargo build -p greentic-operala --no-default-features --lib --target wasm32-wasip2` succeeds. `ChatFn`, `infer_capability_answers`, `classify_capability`, `update_answers`, `UpdateOutcome`, the structs, `ExtensionRegistry`, `OperaLaExtension`, all `greentic_llm` types remain available on both targets.

- [ ] **Step 1: Restructure greentic-llm + tokio + greentic-pack deps target-conditionally**

Edit root `Cargo.toml`. Base (both targets), no rig:
```toml
greentic-llm = { version = "1.0.1", default-features = false }
greentic-pack = { package = "greentic-pack-lib", path = "../greentic-pack/crates/greentic-pack", version = "0.5", default-features = false }
```
Native adds the heavy features + tokio:
```toml
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
greentic-llm = { version = "1.0.1", features = ["rig", "clap"] }
greentic-pack = { package = "greentic-pack-lib", path = "../greentic-pack/crates/greentic-pack", version = "0.5", features = ["native"] }
tokio = { version = "1", features = ["rt"] }
```
(Cargo unions features for the same crate across the base + target tables.)

- [ ] **Step 2: Gate the native LLM runtime in `src/inference/mod.rs`**

- Change `use greentic_llm::{CredentialSource, EnvCredentialSource, LlmProvider, ProviderKind, RigBackend};` to import only the types both targets need at the top, and move `CredentialSource, EnvCredentialSource, LlmProvider, RigBackend` into a `#[cfg(not(target_arch = "wasm32"))]` `use`.
- Annotate `struct LlmRuntime`, `impl LlmRuntime`, `impl ChatFn for LlmRuntime`, `struct ResolvedLlm`, `resolve_llm_request`, `resolve_llm_request_from_process_env` with `#[cfg(not(target_arch = "wasm32"))]`.
- Keep `pub trait ChatFn`, `infer_capability_answers`, `classify_capability`, `update_answers`, `UpdateOutcome`, `pub struct UpdateOutcome` un-gated.

- [ ] **Step 3: Gate greentic-pack + fs in `src/lib.rs`**

- `use greentic_pack::builder::{...};` → `#[cfg(not(target_arch = "wasm32"))]` (it will be re-used through Task 7's shared fn, which itself is partly gated).
- `write_operala_gtpack`, `run_wizard`, and any `pub fn` that calls `fs::`/`env::var_os` (the CLI orchestration: `run_wizard`, `prompt_answers_with_llm` if it reads files, `LocalCacheArtifactResolver::from_env`) → `#[cfg(not(target_arch = "wasm32"))]`. Leave `build_handoff`, `analyse_sorla`, `validate_answers`, `follow_up_required`, `ExtensionRegistry`, the structs, and `load_sorla_contract` available (Task 6 adds the wasm-safe parse).

- [ ] **Step 4: Add a `default`/no-default feature seam if needed**

If `default-features = false` on `greentic-operala` doesn't already drop CLI-only deps (clap, etc.), introduce:
```toml
[features]
default = ["cli"]
cli = ["dep:clap"]
```
and gate `src/main.rs` / arg structs behind `#[cfg(feature = "cli")]`. The extension depends with `default-features = false`.

- [ ] **Step 5: Native build + tests unchanged**

Run: `cargo build && cargo test`
Expected: PASS.

- [ ] **Step 6: Prove operala lib builds on wasm**

Run: `cargo build -p greentic-operala --no-default-features --lib --target wasm32-wasip2`
Expected: PASS. Iterate on any transitive wasm break (e.g. `greentic-qa-lib`, `semver`, `serde_yaml`, `blake3`, `sha2`): gate the offending lib path behind `#[cfg(not(target_arch = "wasm32"))]` if it's CLI-only, or confirm the dep builds on wasip2. Stop when green.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/inference/mod.rs src/lib.rs src/main.rs
git commit -m "refactor: cfg-split operala lib so wasm build drops native LLM runtime + pack/fs"
```

### Task 6: `parse_sorla_contract_from_yaml` + string-threaded generate/update

**Files:**
- Modify: `src/lib.rs` (add `parse_sorla_contract_from_yaml`; refactor parsing core out of `load_sorla_contract`)
- Modify: `src/inference/mod.rs` (`update_answers` takes `&SorlaContract` not `sorla_path`)
- Test: `src/lib.rs` test module

**Interfaces:**
- Produces: `pub fn parse_sorla_contract_from_yaml(raw_yaml: &str) -> OperalaResult<SorlaContract>` (no FS); `pub fn update_answers(chat: &dyn ChatFn, existing: &OperalaAnswers, sorla: &SorlaContract, instruction: &str) -> OperalaResult<UpdateOutcome>`.
- Consumes: existing `yaml_named_list`, `yaml_id_list`, `yaml_string`, `sha256_hex`.

- [ ] **Step 1: Failing test for string parse**

```rust
#[test]
fn parse_sorla_from_yaml_matches_file_load() {
    let yaml = std::fs::read_to_string("extensions/reconciliation/examples/tenancy/sorla.yaml").unwrap();
    let parsed = parse_sorla_contract_from_yaml(&yaml).expect("parse");
    assert_eq!(parsed.raw_yaml, yaml);
    assert!(!parsed.records.is_empty());
    assert_eq!(parsed.source.kind, SourceKind::File); // or a new Inline kind — see step 3
}
```

- [ ] **Step 2: Run — fails (fn missing)**

Run: `cargo test parse_sorla_from_yaml_matches_file_load`
Expected: FAIL.

- [ ] **Step 3: Extract the pure parse core**

Refactor `load_sorla_contract` so the post-read body becomes:
```rust
pub fn parse_sorla_contract_from_yaml(raw_yaml: &str) -> OperalaResult<SorlaContract> {
    let actual_digest = format!("sha256:{}", sha256_hex(raw_yaml.as_bytes()));
    let yaml: serde_yaml::Value = serde_yaml::from_str(raw_yaml)
        .map_err(|err| format!("failed to parse SoRLa YAML: {err}"))?;
    let package = yaml.get("package").and_then(serde_yaml::Value::as_mapping)
        .ok_or_else(|| "SoRLa source must contain package".to_string())?;
    Ok(SorlaContract {
        source: SourceRef { kind: SourceKind::File, uri: String::new(), digest: Some(actual_digest.clone()) },
        source_digest: actual_digest,
        package_name: yaml_string(package, "name").unwrap_or_else(|| "unknown".to_string()),
        package_version: yaml_string(package, "version").unwrap_or_else(|| "0.1.0".to_string()),
        records: yaml_named_list(&yaml, "records"),
        events: yaml_named_list(&yaml, "events"),
        actions: yaml_named_list(&yaml, "actions"),
        agent_endpoints: yaml_id_list(&yaml, "agent_endpoints"),
        raw_yaml: raw_yaml.to_string(),
    })
}
```
`#[cfg(not(target_arch = "wasm32"))] load_sorla_contract` now reads the file then delegates to `parse_sorla_contract_from_yaml`, re-applying digest verification.

- [ ] **Step 4: Make `update_answers` take a parsed contract**

In `src/inference/mod.rs`, change `update_answers` signature to accept `sorla: &crate::SorlaContract` and delete the internal `load_sorla_contract(File)` call. Update the CLI caller (the `update` command) to parse first (it has a path; it can `load_sorla_contract`).

- [ ] **Step 5: Run tests**

Run: `cargo test` (native) — Expected: PASS, including the existing `inference` driver tests.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/inference/mod.rs
git commit -m "feat: parse SoRLa from in-memory yaml; update_answers takes parsed contract"
```

### Task 7: `build_operala_pack_entries` — in-memory pack file set via `PackBuilder::entries()`

**Files:**
- Modify: `src/lib.rs` (extract assembly from `write_operala_gtpack`; new shared fn)
- Test: `src/lib.rs` test module

**Interfaces:**
- Produces: `pub fn build_operala_pack_entries(handoff: &OperaLaHandoff) -> OperalaResult<Vec<(String, Vec<u8>)>>` (no FS, no zip). `#[cfg(not(target_arch = "wasm32"))] write_operala_gtpack` rewires through it.
- Consumes: `greentic_pack::builder::{PackBuilder, PackMeta, Provenance, Signing, FlowBundle, PACK_VERSION}` (the base dep, available on both targets after Task 5), `PackBuilder::entries()` (Task 2/3).

- [ ] **Step 1: Failing test for entries**

```rust
#[test]
fn pack_entries_include_manifest_and_handoff() {
    let handoff = sample_handoff(); // build via analyse_sorla+build_handoff over a fixture
    let entries = build_operala_pack_entries(&handoff).expect("entries");
    let paths: Vec<&str> = entries.iter().map(|(p, _)| p.as_str()).collect();
    assert!(paths.contains(&"manifest.cbor"));
    assert!(paths.contains(&"operala/operala-handoff.json"));
    assert!(paths.contains(&"operala/operala.yaml"));
}
```

- [ ] **Step 2: Run — fails**

Run: `cargo test pack_entries_include_manifest`
Expected: FAIL.

- [ ] **Step 3: Extract assembly into shared fn**

Move the `PackMeta`/`Provenance`/`FlowBundle`/`with_asset_bytes` assembly from `write_operala_gtpack` (lib.rs:1839-1950) into:
```rust
pub fn build_operala_pack_entries(handoff: &OperaLaHandoff) -> OperalaResult<Vec<(String, Vec<u8>)>> {
    let builder = build_operala_pack_builder(handoff)?; // the PackBuilder assembly, FS-free
    let map = builder.entries().map_err(to_string)?;
    Ok(map.into_iter().collect())
}
```
Factor the builder assembly into `fn build_operala_pack_builder(handoff: &OperaLaHandoff) -> OperalaResult<PackBuilder>` containing lines 1839-1950 (meta with fixed `created_at_utc`/`built_at_utc`, `Signing::None`, the two assets, the flow loop, the schema loop) — note it already uses `Signing::None` and pre-filled timestamps, so it is wasm-safe. Keep `build_operala_pack_builder` available on both targets.
Rewire native `#[cfg(not(target_arch = "wasm32"))] write_operala_gtpack`:
```rust
fn write_operala_gtpack(path: &Path, handoff: &OperaLaHandoff) -> OperalaResult<()> {
    // ... existing path cleanup ...
    let builder = build_operala_pack_builder(handoff)?;
    builder.build(path).map_err(to_string)?;
    Ok(())
}
```

- [ ] **Step 4: Run native tests (pack output unchanged)**

Run: `cargo test`
Expected: PASS — the `.gtpack` produced by the CLI is byte-identical (same assembly, same Signing::None).

- [ ] **Step 5: Commit**

```bash
git add src/lib.rs
git commit -m "feat: build_operala_pack_entries via PackBuilder::entries(); CLI reuses assembly"
```

### Task 8: Extension tool logic + WIT + describe + HostLlm adapter

**Files:**
- Create: `crates/greentic-operala-designer-extension/wit/world.wit`
- Create: `crates/greentic-operala-designer-extension/wit-deps/extension-base.wit`, `extension-host.wit`, `extension-design.wit`
- Create: `crates/greentic-operala-designer-extension/describe.json`
- Modify: `crates/greentic-operala-designer-extension/src/lib.rs` (5 tool arms + native ChatFn stub)
- Create: `crates/greentic-operala-designer-extension/src/component.rs` (`#[cfg(target_arch = "wasm32")]` Guest impls + `HostLlmChat`)
- Test: `src/lib.rs` test module

**Interfaces:**
- Consumes: operala `parse_sorla_contract_from_yaml`, `ExtensionRegistry`, `OperaLaExtension`, `infer_capability_answers`, `classify_capability`, `update_answers`, `analyse_sorla`, `build_operala_pack_entries`, `ChatFn`, `ChatRequest`/`ChatResponse`/`ChatMessage`/`MessageRole`/`FinishReason` (re-exported from `greentic_llm` via operala).
- Produces: `invoke_tool(name, args_json) -> Result<String, String>` dispatching the 5 tools with the verified result shapes.

- [ ] **Step 1: Copy WIT scaffolding from sorla**

```bash
S=/home/bima-pangestu/Works/greentic/greentic-sorla/crates/greentic-sorla-designer-extension
D=/home/bima-pangestu/Works/greentic/greentic-operala/crates/greentic-operala-designer-extension
mkdir -p $D/wit $D/wit-deps
cp $S/wit-deps/extension-base.wit $S/wit-deps/extension-host.wit $S/wit-deps/extension-design.wit $D/wit-deps/
```
Create `$D/wit/world.wit` (package renamed; same imports/exports as sorla's):
```wit
package greentic:operala-designer-extension;

world design-extension {
  import greentic:extension-base/types@0.1.0;
  import greentic:extension-host/logging@0.1.0;
  import greentic:extension-host/i18n@0.1.0;
  import greentic:extension-host/secrets@0.1.0;
  import greentic:extension-host/broker@0.1.0;
  import greentic:extension-host/http@0.1.0;
  import greentic:extension-host/llm@0.1.0;

  export greentic:extension-base/manifest@0.1.0;
  export greentic:extension-base/lifecycle@0.1.0;
  export greentic:extension-design/tools@0.2.0;
  export greentic:extension-design/validation@0.2.0;
  export greentic:extension-design/prompting@0.2.0;
  export greentic:extension-design/knowledge@0.2.0;
}
```

- [ ] **Step 2: describe.json**

Create `$D/describe.json` (mirror sorla's shape; operala values):
```json
{
  "$schema": "https://store.greentic.cloud/schemas/describe-v2.json",
  "apiVersion": "greentic.ai/v2",
  "kind": "DesignExtension",
  "compat": { "min_designer_version": ">=1.2.0", "min_runner_version": "^0.12.0", "contract_version": "1.2.0" },
  "metadata": {
    "id": "greentic.operala",
    "name": "OperaLa Composer",
    "version": "0.1.0-research",
    "summary": "Author operational-behaviour handoff artifacts from a guided prompt session.",
    "description": "Author and manage OperaLa operational behaviours anchored to a SoRLa contract.",
    "author": { "name": "Greentic AI", "email": "team@greentic.ai" },
    "license": "MIT"
  },
  "engine": { "greenticDesigner": ">=1.2.0", "extRuntime": "^1.2.0" },
  "capabilities": { "offered": [ { "id": "greentic:operala/composition", "version": "1.0.0" } ], "required": [] },
  "runtime": {
    "memoryLimitMB": 64,
    "permissions": { "llmRoles": ["operala_composer"] },
    "components": {
      "operala": {
        "gtpack": { "file": "extension.wasm", "sha256": "0000000000000000000000000000000000000000000000000000000000000000", "pack_id": "greentic.operala", "component_version": "0.1.0" },
        "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
        "world": "greentic:operala-designer-extension/design-extension"
      }
    }
  },
  "contributions": {
    "tools": [
      { "name": "list_operala_capabilities", "export": "invoke-tool" },
      { "name": "generate_operala_answers", "export": "invoke-tool" },
      { "name": "update_operala_answers", "export": "invoke-tool" },
      { "name": "validate_operala_answers", "export": "invoke-tool" },
      { "name": "generate_handoff_pack", "export": "invoke-tool" }
    ]
  }
}
```

- [ ] **Step 3: Failing tests for the pure tool arms**

In `src/lib.rs` test module:
```rust
#[test]
fn list_capabilities_returns_both() {
    let out = invoke_tool("list_operala_capabilities", "{}").unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let caps = v["capabilities"].as_array().unwrap();
    assert_eq!(caps.len(), 2);
}
#[test]
fn validate_returns_readiness() {
    let yaml = std::fs::read_to_string("../../extensions/reconciliation/examples/tenancy/sorla.yaml").unwrap();
    let answers = std::fs::read_to_string("../../extensions/reconciliation/examples/tenancy/answers.json").unwrap();
    let args = serde_json::json!({ "sorla_yaml": yaml, "answers": serde_json::from_str::<serde_json::Value>(&answers).unwrap() }).to_string();
    let out = invoke_tool("validate_operala_answers", &args).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v.get("readiness").is_some());
}
#[test]
fn handoff_returns_pack_entries() {
    let yaml = std::fs::read_to_string("../../extensions/reconciliation/examples/tenancy/sorla.yaml").unwrap();
    let answers = std::fs::read_to_string("../../extensions/reconciliation/examples/tenancy/answers.json").unwrap();
    let args = serde_json::json!({ "sorla_yaml": yaml, "answers": serde_json::from_str::<serde_json::Value>(&answers).unwrap() }).to_string();
    let out = invoke_tool("generate_handoff_pack", &args).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let entries = v["pack_entries"].as_array().unwrap();
    assert!(entries.iter().any(|e| e["path"] == "manifest.cbor"));
    assert!(entries[0]["sha256"].is_string() && entries[0]["content_base64"].is_string());
}
```

- [ ] **Step 4: Run — fail**

Run: `cargo test -p greentic-operala-designer-extension`
Expected: FAIL (`not yet implemented`).

- [ ] **Step 5: Implement the 5 tool arms (pure, native-testable)**

In `src/lib.rs`, implement `invoke_tool` dispatch + helpers. Sketch (use real operala APIs):
```rust
use greentic_operala as op;
use serde_json::{json, Value};

pub fn invoke_tool(name: &str, args_json: &str) -> Result<String, String> {
    let input: Value = serde_json::from_str(args_json).map_err(|e| e.to_string())?;
    let out = match name {
        "list_operala_capabilities" => list_capabilities(),
        "generate_operala_answers" => generate_answers(&input, &chat())?,
        "update_operala_answers" => update_answers_tool(&input, &chat())?,
        "validate_operala_answers" => validate_answers_tool(&input)?,
        "generate_handoff_pack" => generate_handoff_pack(&input)?,
        other => return Err(format!("unknown OperaLa tool `{other}`")),
    };
    serde_json::to_string(&out).map_err(|e| e.to_string())
}

fn list_capabilities() -> Value {
    let reg = op::ExtensionRegistry::built_in();
    let exts: Vec<Value> = reg.all().iter().map(|e| json!({
        "id": e.id(), "capability": e.capability(), "version": e.version()
    })).collect();
    json!({ "capabilities": exts.iter().map(|e| e["capability"].clone()).collect::<Vec<_>>(), "extensions": exts })
}

fn sorla_from(input: &Value) -> Result<op::SorlaContract, String> {
    let yaml = input.get("sorla_yaml").and_then(Value::as_str).ok_or("missing sorla_yaml")?;
    op::parse_sorla_contract_from_yaml(yaml)
}

fn generate_answers(input: &Value, chat: &dyn op::inference::ChatFn) -> Result<Value, String> {
    let sorla = sorla_from(input)?;
    let prompt = input.get("prompt").and_then(Value::as_str).ok_or("missing prompt")?;
    let capability = match input.get("capability").and_then(Value::as_str) {
        Some(c) => c.to_string(),
        None => op::inference::classify_capability(chat, prompt)?
            .ok_or_else(|| "could not classify capability".to_string())?,
    };
    let (ext_id, schema) = capability_ids(&capability)?;
    match op::inference::infer_capability_answers(chat, ext_id, &schema, &sorla, prompt, None) {
        Ok(answers) => Ok(json!({ "answers": answers })),
        Err(e) if e.starts_with("follow-up required:") =>
            Ok(json!({ "follow_up": e.trim_start_matches("follow-up required:").trim() })),
        Err(e) => Err(e),
    }
}
// update_answers_tool: parse existing OperalaAnswers + instruction -> op::inference::update_answers(chat, &existing, &sorla, instruction) -> json!({"answers": .answers, "diff": .diff})
// validate_answers_tool: registry.get(answers.extension).analyse_sorla(&sorla, &answers) -> json!({"readiness": report})
// generate_handoff_pack: analyse_sorla -> build_handoff -> op::build_operala_pack_entries(&handoff) -> entries to [{path, sha256(sha2), content_base64(STANDARD)}] + {"handoff": handoff}
```
Add `capability_ids(name) -> (ext_id, answers_schema Value)` mapping `"reconciliation"`/`"bulk_ingest"` to the `EXTENSION_*` consts + the matching `*_EXTENSION.answers_schema()` (expose a helper from operala if these statics aren't public).
`chat()` returns the native stub in tests / `HostLlmChat` on wasm:
```rust
#[cfg(target_arch = "wasm32")]
fn chat() -> impl op::inference::ChatFn { crate::component::HostLlmChat }
#[cfg(not(target_arch = "wasm32"))]
fn chat() -> impl op::inference::ChatFn { NativeStubChat } // returns a fixed follow_up; tests inject their own via the *_with_chat helpers
```
For testability, give `generate`/`update` an inner `*_with_chat(input, chat)` taking `&dyn ChatFn`, and have the LLM tests call those with a scripted `ChatFn` (reuse operala's `tests_support::scripted_chat` pattern, or a local stub returning `ChatResponse{content, tool_calls: vec![], finish_reason: Stop}` with `tools_supported()->false`).

- [ ] **Step 6: Run pure-arm tests**

Run: `cargo test -p greentic-operala-designer-extension`
Expected: PASS for `list`/`validate`/`handoff`; add a `generate`/`update` test with a scripted no-tools `ChatFn` returning `{"emit_answers": <reconciliation answers>}`.

- [ ] **Step 7: Implement `src/component.rs` (wasm Guest + HostLlmChat)**

Mirror sorla's `component.rs`. `HostLlmChat` impl of `op::inference::ChatFn`:
```rust
pub struct HostLlmChat;
impl op::inference::ChatFn for HostLlmChat {
    fn tools_supported(&self) -> bool { false }
    fn chat(&self, request: op::ChatRequest) -> Result<op::ChatResponse, op::LlmError> {
        let mut system = String::new();
        let mut messages = Vec::new();
        for m in &request.messages {
            match m.role {
                op::MessageRole::System => { if !system.is_empty() { system.push('\n'); } system.push_str(&m.content); }
                op::MessageRole::User => messages.push(host_llm::LlmMessage { role: "user".into(), content: m.content.clone() }),
                op::MessageRole::Assistant => messages.push(host_llm::LlmMessage { role: "assistant".into(), content: m.content.clone() }),
            }
        }
        let req = host_llm::LlmRequest {
            role_hint: None, system_prompt: system, messages,
            response_format: Some(host_llm::ResponseFormat::Json),
        };
        match host_llm::complete(&req) {
            Ok(r) => Ok(op::ChatResponse { content: r.content, tool_calls: vec![], finish_reason: op::FinishReason::Stop }),
            Err(e) => Err(op::LlmError::Provider(format!("host LLM completion failed: {e}"))),
        }
    }
}
```
(Confirm the `LlmError` variant name from `greentic_llm::provider::LlmError`; use the closest provider/other variant.)
Guest impls (`manifest`, `lifecycle`, `tools`, `validation`, `prompting`, `knowledge`) mirror sorla's, calling `crate::invoke_tool` for `tools::invoke_tool`, `crate::list_tools()` for `list_tools`, and returning empty vecs for prompting/knowledge/validation defaults. `get_identity` returns id `greentic.operala`, kind `Design`. End with `crate::bindings::export!(Component with_types_in crate::bindings);`. Gate the whole module via `#[cfg(target_arch = "wasm32")] mod component;` in `lib.rs`.
Add `pub fn list_tools() -> Vec<ToolDefinitionLite>` returning the 5 tool names + minimal input/output schema JSON strings.

- [ ] **Step 8: Commit**

```bash
git add crates/greentic-operala-designer-extension
git commit -m "feat: implement 5 operala designer tools + WIT/describe + HostLlm adapter"
```

### Task 9: Build the WASM component (cargo-component) + validate describe.json

**Files:** none new — build + contract test.

- [ ] **Step 1: Install toolchain**

Run: `rustup target add wasm32-wasip2; cargo install cargo-component --locked` (skip if present).

- [ ] **Step 2: Build the component**

Run: `cd crates/greentic-operala-designer-extension && cargo component build --release --target wasm32-wasip2`
Expected: PASS, produces `target/wasm32-wasip2/release/greentic_operala_designer_extension.wasm`. Iterate on any wasm break (most likely a transitive dep that slipped the cfg gates in Task 5 — fix there).

- [ ] **Step 3: Contract test for describe.json**

Add a test using `greentic-extension-sdk-contract` that loads `describe.json` and asserts it matches the v2 describe schema + the 5 tool names. (Follow sorla's describe-validation test pattern.)
Run: `cargo test -p greentic-operala-designer-extension describe`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/greentic-operala-designer-extension
git commit -m "test: validate operala describe.json against extension contract; wasm component builds"
```

### Task 10: Release workflow (Store publish job)

**Files:**
- Create: `.github/workflows/release-binaries.yml` (or add a job to the existing release workflow)

- [ ] **Step 1: Add the publish job (mirror sorla)**

```yaml
publish-designer-extension:
  if: github.ref_type == 'tag'
  name: Publish OperaLa designer extension to Greentic Store
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: greenticai/greentic-designer-extension-action@v2
      with:
        gtdx-version: "=1.2.7-research"
        manifest: crates/greentic-operala-designer-extension/Cargo.toml
        store-url: https://store.greentic.cloud
        store-token: ${{ secrets.GREENTIC_STORE_TOKEN }}
        version: ${{ github.ref_name }}
        rust-toolchain: "1.95.0"
```
(`gtdx-version` MUST equal the `greentic-extension-sdk-contract` dev-dep pin `=1.2.7-research`.)

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/release-binaries.yml
git commit -m "ci: publish operala designer extension to Greentic Store on tag"
```

- [ ] **Step 3: `bash ci/local_check.sh` for the whole repo; open PR 3 → research/main per landing decision.**

---

## Repo: greentic-designer (PR 4) — branch off `research`

### Task 11: Bundle the extension into the designer

**Files:**
- Create: `bundled/greentic.operala-0.1.0-research.gtxpack`
- Modify: `bundled/manifest.json`

**Interfaces:**
- Consumes: a `.gtxpack` produced by `gtdx` from the operala extension crate.

- [ ] **Step 1: Build the gtxpack locally (no Store publish)**

Run (with `gtdx` installed, pinned `=1.2.7-research`):
```bash
cd /home/bima-pangestu/Works/greentic/greentic-operala/crates/greentic-operala-designer-extension
gtdx pack --manifest Cargo.toml --version 0.1.0-research --out /tmp/greentic.operala-0.1.0-research.gtxpack
```
(Use the gtdx subcommand that builds a `.gtxpack` without Store auth; if only `publish` exists, run it against a local/dummy store or use `--dry-run --keep-artifact`. Confirm the exact gtdx build subcommand from `gtdx --help`.)

- [ ] **Step 2: Copy into designer + compute sha256**

```bash
cp /tmp/greentic.operala-0.1.0-research.gtxpack /home/bima-pangestu/Works/greentic/greentic-designer/bundled/
sha256sum /home/bima-pangestu/Works/greentic/greentic-designer/bundled/greentic.operala-0.1.0-research.gtxpack
```

- [ ] **Step 3: Add the manifest entry**

In `bundled/manifest.json`, append after the sorla entry:
```json
{
  "name": "greentic.operala",
  "kind": "DesignExtension",
  "version": "0.1.0-research",
  "sha256": "<sha256 from step 2>",
  "file": "greentic.operala-0.1.0-research.gtxpack"
}
```

- [ ] **Step 4: Smoke test — extension loads, routes live**

```bash
cd /home/bima-pangestu/Works/greentic/greentic-designer
cargo run --bin greentic-designer -- ui   # or the repo's run command
```
Verify: `/api/extensions` lists `greentic.operala` enabled; an `/api/operala/*` call no longer returns `404 operala_extension_unavailable`; one `generate_operala_answers` round-trip with a tenant context resolves through `host.llm`.

- [ ] **Step 5: Commit + PR**

```bash
git add bundled/manifest.json bundled/greentic.operala-0.1.0-research.gtxpack
git commit -m "feat: bundle greentic.operala designer extension"
```
Open PR 4 → `research`.

---

## Self-Review

**Spec coverage:** greentic-llm gate (T1 ↔ spec §1); greentic-pack entries (T2/T3 ↔ §2); operala cfg-split (T5 ↔ §3), sorla-from-string (T6), pack entries (T7), extension crate + 5 tools + HostLlm + describe + workflow (T4/T8/T9/T10 ↔ §3); designer bundle (T11 ↔ §4). All 5 tool result shapes covered (T8 arms). Testing §: T1 wasm check, T5/T6/T7 native tests, T8/T9 arm + describe tests, T11 smoke. ✓

**Placeholder scan:** No "TBD/TODO". Two empirical-iterate steps (T3 step 4, T5 step 6, T9 step 2) are deliberate ("gate whatever fails" with a concrete command + stop condition), not placeholders. The `gtdx pack` subcommand name (T11 step 1) is flagged to confirm from `gtdx --help`.

**Type consistency:** `ChatFn`/`ChatRequest`/`ChatResponse`/`MessageRole`/`FinishReason` used consistently (operala re-exports greentic_llm types); `build_operala_pack_entries`/`build_operala_pack_builder`/`parse_sorla_contract_from_yaml`/`PackBuilder::entries` names consistent across tasks; `PlanEntry{path,sha256,content_base64}` matches the designer's deserialize target. ✓

## Risks / watch-items

- `greentic_llm::LlmError` variant name in HostLlmChat (T8 step 7) — confirm at impl time.
- `RECONCILIATION_EXTENSION`/`BULK_INGEST_EXTENSION` statics are private — T8 needs operala to expose `answers_schema` per capability (add a small `pub fn` if absent).
- wasm-compat of transitive operala deps (`greentic-qa-lib`, `serde_yaml`, `blake3`) — surfaces at T5 step 6 / T9 step 2; gate CLI-only ones behind `cfg(not(wasm))`.
- `feat/llm-prompting` must reach a shared base before PR 3 merges (engine dependency).
