#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

export RUSTFLAGS='--cfg getrandom_backend="wasm_js"'

wasm-pack build crates/schematter-wasm \
  --target web \
  --release \
  --out-dir pkg \
  --out-name schematter

echo
echo "Built:"
ls -la crates/schematter-wasm/pkg/
