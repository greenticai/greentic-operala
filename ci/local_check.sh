#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

run_step() {
  echo
  echo "=== $1 ==="
}

run_cmd() {
  echo "+ $*"
  "$@"
}

missing_metadata() {
  local manifest_path="$1"
  local field="$2"
  if ! grep -qE "^[[:space:]]*${field}([[:space:]]*=[[:space:]]*|\\.workspace[[:space:]]*=[[:space:]]*true)" "$manifest_path"; then
    echo "Missing required field ${field} in ${manifest_path}" >&2
    return 1
  fi
}

run_step "Environment and metadata pre-checks"
echo "+ cargo metadata --no-deps --format-version 1 > /tmp/greentic-operala-cargo-metadata.json"
cargo metadata --no-deps --format-version 1 >/tmp/greentic-operala-cargo-metadata.json

if command -v jq >/dev/null 2>&1; then
  while IFS= read -r manifest_path; do
    missing_metadata "$manifest_path" "license"
    missing_metadata "$manifest_path" "repository"
    missing_metadata "$manifest_path" "description"
    missing_metadata "$manifest_path" "readme"
    missing_metadata "$manifest_path" "categories"
    missing_metadata "$manifest_path" "keywords"
  done < <(jq -r '.packages[] | select(.publish == null or (.publish | length) > 0) | .manifest_path' /tmp/greentic-operala-cargo-metadata.json)
else
  echo "[metadata] jq not found; skipping publishable metadata field scan"
fi

run_step "cargo fmt"
run_cmd cargo fmt --all -- --check

run_step "cargo clippy"
run_cmd cargo clippy --all-targets --all-features -- -D warnings

run_step "cargo test"
run_cmd cargo test --all-features

run_step "cargo build"
run_cmd cargo build --all-features

run_step "cargo doc"
run_cmd cargo doc --no-deps --all-features

run_step "Codex planning material"
run_cmd bash scripts/validate_codex_material.sh

run_step "Customer pilot fixture smoke"
run_cmd bash scripts/e2e/customer-pilot-demo-smoke.sh

run_step "Generated artifact safety"
run_cmd bash scripts/validate_generated_artifacts.sh target/operala target/gtpacks

run_step "i18n checks"
run_cmd bash tools/i18n.sh validate

run_step "Coverage policy"
if command -v greentic-dev >/dev/null 2>&1; then
  run_cmd greentic-dev coverage
else
  echo "[coverage] greentic-dev not installed; nightly coverage workflow enforces coverage when tooling is available"
fi

run_step "Validation complete"
echo "All checks passed."
