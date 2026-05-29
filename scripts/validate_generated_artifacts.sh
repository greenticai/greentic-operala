#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

targets=("$@")
if [[ ${#targets[@]} -eq 0 ]]; then
  targets=(target/operala target/gtpacks)
fi

existing=()
for target in "${targets[@]}"; do
  if [[ -e "$target" ]]; then
    existing+=("$target")
  fi
done

if [[ ${#existing[@]} -eq 0 ]]; then
  echo "No generated artifacts found to scan."
  exit 0
fi

if rg -n 'plaintext secret|provider credential|AKIA[0-9A-Z]{16}|BEGIN (RSA |OPENSSH |EC )?PRIVATE KEY' "${existing[@]}"; then
  echo "Generated artifacts contain secret-like material." >&2
  exit 1
fi

if rg -n 'http://(localhost|127\.0\.0\.1)|https://(localhost|127\.0\.0\.1)' "${existing[@]}"; then
  echo "Generated artifacts contain concrete local SORX URLs." >&2
  exit 1
fi

if rg -n '^sorla: v0\.1|^entities:|^functions:' "${existing[@]}"; then
  echo "Generated artifacts contain legacy SoRLa shapes." >&2
  exit 1
fi

echo "Generated artifacts passed safety scan."
