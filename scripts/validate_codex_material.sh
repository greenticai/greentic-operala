#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

run_cmd() {
  echo "+ $*"
  "$@"
}

if command -v jq >/dev/null 2>&1; then
  run_cmd jq empty \
    schemas/operala.answers.schema.json \
    extensions/reconciliation/examples/tenancy/answers.json
else
  echo "jq is required to validate OperaLa JSON schemas and fixtures." >&2
  exit 1
fi

if ! command -v rg >/dev/null 2>&1; then
  echo "rg (ripgrep) is required to scan OperaLa fixtures and schemas." >&2
  exit 1
fi

# scan <pattern> <message>: rg exit 0 => violation, 1 => clean, >=2 => error.
scan() {
  local pattern="$1"
  local message="$2"
  local rc=0
  rg -uuu -n -- "$pattern" extensions schemas || rc=$?
  case "$rc" in
    0)
      echo "$message" >&2
      exit 1
      ;;
    1) ;;
    *)
      echo "ripgrep failed (exit $rc) while scanning fixtures." >&2
      exit "$rc"
      ;;
  esac
}

scan '^sorla: v0\.1|^entities:|^functions:' \
  "Legacy SoRLa v0.1/entities/functions material found in OperaLa fixtures or schemas."
scan 'plaintext secret|provider credential example' \
  "Potential secret-like example material found."

echo "OperaLa schemas and fixtures are internally consistent."
