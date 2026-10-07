#!/usr/bin/env bash
# Regression gate: convert every fixture to standalone CeTZ and compile it
# with typst. Requires `typst` (and a cached @preview/cetz) on PATH.
set -uo pipefail
cd "$(dirname "$0")/.."

BIN=target/release/svg2cetz
if [ ! -x "$BIN" ]; then
  cargo build --release || exit 1
fi
if ! command -v typst >/dev/null; then
  echo "typst not found on PATH; skipping compile verification" >&2
  exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fail=0
total=0
for svg in tests/fixtures/*.svg; do
  base=$(basename "$svg" .svg)
  total=$((total + 1))
  if ! "$BIN" --standalone "$svg" -o "$work/$base.typ" 2>"$work/$base.conv.err"; then
    echo "CONVERT-FAIL $base"
    sed -n '1,6p' "$work/$base.conv.err"
    fail=1
    continue
  fi
  if typst compile "$work/$base.typ" "$work/$base.pdf" 2>"$work/$base.typ.err"; then
    echo "ok    $base"
  else
    echo "FAIL  $base"
    sed -n '1,8p' "$work/$base.typ.err"
    fail=1
  fi
done

echo "---"
echo "$total fixtures checked"
exit "$fail"
