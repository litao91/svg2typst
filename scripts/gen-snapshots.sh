#!/usr/bin/env bash
# Regenerate the golden CeTZ snapshots used by tests/convert_fixtures.rs.
# Run this after an intentional change to the output format.
set -euo pipefail
cd "$(dirname "$0")/.."

BIN=target/release/svg2cetz
if [ ! -x "$BIN" ]; then
  cargo build --release
fi
mkdir -p tests/snapshots
for name in robot test2 use_symbol; do
  "$BIN" "tests/fixtures/$name.svg" > "tests/snapshots/$name.typ"
  echo "wrote tests/snapshots/$name.typ"
done
