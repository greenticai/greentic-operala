#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

run_cmd() {
  echo "+ $*"
  "$@"
}

run_cmd bash scripts/validate_codex_material.sh

if ! command -v greentic-operax >/dev/null 2>&1; then
  echo "greentic-operax is required on PATH. Install it with: cargo binstall greentic-operax" >&2
  exit 1
fi

if [[ -d ../greentic-sorla ]]; then
  run_cmd env CARGO_TARGET_DIR=/tmp/greentic-operala-sorla-target \
    cargo run -p greentic-sorla --manifest-path ../greentic-sorla/Cargo.toml -- \
    design validate ../greentic-operala/examples/tenancy/sorla.yaml
else
  echo "[sorla] ../greentic-sorla not present; skipping cross-repo SoRLa fixture validation"
fi

tmp_dir="$(mktemp -d -t operala-customer-pilot.XXXXXX)"
mock_pid=""
cleanup() {
  if [[ -n "$mock_pid" ]]; then
    kill "$mock_pid" >/dev/null 2>&1 || true
  fi
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

run_cmd cargo run --bin greentic-operala -- prompt \
  --locale en-GB \
  --tenant demo-tenant \
  --team property-ops \
  --sorla examples/tenancy/sorla.yaml \
  --output "$tmp_dir/answers.json" \
  "$(cat examples/tenancy/prompt.txt)"

run_cmd cargo run --bin greentic-operala -- wizard --answers "$tmp_dir/answers.json" --locale en-GB

distributor_root="$tmp_dir/distributor"
mkdir -p "$distributor_root"
distributed_ref="store://customer-store/demo-tenant/operala/answers/rent-recon"
distributed_file="$distributor_root/store___customer-store_demo-tenant_operala_answers_rent-recon.json"
cp "$tmp_dir/answers.json" "$distributed_file"
run_cmd env OPERALA_DISTRIBUTOR_ROOT="$distributor_root" \
  cargo run --bin greentic-operala -- wizard --answers "$distributed_ref" --locale en-GB

run_cmd test -f target/operala/tenancy_rent_reconciliation/operala-handoff.json
run_cmd jq -e '(.schema == "greentic.operala.handoff.v1") and (.sorla.source_digest | startswith("sha256:"))' target/operala/tenancy_rent_reconciliation/operala-handoff.json
run_cmd jq -e '(.answers_digest | startswith("sha256:")) and (.sorla_source_digest | startswith("sha256:")) and (.extension_version != "")' target/operala/tenancy_rent_reconciliation/operala.build.lock
run_cmd test -f target/gtpacks/tenancy-rent-reconciliation.gtpack

run_cmd greentic-operax run \
  target/gtpacks/tenancy-rent-reconciliation.gtpack \
  --tenant demo-tenant \
  --team property-ops \
  --sorx-url http://localhost:8088 \
  --input examples/tenancy/banking/one-transaction.json \
  --dry-run \
  --json

cp examples/tenancy/sorx-fixtures/initial-state.json "$tmp_dir/sorx-state.json"
echo "[]" > "$tmp_dir/sorx-calls.json"
python3 examples/tenancy/mock-sorx/server.py \
  --port 18088 \
  --state "$tmp_dir/sorx-state.json" \
  --calls "$tmp_dir/sorx-calls.json" > "$tmp_dir/mock-sorx.log" 2>&1 &
mock_pid="$!"
sleep 0.5

run_cmd greentic-operax run \
  target/gtpacks/tenancy-rent-reconciliation.gtpack \
  --tenant demo-tenant \
  --team property-ops \
  --sorx-url http://127.0.0.1:18088 \
  --input examples/tenancy/banking/daily-transactions.json \
  --json

run_cmd jq -e 'length == 1' "$tmp_dir/sorx-calls.json"
run_cmd jq -e '.[0].action == "record_rent_payment"' "$tmp_dir/sorx-calls.json"
run_cmd jq -e --slurpfile expected examples/tenancy/expected/decisions.batch.json '[.[].action] == $expected[0].expected_actions' "$tmp_dir/sorx-calls.json"
run_cmd jq -e '.Payment | length == 1' "$tmp_dir/sorx-state.json"
run_cmd jq -e '.ReconciliationCase | length == 0' "$tmp_dir/sorx-state.json"
run_cmd jq -e '.RentObligation[] | select(.obligation_id == "rent_001" and .status == "paid")' "$tmp_dir/sorx-state.json"
run_cmd jq -e --slurpfile expected examples/tenancy/expected/decisions.batch.json '(.Payment | length) == $expected[0].expected_state.payments and (.ReconciliationCase | length) == $expected[0].expected_state.reconciliation_cases' "$tmp_dir/sorx-state.json"

run_cmd bash scripts/validate_generated_artifacts.sh target/operala target/gtpacks

echo "Customer pilot fixture smoke passed."
