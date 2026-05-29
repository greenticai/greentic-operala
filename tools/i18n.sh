#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MODE="${1:-all}"
AUTH_MODE="${AUTH_MODE:-auto}"
LOCALE="${LOCALE:-en}"
EN_PATH="${EN_PATH:-all}"
LOCALES_PATH="${LOCALES_PATH:-i18n/locales.json}"
BATCH_SIZE="${I18N_BATCH_SIZE:-200}"
TRANSLATOR_BIN="${TRANSLATOR_BIN:-greentic0i18n-translator}"

auth_mode_supported() {
  if [[ "$AUTH_MODE" == "auto" || "$AUTH_MODE" == "api-key" || "$AUTH_MODE" == "browser" ]]; then
    return 0
  fi
  return 1
}

usage() {
  cat <<'EOF_USAGE'
Usage: tools/i18n.sh [translate|validate|status|all]

Environment overrides:
  EN_PATH=...                      English source file path, or `all` to scan repo i18n/en.json files (default: all)
  LOCALES_PATH=...                 Locale list file path (default: i18n/locales.json)
  AUTH_MODE=auto|api-key|browser    translator auth mode (default: auto)
  LOCALE=en                        CLI locale used for translator output (default: en)
  I18N_BATCH_SIZE=<int>            target translations per batch (default: 200)
  TRANSLATOR_BIN=...               translator binary (default: greentic0i18n-translator)

Modes:
  translate  Translate every discovered en.json to all supported locales in 200-item batches.
  validate   Validate translation files against discovered i18n/en.json files
  status     Show translation status for all locales
  all        Run translate + validate + status

EOF_USAGE
}

log() {
  printf '[i18n] %s\n' "$*"
}

fail() {
  printf '[i18n] error: %s\n' "$*" >&2
  exit 1
}

require_tool() {
  local tool="$1"
  command -v "$tool" >/dev/null 2>&1 || fail "required command not found: ${tool}"
}

read_lines_into_array() {
  local array_name="$1"
  shift

  eval "${array_name}=()"
  local line
  while IFS= read -r line; do
    eval "${array_name}+=(\"\$line\")"
  done < <("$@")
}

ensure_translator() {
  if command -v "$TRANSLATOR_BIN" >/dev/null 2>&1; then
    return
  fi

  if command -v greentic-i18n-translator >/dev/null 2>&1; then
    TRANSLATOR_BIN="greentic-i18n-translator"
    return
  fi

  command -v cargo-binstall >/dev/null 2>&1 \
    || fail "${TRANSLATOR_BIN} not found and cargo-binstall is unavailable"

  log "installing greentic-i18n-translator via cargo-binstall"
  cargo binstall -y greentic-i18n-translator \
    || fail "failed to install greentic-i18n-translator via cargo-binstall"

  if command -v "$TRANSLATOR_BIN" >/dev/null 2>&1; then
    return
  fi
  if command -v greentic-i18n-translator >/dev/null 2>&1; then
    TRANSLATOR_BIN="greentic-i18n-translator"
    return
  fi
  fail "translator is still not on PATH after cargo-binstall"
}

load_locales() {
  require_tool jq
  if [[ ! -f "$LOCALES_PATH" ]]; then
    fail "missing locales file: ${LOCALES_PATH}"
  fi
  jq -r '.[]' "$LOCALES_PATH"
}

ensure_locale_files() {
  local en_file="$1"
  local locale_dir
  locale_dir="$(dirname "$en_file")"
  for lang in "${LOCALE_LIST[@]}"; do
    local locale_file="${locale_dir}/${lang}.json"
    if [[ ! -f "$locale_file" ]]; then
      printf '{\n}\n' > "$locale_file"
      log "created locale file: ${locale_file}"
    fi
  done
}

locale_file_for() {
  local en_file="$1"
  local lang="$2"
  local locale_dir
  locale_dir="$(dirname "$en_file")"
  printf '%s/%s.json\n' "$locale_dir" "$lang"
}

json_equal() {
  local left="$1"
  local right="$2"
  jq -S . "$left" > /tmp/i18n-left.$$.$RANDOM.json
  jq -S . "$right" > /tmp/i18n-right.$$.$RANDOM.json
  cmp -s /tmp/i18n-left.$$.*.json /tmp/i18n-right.$$.*.json
}

locale_is_english_copy() {
  local en_file="$1"
  local locale_file="$2"
  [[ -f "$locale_file" ]] || return 1
  local tmp_left tmp_right
  tmp_left="$(mktemp)"
  tmp_right="$(mktemp)"
  jq -S . "$en_file" > "$tmp_left"
  jq -S . "$locale_file" > "$tmp_right"
  local result=1
  if cmp -s "$tmp_left" "$tmp_right"; then
    result=0
  fi
  rm -f "$tmp_left" "$tmp_right"
  return "$result"
}

prepare_locale_targets_for_translation() {
  local en_file="$1"
  local lang locale_file
  for lang in "${LOCALE_LIST[@]}"; do
    if [[ "$lang" == "en" ]]; then
      continue
    fi
    locale_file="$(locale_file_for "$en_file" "$lang")"
    if locale_is_english_copy "$en_file" "$locale_file"; then
      printf '{\n}\n' > "$locale_file"
      log "reset English-copy locale before translation: ${locale_file}"
    fi
  done
}

merge_batch_results() {
  local en_file="$1"
  local batch_dir="$2"
  local lang locale_file batch_locale_file merged_file
  for lang in "${LOCALE_LIST[@]}"; do
    locale_file="$(locale_file_for "$en_file" "$lang")"
    batch_locale_file="${batch_dir}/${lang}.json"
    if [[ ! -f "$batch_locale_file" ]]; then
      continue
    fi
    merged_file="$(mktemp)"
    jq -s '.[0] * .[1]' "$locale_file" "$batch_locale_file" > "$merged_file"
    mv "$merged_file" "$locale_file"
  done
}

check_for_english_copies() {
  local en_file="$1"
  local fail_on_copy="${2:-0}"
  local lang locale_file found=0
  for lang in "${LOCALE_LIST[@]}"; do
    if [[ "$lang" == "en" ]]; then
      continue
    fi
    locale_file="$(locale_file_for "$en_file" "$lang")"
    if locale_is_english_copy "$en_file" "$locale_file"; then
      printf '[i18n] untranslated-copy: %s matches %s exactly\n' "$locale_file" "$en_file"
      found=1
    fi
  done
  if (( fail_on_copy != 0 && found != 0 )); then
    fail "one or more locale files are still exact English copies for ${en_file}"
  fi
}

load_en_files() {
  if [[ "$EN_PATH" != "all" ]]; then
    if [[ ! -f "$EN_PATH" ]]; then
      fail "missing English source file: ${EN_PATH}"
    fi
    printf '%s\n' "$EN_PATH"
    return
  fi

  find . \
    -path './.git' -prune -o \
    -path './target' -prune -o \
    -type f -path '*/i18n/en.json' -print \
    | LC_ALL=C sort
}

locale_csv() {
  local langs=("$@")
  local IFS=','
  printf '%s\n' "${langs[*]}"
}

split_translate_batch() {
  local start="$1"
  local size="$2"
  local source_file="$3"
  local batch_file="$4"

  jq -s --argjson start "$start" --argjson size "$size" '
    .[0]
    | to_entries
    | .[$start:($start + $size)]
    | from_entries
  ' "$source_file" > "$batch_file"
}

run_translate_batch() {
  local en_file="$1"
  local langs="$2"
  local supports_batch_size support

  if command -v rg >/dev/null 2>&1; then
    support="$("$TRANSLATOR_BIN" --help 2>/dev/null | rg -o -- "--batch-size" || true)"
  else
    support="$("$TRANSLATOR_BIN" --help 2>/dev/null | grep -Eo -- "--batch-size" || true)"
  fi

  if [[ -n "$support" ]]; then
    "$TRANSLATOR_BIN" \
      --locale "$LOCALE" \
      translate --langs "$langs" --en "$en_file" --batch-size "$BATCH_SIZE" --auth-mode "$AUTH_MODE"
  else
    "$TRANSLATOR_BIN" \
      --locale "$LOCALE" \
      translate --langs "$langs" --en "$en_file" --auth-mode "$AUTH_MODE"
  fi
}

run_translate() {
  require_tool jq

  read_lines_into_array LOCALE_LIST load_locales
  read_lines_into_array EN_FILES load_en_files
  if [[ ${#LOCALE_LIST[@]} -eq 0 ]]; then
    fail "no locales found in ${LOCALES_PATH}"
  fi
  if [[ ${#EN_FILES[@]} -eq 0 ]]; then
    fail "no en.json files found"
  fi

  if ! auth_mode_supported; then
    fail "unsupported AUTH_MODE '${AUTH_MODE}'. expected auto|api-key|browser"
  fi

  local locales_csv="$(locale_csv "${LOCALE_LIST[@]}")"
  local batch_size="${BATCH_SIZE}"
  if ! [[ "$batch_size" =~ ^[1-9][0-9]*$ ]]; then
    batch_size=200
  fi

  local en_file
  for en_file in "${EN_FILES[@]}"; do
    ensure_locale_files "$en_file"
    prepare_locale_targets_for_translation "$en_file"
    local keys
    keys=$(jq 'length' "$en_file")

    if (( keys <= 0 )); then
      log "nothing to translate in ${en_file}"
      continue
    fi

    log "translating ${en_file}"
    if (( keys <= batch_size )); then
      run_translate_batch "$en_file" "$locales_csv"
      continue
    fi

    local start=0
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    trap 'rm -rf "$tmp_dir"' RETURN

    while (( start < keys )); do
      local batch_file="$tmp_dir/en.${start}.json"
      split_translate_batch "$start" "$batch_size" "$en_file" "$batch_file"
      run_translate_batch "$batch_file" "$locales_csv"
      merge_batch_results "$en_file" "$tmp_dir"
      start=$(( start + batch_size ))
    done
    check_for_english_copies "$en_file" 0
  done
}

run_validate() {
  require_tool jq

  read_lines_into_array LOCALE_LIST load_locales
  read_lines_into_array EN_FILES load_en_files
  if [[ ${#LOCALE_LIST[@]} -eq 0 ]]; then
    fail "no locales found in ${LOCALES_PATH}"
  fi
  if [[ ${#EN_FILES[@]} -eq 0 ]]; then
    fail "no en.json files found"
  fi

  local locales_csv="$(locale_csv "${LOCALE_LIST[@]}")"
  local en_file
  for en_file in "${EN_FILES[@]}"; do
    log "validating ${en_file}"
    "$TRANSLATOR_BIN" \
      --locale "$LOCALE" \
      validate --langs "$locales_csv" --en "$en_file"
    check_for_english_copies "$en_file" 1
  done
}

run_status() {
  require_tool jq

  read_lines_into_array LOCALE_LIST load_locales
  read_lines_into_array EN_FILES load_en_files
  if [[ ${#LOCALE_LIST[@]} -eq 0 ]]; then
    fail "no locales found in ${LOCALES_PATH}"
  fi
  if [[ ${#EN_FILES[@]} -eq 0 ]]; then
    fail "no en.json files found"
  fi

  local locales_csv="$(locale_csv "${LOCALE_LIST[@]}")"
  local en_file
  for en_file in "${EN_FILES[@]}"; do
    log "status ${en_file}"
    "$TRANSLATOR_BIN" \
      --locale "$LOCALE" \
      status --langs "$locales_csv" --en "$en_file"
    check_for_english_copies "$en_file" 0
  done
}

if [[ "${MODE}" == "-h" || "${MODE}" == "--help" ]]; then
  usage
  exit 0
fi

ensure_translator

case "$MODE" in
  translate)
    run_translate
    ;;
  validate)
    run_validate
    ;;
  status)
    run_status
    ;;
  all)
    run_translate
    run_validate
    run_status
    ;;
  *)
    echo "Unknown mode: $MODE" >&2
    usage
    exit 2
    ;;
esac
