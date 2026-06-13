# greentic-operala

OperaLa turns business operations intent into deterministic handoff artifacts.

In simple terms: SoRLa describes the system of record, such as records, events,
actions, and agent endpoints. OperaLa sits next to that and describes the
operational business logic that should run against those SoRLa contracts. It
does not mutate SoRLa directly. It reads a SoRLa source, asks or infers the
operational answers, checks whether the SoRLa contract has the pieces needed,
and writes handoff files that a runner such as `greentic-operax` can execute.

The first built-in capability is tenancy rent reconciliation from bank
transactions.

## What It Can Do

- Read a SoRLa YAML source.
- Create an `answers.json` draft from a business prompt.
- Emit a localized QA schema for authoring tools.
- Validate answers and analyse readiness against the SoRLa contract.
- Propose additive SoRLa patch material when required records, events, actions,
  or endpoints are missing.
- Generate deterministic OperaLa handoff artifacts.
- Build a `.gtpack` using `greentic-pack`.
- Hand the generated pack to the installed `greentic-operax` runner.

## Install

Future release builds are intended to be installable with `cargo-binstall`:

```bash
cargo binstall greentic-operala
cargo binstall greentic-operax
```

For local development, run the OperaLa binary from this checkout:

```bash
cargo run --bin greentic-operala -- --help
```

## Quick Example

Generate answers from the tenancy reconciliation prompt:

```bash
cargo run --bin greentic-operala -- prompt \
  --locale en-GB \
  --tenant demo-tenant \
  --team property-ops \
  --sorla extensions/reconciliation/examples/tenancy/sorla.yaml \
  --output target/operala-demo/answers.json \
  "$(cat extensions/reconciliation/examples/tenancy/prompt.txt)"
```

Turn those answers into handoff artifacts and a `.gtpack`:

```bash
cargo run --bin greentic-operala -- wizard \
  --answers target/operala-demo/answers.json \
  --locale en-GB
```

The main outputs are:

- `target/operala/tenancy_rent_reconciliation/operala-handoff.json`
- `target/operala/tenancy_rent_reconciliation/operala.build.lock`
- `target/operala/tenancy_rent_reconciliation/tenancy-rent-reconciliation.gtpack`

## LLM-backed prompting

`prompt` uses an LLM to infer answers when one is configured; otherwise it falls back to the deterministic keyword path (a one-line `note:` on stderr).

```bash
# Configure a provider (any greentic-llm provider: openai, anthropic, ollama, groq, ...)
export GREENTIC_LLM_PROVIDER=anthropic
export GREENTIC_LLM_API_KEY=sk-...
export GREENTIC_LLM_MODEL=claude-sonnet-4-6

# New definition
greentic-operala prompt --sorla sorla.yaml \
  "Set up rent payment reconciliation from bank transactions"

# Update an existing definition (requires an LLM)
greentic-operala prompt --sorla sorla.yaml \
  --existing answers.json \
  "raise the amount tolerance to 5"
# → prints a field-level diff, writes answers.updated.json (--in-place to overwrite)

# Force the deterministic path
greentic-operala prompt --no-llm --sorla sorla.yaml "..."
```

Flags `--llm-provider` / `--llm-model` override the env vars. The LLM binds only to identifiers present in the SoRLa contract — every output passes a deterministic validation gate, and ambiguity surfaces as the same `follow-up required:` errors as the keyword path. Provider support comes from the [`greentic-llm`](https://github.com/greenticai/greentic-llm) crate.

## Run With OperaX

After `greentic-operax` is installed on `PATH`, dry-run the generated pack:

```bash
greentic-operax run \
  target/operala/tenancy_rent_reconciliation/tenancy-rent-reconciliation.gtpack \
  --tenant demo-tenant \
  --team property-ops \
  --sorx-url http://localhost:8088 \
  --input extensions/reconciliation/examples/tenancy/banking/one-transaction.json \
  --dry-run \
  --json
```

There is also a small mock SORX server for an end-to-end local demo:

```bash
tmp_dir="$(mktemp -d -t operala-demo.XXXXXX)"
cp extensions/reconciliation/examples/tenancy/sorx-fixtures/initial-state.json "$tmp_dir/sorx-state.json"
echo "[]" > "$tmp_dir/sorx-calls.json"

python3 extensions/reconciliation/examples/tenancy/mock-sorx/server.py \
  --port 18088 \
  --state "$tmp_dir/sorx-state.json" \
  --calls "$tmp_dir/sorx-calls.json" &
mock_pid="$!"

greentic-operax run \
  target/operala/tenancy_rent_reconciliation/tenancy-rent-reconciliation.gtpack \
  --tenant demo-tenant \
  --team property-ops \
  --sorx-url http://127.0.0.1:18088 \
  --input extensions/reconciliation/examples/tenancy/banking/daily-transactions.json \
  --json

kill "$mock_pid"
```

## Useful Commands

Show localized help:

```bash
cargo run --bin greentic-operala -- --help --locale de
cargo run --bin greentic-operala -- wizard --schema --locale nl
```

Run the core checks:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

Run the full local check:

```bash
bash ci/local_check.sh
```

The full local check expects `greentic-operax` to be installed on `PATH`.

## Designer Extension

The `greentic.operala` design extension
(`crates/greentic-operala-designer-extension`) lets the Greentic Designer author
operational behaviour against a built SoRLa system of record, entirely through
the conversational composer UI.

### What It Provides

The extension exposes five tools to the designer host:

| Tool | Purpose |
|------|---------|
| `list_operala_capabilities` | List all built-in OperaLa capabilities with their answers schemas so the designer can surface the wizard form. |
| `generate_operala_answers` | Parse a SoRLa YAML string and a natural-language prompt to produce an `answers.json` draft. One clarification round surfaces as a `{ "follow_up": "question" }` envelope; the designer chat loop shows the question and re-invokes the tool with the operator's reply. |
| `update_operala_answers` | Apply a change instruction (e.g. "raise the tolerance to 5") to existing answers via the host LLM. Returns the updated answers document plus a field-level diff so the designer can show exactly what changed. |
| `validate_operala_answers` | Check answers against the JSON schema and the SoRLa contract's readiness rules. Returns `{ "valid", "issues", "readiness", "patch_proposal" }` — the patch proposal contains additive SoRLa material the designer can present to the operator when required records, events, actions, or endpoints are missing. |
| `generate_handoff_pack` | Build the OperaLa handoff plan in memory and return all pack entries (path + sha256 + base64 content). The designer zips these into a `.gtpack` without writing to disk. |

### State And LLM

The extension is **stateless** — all working state (answers, SoRLa YAML, locale,
tenant/team context) lives in the designer and is passed as arguments on every
call. There is no server-side session.

LLM inference goes through the host via the `operala_composer` role. The designer
must grant that role in the extension's runtime permissions; `generate_operala_answers`
degrades gracefully to the deterministic keyword path when no LLM is present, while
`update_operala_answers` requires a live LLM and returns a descriptive error
otherwise.

### Building The WASM Component

```bash
# Full WASM component (requires cargo-component)
cargo component build -p greentic-operala-designer-extension --release

# Plain cdylib (wasm32-wasip2, no cargo-component tooling required)
cargo build -p greentic-operala-designer-extension \
  --target wasm32-wasip2 --release

# Compile the WASM core without the native CLI (--no-default-features strips the
# `native` feature that pulls in file I/O and the binary entry-point)
cargo check -p greentic-operala-designer-extension \
  --no-default-features --target wasm32-wasip2
```

The crate exposes two feature gates:

| Feature | Default | Purpose |
|---------|---------|---------|
| `native` | yes | Enables the native CLI binary and file-system helpers. Omit this for WASM builds. |
| *(none)* | — | WASM core only; the component shell is always compiled in. |

### Publishing

The extension is published to `store.greentic.cloud` on release tags (`v*.*.*`).
The CI workflow in `.github/workflows/release.yml` builds the WASM component,
packages it with `describe.json`, and uploads the `.gtxpack` to the store. The
`sha256` fields in `describe.json` are stamped by the release pipeline — the
placeholder `000…` values in the source tree are intentional.

## Relationship To Other Greentic Tools

- SoRLa owns system-of-record contracts.
- OperaLa owns operational handoff authoring.
- OperaX runs OperaLa handoff packs against SORX.
- `greentic-pack` builds the `.gtpack` package format.

