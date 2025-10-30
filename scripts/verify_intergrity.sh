#!/usr/bin/env bash
set -euo pipefail

IN="INTEGRITY.sha256"

# Excute this under root directory of the project
[ -f Cargo.toml ] || { echo "❌ Please excute this under root direcory(can't find Cargo.toml)"; exit 1; }
[ -f "$IN" ] || { echo "❌ Can't find $IN, please excute scripts/gen_integrity.sh"; exit 1; }

# Initial
pass=0
fail=0

while IFS= read -r line; do
  line="${line%%$'\r'}"           # remove CR
  [ -z "$line" ] && continue

  expected_hash="${line%%  *}"    # hash
  file="${line#*  }"              # file name

  if [ ! -f "$file" ]; then
    echo "❌ Missing file: $file"
    ((fail++))
    continue
  fi

  got="$(openssl dgst -sha256 -r "$file" | awk '{print $1}')"
  if [ "$got" = "$expected_hash" ]; then
    echo "✅ OK  $file"
    ((pass++))
  else
    echo "❌ MISMATCH  $file"
    ((fail++))
  fi
done < "$IN"

echo "---"
echo "PASS: $pass, FAIL: $fail"
if (( fail > 0 )); then
  exit 1
fi
