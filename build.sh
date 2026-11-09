#!/usr/bin/env bash
# Reproducible contract build without cargo-near. `--cfg near` is required so near-sdk
# routes env calls (including panic messages) to the NEAR host instead of local stubs.
set -euo pipefail
cd "$(dirname "$0")/contract"
RUSTFLAGS="--cfg near -C target-cpu=mvp -C link-arg=-s" \
  cargo build --target wasm32-unknown-unknown --release
mkdir -p ../out && cp target/wasm32-unknown-unknown/release/leash.wasm ../out/leash.wasm
sha256sum ../out/leash.wasm
