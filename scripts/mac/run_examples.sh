#!/usr/bin/env bash
set -euo pipefail

wgpu_mode="${1:-both}"
if [[ "$wgpu_mode" != "native" && "$wgpu_mode" != "portable" && "$wgpu_mode" != "both" ]]; then
    echo "usage: run_examples.sh [native|portable|both]" >&2
    exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

cd "$repo_root"

echo "Building examples..."
cargo build --release --example wgpu_examples

if [[ "$wgpu_mode" == "native" ]]; then
    echo "Running example: wgpu_examples [native]"
    TILEINK_WGPU_MODE=native cargo run --release --example wgpu_examples
else
    echo "Running example: wgpu_examples [native + portable pixel compare]"
    TILEINK_WGPU_MODE=native TILEINK_WGPU_COMPARE_PORTABLE=1 cargo run --release --example wgpu_examples
fi

echo "All examples finished. Outputs are in examples/wgpu/out."
