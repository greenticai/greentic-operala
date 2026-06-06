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

## Relationship To Other Greentic Tools

- SoRLa owns system-of-record contracts.
- OperaLa owns operational handoff authoring.
- OperaX runs OperaLa handoff packs against SORX.
- `greentic-pack` builds the `.gtpack` package format.

