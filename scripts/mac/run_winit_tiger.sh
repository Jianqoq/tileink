#!/usr/bin/env bash
set -euo pipefail

build_only=0
for arg in "$@"; do
    case "$arg" in
        --build-only)
            build_only=1
            ;;
        *)
            echo "usage: run_winit_tiger.sh [--build-only]" >&2
            exit 2
            ;;
    esac
done

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

cd "$repo_root"

if [[ "$build_only" == "1" ]]; then
    cargo build --release --example winit_svg_tiger
else
    cargo run --release --example winit_svg_tiger
fi
