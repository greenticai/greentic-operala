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
    examples/tenancy/answers.json
else
  echo "jq is required to validate OperaLa JSON schemas and fixtures." >&2
  exit 1
fi

if rg -n '^sorla: v0\.1|^entities:|^functions:' examples schemas; then
  echo "Legacy SoRLa v0.1/entities/functions material found in OperaLa fixtures or schemas." >&2
  exit 1
fi

if rg -n 'plaintext secret|provider credential example' examples schemas; then
  echo "Potential secret-like example material found." >&2
  exit 1
fi

echo "OperaLa schemas and fixtures are internally consistent."
