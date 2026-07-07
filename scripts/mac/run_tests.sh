#!/usr/bin/env bash
set -euo pipefail

wgpu_mode="both"
cargo_args=()

while (($#)); do
    case "$1" in
        --wgpu-mode)
            shift
            [[ $# -gt 0 ]] || { echo "--wgpu-mode requires a value" >&2; exit 2; }
            wgpu_mode="$1"
            ;;
        native|portable|both)
            wgpu_mode="$1"
            ;;
        --)
            shift
            cargo_args+=("$@")
            break
            ;;
        *)
            cargo_args+=("$1")
            ;;
    esac
    shift
done

if [[ "$wgpu_mode" != "native" && "$wgpu_mode" != "portable" && "$wgpu_mode" != "both" ]]; then
    echo "usage: run_tests.sh [native|portable|both] [-- cargo test args...]" >&2
    exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

cd "$repo_root"

if [[ "$wgpu_mode" == "both" ]]; then
    modes=(native portable)
else
    modes=("$wgpu_mode")
fi

for mode in "${modes[@]}"; do
    echo "Running release tests [$mode]"
    TILEINK_RUN_WGPU_TESTS=1 TILEINK_WGPU_MODE="$mode" cargo test --release "${cargo_args[@]}"
done
