#!/usr/bin/env bash
# Each renderer is built separately; native Metal never links the WGSL runtime.
set -euo pipefail
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$script_dir/../.."
case "${1:-}" in
    --present)
        env MTL_DEBUG_LAYER=1 cargo test --release --no-default-features --features metal --example native_present -- --include-ignored --test-threads=1
        env MTL_DEBUG_LAYER=1 cargo run --release --no-default-features --features metal --example native_present -- metal --smoke
        ;;
    --retained)
        env MTL_DEBUG_LAYER=1 cargo test --release --test metal_retained_parity -- --include-ignored --test-threads=1 --nocapture
        env MTL_DEBUG_LAYER=1 cargo test --release --no-default-features --features metal --test metal_retained_parity -- --include-ignored --test-threads=1 --nocapture
        ;;
    --examples)
        env MTL_DEBUG_LAYER=1 cargo test --release --test metal_examples_parity complete_example_catalog -- --ignored --test-threads=1 --nocapture
        env MTL_DEBUG_LAYER=1 cargo test --release --no-default-features --features metal --test metal_examples_parity complete_example_catalog -- --ignored --test-threads=1 --nocapture
        ;;
    --svg)
        env MTL_DEBUG_LAYER=1 cargo test --release --test metal_svg_parity -- --ignored --test-threads=1 --nocapture
        env MTL_DEBUG_LAYER=1 cargo test --release --no-default-features --features metal --test metal_svg_parity -- --ignored --test-threads=1 --nocapture
        ;;
    "")
        env MTL_DEBUG_LAYER=1 cargo test --release --test metal_math_reference -- --ignored --test-threads=1
        env MTL_DEBUG_LAYER=1 cargo test --release --test metal_render_parity -- --ignored --test-threads=1
        export TILEINK_NATIVE_GPU="$(python3 -c 'import json; print(json.load(open("target/metal-validation/render/identity.json"))["physical_identity"])')"
        env MTL_DEBUG_LAYER=1 cargo test --release --no-default-features --features metal --lib native::runtime -- --include-ignored --test-threads=1
        env MTL_DEBUG_LAYER=1 cargo test --release --no-default-features --features metal --test metal_render_parity -- --ignored --test-threads=1
        ;;
    *) echo "usage: run_native_metal_tests.sh [--svg|--examples|--retained|--present]" >&2; exit 2 ;;
esac
