#!/usr/bin/env bash
set -euo pipefail

IN="INTEGRITY.sha256"

# 確保在專案根目錄執行
[ -f Cargo.toml ] || { echo "❌ 請在專案根目錄執行（找不到 Cargo.toml）"; exit 1; }
[ -f "$IN" ] || { echo "❌ 找不到 $IN，請先執行 scripts/gen_integrity.sh"; exit 1; }

# ✅ 初始化變數，避免 unbound variable 錯誤
pass=0
fail=0

while IFS= read -r line; do
  line="${line%%$'\r'}"           # 去除 CR
  [ -z "$line" ] && continue

  expected_hash="${line%%  *}"    # 取雙空白前（hash）
  file="${line#*  }"              # 取雙空白後（檔名）

  if [ ! -f "$file" ]; then
    echo "❌ 缺少檔案：$file"
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
echo "通過：$pass，失敗：$fail"
if (( fail > 0 )); then
  exit 1
fi
