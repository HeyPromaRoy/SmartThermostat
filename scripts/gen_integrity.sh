#!/usr/bin/env bash
set -euo pipefail

IGNORE_FILE=".integrityignore"
OUT="INTEGRITY.sha256"

# Check prerequisites
command -v openssl >/dev/null 2>&1 || { echo "❌ openssl is required"; exit 1; }
[ -f Cargo.toml ] || { echo "❌ Please run this script in the project root (Cargo.toml not found)"; exit 1; }

# Load ignore rules
RULES=()
if [ -f "$IGNORE_FILE" ]; then
  while IFS= read -r line; do
    line="${line%$'\r'}"
    [[ -z "$line" || "$line" =~ ^# ]] && continue
    RULES+=("$line")
  done < "$IGNORE_FILE"
fi

should_skip() {
  local p="${1#./}"
  for r in "${RULES[@]}"; do
    if [[ "$p" =~ $r ]]; then
      return 0
    fi
  done
  return 1
}

list_files_git() {
  git ls-files -z --cached --others --exclude-standard
}

list_files_find() {
  find . -type f ! -path "./.git/*" ! -name "$OUT" -print0
}

: > "$OUT"

if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  SRC="git"
  LIST_CMD=list_files_git
else
  SRC="find"
  LIST_CMD=list_files_find
fi

KEPT_TMP="$(mktemp)"
trap 'rm -f "$KEPT_TMP"' EXIT

$LIST_CMD | while IFS= read -r -d '' f; do
  f="${f%$'\r'}"
  f="${f#./}"

  if should_skip "$f"; then
    continue
  fi

  line=$(openssl dgst -sha256 -r "$f")
  hash=${line%% *}
  printf '%s  %s\n' "$hash" "$f" >> "$OUT"
  printf '%s\0' "$f" >> "$KEPT_TMP"
done

TOTAL=$($LIST_CMD | tr -cd '\0' | wc -c | awk '{print $1}')
KEPT=$(tr -cd '\0' < "$KEPT_TMP" | wc -c | awk '{print $1}')
TOTAL=$((TOTAL + 0))
KEPT=$((KEPT + 0))
SKIPPED=$(( TOTAL - KEPT ))

echo "---"
echo "Source: $SRC"
echo "Total files: $TOTAL; Added: $KEPT; Skipped: $SKIPPED"
echo "✅ Generated $OUT"
