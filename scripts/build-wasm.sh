#!/usr/bin/env bash
# Build loquaris-core-wasm and generate its JavaScript bindings into
# web/src/wasm/pkg for the analysis worker.
#
# Needs the wasm32-unknown-unknown target and wasm-bindgen-cli at the same
# version as the wasm-bindgen crate (0.2.130). wasm-opt (binaryen) is used
# when installed. CARGO_TARGET_DIR is respected.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target_dir="${CARGO_TARGET_DIR:-$root/target}"
out_dir="$root/web/src/wasm/pkg"

cd "$root"
cargo build -p loquaris-core-wasm --profile wasm --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir "$out_dir" \
    "$target_dir/wasm32-unknown-unknown/wasm/loquaris_core_wasm.wasm"

wasm="$out_dir/loquaris_core_wasm_bg.wasm"
if command -v wasm-opt >/dev/null 2>&1; then
    wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
        --enable-sign-ext --enable-mutable-globals -o "$wasm.opt" "$wasm"
    mv "$wasm.opt" "$wasm"
else
    echo "wasm-opt not found; skipping size optimization" >&2
fi

size() { wc -c | tr -d ' '; }
echo "$(basename "$wasm"): $(size < "$wasm") bytes," \
    "gzip $(gzip -9 -c "$wasm" | size)," \
    "brotli $(if command -v brotli >/dev/null 2>&1; then brotli -c -q 11 "$wasm" | size; else echo n/a; fi)"
