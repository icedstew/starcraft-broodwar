#!/bin/sh
# Build scbw for the browser into web/pkg (wasm-bindgen glue + scbw_bg.wasm).
# Needs: rustup target add wasm32-unknown-unknown
#        cargo install wasm-bindgen-cli --version <wasm-bindgen version in Cargo.lock>
# Serve: python3 -m http.server -d web 8000, open http://localhost:8000
# Or web/build-and-run.sh: build, serve and start a map with the local install.
set -e
cd "$(dirname "$0")/.."
cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir web/pkg target/wasm32-unknown-unknown/release/scbw.wasm
echo "built web/pkg; serve with: python3 -m http.server -d web 8000"
