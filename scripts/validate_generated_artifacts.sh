#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if ! command -v rg >/dev/null 2>&1; then
  echo "rg (ripgrep) is required to scan generated artifacts." >&2
  exit 1
fi

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

# .gtpack files are zip archives (and ignored by .gitignore), so scan the raw
# targets with -uuu and also scan the extracted contents of every archive.
scan_dirs=()
extract_root=""
cleanup() {
  if [[ -n "$extract_root" ]]; then
    rm -rf "$extract_root"
  fi
}
trap cleanup EXIT

gtpacks=()
while IFS= read -r -d '' pack; do
  gtpacks+=("$pack")
done < <(find "${existing[@]}" -type f -name '*.gtpack' -print0)

if [[ ${#gtpacks[@]} -gt 0 ]]; then
  if ! command -v unzip >/dev/null 2>&1; then
    echo "unzip is required to scan .gtpack contents." >&2
    exit 1
  fi
  extract_root="$(mktemp -d)"
  idx=0
  for pack in "${gtpacks[@]}"; do
    dest="$extract_root/$idx"
    mkdir -p "$dest"
    if ! unzip -q -o "$pack" -d "$dest"; then
      echo "Failed to extract $pack for scanning." >&2
      exit 1
    fi
    scan_dirs+=("$dest")
    idx=$((idx + 1))
  done
fi

# scan <pattern> <message>: rg exit 0 => violation, 1 => clean, >=2 => error.
scan() {
  local pattern="$1"
  local message="$2"
  local rc=0
  rg -uuu -n -- "$pattern" "${existing[@]}" ${scan_dirs[@]+"${scan_dirs[@]}"} || rc=$?
  case "$rc" in
    0)
      echo "$message" >&2
      exit 1
      ;;
    1) ;;
    *)
      echo "ripgrep failed (exit $rc) while scanning generated artifacts." >&2
      exit "$rc"
      ;;
  esac
}

scan 'plaintext secret|provider credential|AKIA[0-9A-Z]{16}|BEGIN (RSA |OPENSSH |EC )?PRIVATE KEY' \
  "Generated artifacts contain secret-like material."
scan 'http://(localhost|127\.0\.0\.1)|https://(localhost|127\.0\.0\.1)' \
  "Generated artifacts contain concrete local SORX URLs."
scan '^sorla: v0\.1|^entities:|^functions:' \
  "Generated artifacts contain legacy SoRLa shapes."

echo "Generated artifacts passed safety scan."
