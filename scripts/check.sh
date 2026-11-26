#!/usr/bin/env bash
# Full local verification — the same gates as CI. Run before every commit.
# Usage: scripts/check.sh [--quick]   (--quick skips the browser e2e test)
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n\033[1;34m▶ %s\033[0m\n' "$*"; }

step "Rust: format";         cargo fmt --all -- --check
step "Rust: clippy";         cargo clippy --all-targets -- -D warnings
step "Rust: tests";          cargo test --quiet
step "TS types up to date";  git diff --exit-code -- app/src/lib/engine/generated \
  || { echo "Generated TS types changed: commit them."; exit 1; }
step "Quality benchmark";    cargo run --quiet --release -p openvectorizer-cli -- eval testdata --baseline testdata/baseline.json
step "WASM build";           scripts/build-wasm.sh
cd app
step "App: types";           pnpm --silent check
step "App: lint";            pnpm --silent lint
step "App: unit tests";      pnpm --silent test
step "App: build";           pnpm --silent exec vite build --logLevel warn
if [[ "${1:-}" != "--quick" ]]; then
  step "App: e2e";           pnpm --silent test:e2e
fi
printf '\n\033[1;32m✔ All checks passed\033[0m\n'
