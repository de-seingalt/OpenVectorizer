#!/usr/bin/env bash
# Build the engine to WebAssembly and generate JS bindings into app/src/lib/engine/pkg.
# Usage: scripts/build-wasm.sh [--dev]
set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE=release
CARGO_FLAGS=(--release)
if [[ "${1:-}" == "--dev" ]]; then PROFILE=debug; CARGO_FLAGS=(); fi

WANT=$(grep -E '^wasm-bindgen = "=' Cargo.toml | sed -E 's/.*"=([0-9.]+)".*/\1/')
HAVE=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)
if [[ "$WANT" != "$HAVE" ]]; then
  echo "wasm-bindgen-cli $WANT required (found '${HAVE:-none}'):" >&2
  echo "  cargo install wasm-bindgen-cli --version $WANT --locked" >&2
  exit 1
fi

cargo build -p openvectorizer-wasm --target wasm32-unknown-unknown "${CARGO_FLAGS[@]}"
wasm-bindgen "target/wasm32-unknown-unknown/$PROFILE/openvectorizer_wasm.wasm" \
  --target web --out-dir app/src/lib/engine/pkg --typescript
echo "WASM: $(du -h app/src/lib/engine/pkg/openvectorizer_wasm_bg.wasm | cut -f1)"
