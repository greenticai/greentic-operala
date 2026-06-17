# OperaLa Designer Extension — Design Spec

- **Date:** 2026-06-17
- **Status:** Approved (design); spec under review
- **Repos touched:** `greentic-llm` (greenticai), `greentic-operala` (greenticai), `greentic-designer` (research line)
- **Goal:** Ship the `greentic.operala` WASM design-extension so the greentic-designer's already-wired `/api/operala/*` routes become functional for the first time.

## Background / Problem

The greentic-designer already dispatches OperaLa authoring through `dispatch_operala`
(`src/ui/tool_bridge/dispatch.rs`) to five WASM tools owned by extension id
`greentic.operala`. **No such extension exists**, so today every `/api/operala/*`
route returns `404 operala_extension_unavailable` (`src/ui/routes/operala/mod.rs`).
There is **no in-process fallback** — OperaLa is 100% extension-dependent and
currently non-functional end-to-end.

`greentic-operala` itself is a working CLI library+binary with a real,
LLM-backed inference engine (`src/inference/`, branch `feat/llm-prompting`). It is
NOT yet packaged as a Wasm component. This spec covers building that component by
wrapping the existing engine — no new authoring logic.

## Contract (dictated by the designer — do not invent)

Extension id: `greentic.operala`. LLM role: `operala_composer`. Stateless — the
designer owns session state (`operala_sessions` table). The five tools, with exact
I/O shapes the designer builds/parses:

| Tool | Input JSON | Output JSON | LLM | Operala lib entry |
|---|---|---|---|---|
| `list_operala_capabilities` | `{}` | `{ capabilities, extensions }` | no | `ExtensionRegistry::all()` + manifest |
| `generate_operala_answers` | `{ sorla_yaml, prompt, capability? }` | `{ follow_up? } \| { answers }` | **yes** | `classify_capability` (if needed) → `inference::infer_capability_answers` |
| `update_operala_answers` | `{ sorla_yaml, answers, instruction }` | `{ answers, diff? }` | **yes** | `inference::update_answers` |
| `validate_operala_answers` | `{ sorla_yaml, answers }` | `{ readiness }` | no | `OperaLaExtension::analyse_sorla` → `ReadinessReport` |
| `generate_handoff_pack` | `{ sorla_yaml, answers }` | `{ handoff, pack_entries }` | no | `OperaLaExtension::build_handoff` → `OperaLaHandoff` |

Two tools (`generate`, `update`) require LLM access; three are pure transformations.

## Architecture

```
greentic-designer (research)
  dispatch_operala ──invoke_tool_ctx(host.llm via tenant ctx)──┐
                                                               ▼
  bundled/greentic.operala-<ver>.gtxpack  ◀── gtdx build ── greentic-operala-designer-extension (WASM)
                                                               │ wraps
                                                               ▼
                                                   greentic-operala lib (types-only on wasm)
                                                               │ depends (default-features=false)
                                                               ▼
                                                   greentic-llm (types-only; rig behind `rig` feature)
```

The WASM component implements `ChatFn` (the existing inference seam,
`src/inference/mod.rs:268`) backed by the `host.llm` WIT import, then calls the
existing `infer_capability_answers` / `update_answers` / `classify_capability`
functions — which are already backend-agnostic (`&dyn ChatFn`). The native CLib path
(`LlmRuntime` + `RigBackend` + tokio) is excluded from the wasm build via `cfg`.

### Why the existing seam fits

- `ChatFn::chat(ChatRequest) -> Result<ChatResponse, LlmError>` + `tools_supported()`
  is the only LLM dependency the inference functions have. `LlmRuntime` is just one
  impl (native, rig). A second impl backed by `host.llm` is all WASM needs.
- Retry/validation/diff/follow-up logic already lives in the lib and is target-agnostic.
- Decision: **wrap operala's own engine.** Do NOT pull in `greentic-sorla-lib`'s
  `PromptAuthoringEngine` (earlier mis-assumption — superseded by code-state check).

## Components & Changes

### 1. `greentic-llm` (greenticai, `main`, 1.0.x line) — feasibility unblock

The current 1.0.0 has `rig-core` as a **non-optional** dependency. `rig-core`
(reqwest/HTTP/tokio) does not build on `wasm32-wasip2`, so anything depending on
`greentic-llm` cannot compile to wasm. All `rig` references in `provider.rs` /
`capabilities.rs` are **doc-comments only**; the actual type definitions
(`ChatRequest`, `ChatResponse`, `ChatMessage`, `MessageRole`, `ToolDef`, `ToolCall`,
`FinishReason`, `LlmError`, `ProviderKind`, `Capabilities`, `LlmProvider` trait) are
rig-free. `rig-core` is used only in `src/rig_backend.rs`.

Change (backward-compatible):
- `rig-core = { version = "0.35", optional = true }`
- `[features] default = ["rig"]; rig = ["dep:rig-core"]`
- `#[cfg(feature = "rig")] pub mod rig_backend;` and gate `pub use rig_backend::RigBackend;`
- Verify `credentials/` (`EnvCredentialSource`) and `async-trait`/`futures-util`/
  `chrono`/`zeroize` compile on `wasm32-wasip2` with `--no-default-features`.
- Republish **1.0.1** to crates.io (org token; no upstream gate). Default-on `rig`
  keeps every existing native consumer unchanged.

### 2. `greentic-operala` (greenticai)

Branch off `feat/llm-prompting` (the engine the extension wraps lives there and is
not yet on `main`/`research`). Workspace: minimal-churn — add
`[workspace] members = ["crates/greentic-operala-designer-extension"]`; root stays the
`greentic-operala` lib+bin.

Lib changes:
- **cfg-split the native runtime.** Gate `LlmRuntime`, `RigBackend` usage,
  `resolve_llm_request*`, and the tokio runtime behind `#[cfg(not(target_arch = "wasm32"))]`.
  `ChatFn`, `infer_capability_answers`, `classify_capability`, `update_answers`,
  `UpdateOutcome`, and all `greentic_llm::` *types* stay for both targets.
- **greentic-llm dep:** `default-features = false` for the wasm build (drop rig);
  keep `clap`/native features on the non-wasm build. Bump pin to `1.0.1`.
- **sorla-from-string.** `update_answers` (and the generate path) currently call
  `load_sorla_contract(SourceKind::File, …)` — file I/O. The designer sends
  `sorla_yaml` as a **string**. Add a `parse_sorla_contract_from_yaml(&str)` (or
  refactor `load_sorla_contract` to take an in-memory source) and thread the string
  through generate/update instead of a path. No filesystem in the wasm sandbox.
- **Expose entry points** the extension calls. Either thin `pub fn`s returning
  `serde_json::Value`, or let the extension marshal directly against the existing
  functions (chosen at plan time; lean to thin wrappers for testability).

New crate `crates/greentic-operala-designer-extension/`:
- `Cargo.toml`: `crate-type = ["rlib", "cdylib"]`; deps `greentic-operala` (path,
  `default-features = false`), `wit-bindgen`/`wit-bindgen-rt` 0.44, `serde`/`serde_json`,
  `base64`; dev-dep `greentic-extension-sdk-contract = "=1.2.7-research"`;
  `[package.metadata.component]` → world `design-extension`.
- `wit/world.wit` + `wit-deps/extension-{base,host,design}.wit`: mirror
  `greentic-sorla/crates/greentic-sorla-designer-extension`. Must import
  `greentic:extension-host/llm` (the `host.llm` port) and export
  manifest/lifecycle/tools/validation/prompting/knowledge.
- `src/lib.rs`: `invoke_tool(name, args_json) -> Result<String, String>` dispatch with
  five arms matching the contract table; pure JSON marshalling around the lib calls.
- `src/component.rs` (`#[cfg(target_arch = "wasm32")]`): wit-bindgen `Guest` impls;
  a `HostLlmChat` struct implementing `ChatFn` by calling the `host.llm` import
  (synchronous in wasm — no tokio); `tools_supported()` reports the host's capability.
- `describe.json`: id `greentic.operala`, `kind: DesignExtension`,
  `llmRoles: ["operala_composer"]`, the five tools, `memoryLimitMB: 64`, schema/apiVersion
  matching the SoRLa describe-v2 shape.
- `.github/workflows/release-binaries.yml`: add a publish job using
  `greenticai/greentic-designer-extension-action@v2`, `gtdx-version: "=1.2.7-research"`,
  `manifest: crates/greentic-operala-designer-extension/Cargo.toml`,
  `store-url: https://store.greentic.cloud`, `store-token: ${{ secrets.GREENTIC_STORE_TOKEN }}`,
  `rust-toolchain: "1.95.0"`, on `push tags: ["v*"]` + `workflow_dispatch`.

### 3. `greentic-designer` (research line)

- Build `greentic.operala-<ver>.gtxpack` locally via `gtdx` (build, not Store publish)
  and commit it to `bundled/`.
- Add the entry to `bundled/manifest.json` (name `greentic.operala`, kind
  `DesignExtension`, version, sha256, file). This **decouples the bundle deliverable
  from the Store token** — `refresh-bundled.sh`/Store publish is the separate
  distribution path.

## Version & naming

- Extension + crate version: **`0.1.0-research`** (research line; align to operala's
  own version at plan time if it already carries a `-research` suffix).
- `greentic-llm`: **`1.0.1`** (main line, additive feature-gate).
- No invented product names in any public artifact — `OperaLa` / `greentic.operala` only.

## Testing

- **greentic-llm:** existing native tests unchanged; add a `--no-default-features`
  `cargo check --target wasm32-wasip2` to confirm types-only builds.
- **operala lib:** existing `inference` driver tests (scripted `ChatFn` mock) keep
  passing; add tests for `parse_sorla_contract_from_yaml` and the string-threaded
  generate/update paths.
- **extension crate:** native unit tests per `invoke_tool` arm (pure 3 directly;
  LLM 2 with a scripted-`ChatFn` stub); `greentic-extension-sdk-contract` dev-dep
  validates `describe.json` against the v2 schema; `cargo build --target wasm32-wasip2`
  must succeed.
- **integration smoke:** designer loads the bundled extension →
  `find_owning_extension` resolves the five tools → `/api/operala/*` no longer 404 →
  one happy-path `generate_operala_answers` round-trip with a tenant ctx (host.llm).

## Delivery / PR plan

1. **greentic-llm** PR (feature-gate rig + wasm check) → merge → publish 1.0.1.
2. **greentic-operala** PR off `feat/llm-prompting`: cfg-split + sorla-from-string +
   extension crate + describe + release workflow. (Depends on 1 for the pin bump.)
3. **greentic-designer** PR (research): bundled manifest entry + committed gtxpack.

Sequencing: 1 before 2 (pin). 3 after the extension builds a gtxpack. The
`feat/llm-prompting` engine must reach a shared branch (or 2 is explicitly based on it).

## Gates / open items

- **`GREENTIC_STORE_TOKEN`** must be provisioned as a GitHub Actions secret in
  `greenticai/greentic-operala` for the Store publish job (job 2's release workflow).
  The designer-bundle deliverable (job 3) does NOT need it. **Token handling for the
  `gts_` key pasted in chat is unresolved — awaiting the user's instruction; it will
  not be used or written anywhere until then, and rotation is advised if shared
  unintentionally.**
- Confirm `feat/llm-prompting` landing target (main vs research) so job 2 has a stable base.
- Confirm `greentic-llm` republish version (`1.0.1`) and that no other consumer pins `=1.0.0`.

## Non-goals

- No new OperaLa authoring/capability logic — pure packaging + wrap.
- No changes to the designer's `dispatch_operala` / route layer (already correct).
- No Store distribution dependency for getting the extension into the designer bundle.
- No `greentic-sorla-lib` dependency.
