#!/bin/sh
# Browser build of the native game into web/dist-web (served by GitHub Pages).
# Needs the wasm32-unknown-unknown target and wasm-bindgen-cli 0.2.130.
set -e
D=$(cd "$(dirname "$0")" && pwd)
cd "$D/../rust" && cargo build --profile web --lib --target wasm32-unknown-unknown
python3 "$D/build.py"
wasm-bindgen --target web --no-typescript --out-dir "$D/dist-web" "$D/../rust/target/wasm32-unknown-unknown/web/fogwood_core.wasm"
ls -l "$D/dist-web"
