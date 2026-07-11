#!/usr/bin/env bash
# Build the browser wasm bundle for the document-schema.org playground.
#
# Output lands in crates/schematter-wasm/pkg/ as an ES module (`--target web`):
#   schematter.js, schematter_bg.wasm, schematter.d.ts
# Copy schematter.js + schematter_bg.wasm into the website's
# public/playground/ directory to deploy.
set -euo pipefail
cd "$(dirname "$0")/../.."

# getrandom 0.3 (via ahash) has no default backend on wasm; select the JS one.
# Scoped to this build only, so native `cargo build`/`cargo test` keep the OS backend.
export RUSTFLAGS='--cfg getrandom_backend="wasm_js"'

wasm-pack build crates/schematter-wasm \
  --target web \
  --release \
  --out-dir pkg \
  --out-name schematter

echo
echo "Built:"
ls -la crates/schematter-wasm/pkg/
