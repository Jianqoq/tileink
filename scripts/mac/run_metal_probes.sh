#!/usr/bin/env bash
# M2 acceptance only: maintained MSL compiled by Apple, plus same-GPU WGSL/CPU
# references. This does not certify the full native Canvas/Retained renderer.
set -euo pipefail
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$script_dir/../.."
output="${1:-target/metal-validation/probes}"
mkdir -p "$output"
output="$(cd "$output" && pwd)"
cargo test --release --test metal_toolchain -- --include-ignored --test-threads=1
for repetition in 1 2 3; do
    printf 'Metal probe process %s/3\n' "$repetition"
    env MTL_DEBUG_LAYER=1 TILEINK_METAL_REPORT="$output/run-$repetition.json" \
        cargo test --release --test metal_probes -- --include-ignored --test-threads=1
    [[ -s "$output/run-$repetition.json" ]] || { echo "Missing Metal acceptance report" >&2; exit 1; }
done
printf 'Metal probe reports: %s\n' "$output"
