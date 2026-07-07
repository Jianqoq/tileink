#!/usr/bin/env bash
set -euo pipefail

type="all"
backend="both"
wgpu_mode="both"
continue_on_error=0

while (($#)); do
    case "$1" in
        --type|-t)
            shift
            [[ $# -gt 0 ]] || { echo "--type requires a value" >&2; exit 2; }
            type="$1"
            ;;
        --backend|-b)
            shift
            [[ $# -gt 0 ]] || { echo "--backend requires a value" >&2; exit 2; }
            backend="$1"
            ;;
        --wgpu-mode|-m)
            shift
            [[ $# -gt 0 ]] || { echo "--wgpu-mode requires a value" >&2; exit 2; }
            wgpu_mode="$1"
            ;;
        --continue-on-error)
            continue_on_error=1
            ;;
        all|filters|masking|paint-servers|painting|shapes|structure|text)
            type="$1"
            ;;
        both|cpu|wgpu)
            backend="$1"
            ;;
        native|portable)
            wgpu_mode="$1"
            ;;
        *)
            echo "usage: run_svg_tests.sh [--type all|filters|masking|paint-servers|painting|shapes|structure|text] [--backend both|cpu|wgpu] [--wgpu-mode native|portable|both] [--continue-on-error]" >&2
            exit 2
            ;;
    esac
    shift
done

case "$type" in all|filters|masking|paint-servers|painting|shapes|structure|text) ;; *) echo "unknown type: $type" >&2; exit 2 ;; esac
case "$backend" in both|cpu|wgpu) ;; *) echo "unknown backend: $backend" >&2; exit 2 ;; esac
case "$wgpu_mode" in native|portable|both) ;; *) echo "unknown wgpu mode: $wgpu_mode" >&2; exit 2 ;; esac

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
tests_root="$repo_root/src/svg/tests"

cd "$repo_root"

cargo build --release --example svg_fixture_render

target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
if command -v python3 >/dev/null 2>&1; then
    metadata_target_dir="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
    if [[ -n "$metadata_target_dir" ]]; then
        target_dir="$metadata_target_dir"
    fi
fi

example="$target_dir/release/examples/svg_fixture_render"
if [[ ! -x "$example" ]]; then
    echo "Expected renderer executable was not created: $example" >&2
    exit 1
fi

if [[ "$type" == "all" ]]; then
    type_dirs=()
    while IFS= read -r dir; do
        type_dirs+=("$dir")
    done < <(find "$tests_root" -mindepth 1 -maxdepth 1 -type d | sort)
else
    type_dirs=("$tests_root/$type")
fi

failures=()
for dir in "${type_dirs[@]}"; do
    [[ -d "$dir" ]] || { echo "SVG test folder does not exist: $dir" >&2; exit 1; }

    if [[ "$backend" == "both" || "$backend" == "cpu" ]]; then
        echo "[cpu] $dir"
        if ! "$example" "$dir" cpu --wgpu-mode native; then
            failures+=("[cpu] $dir")
            [[ "$continue_on_error" == "1" ]] || break
        fi
    fi

    if [[ "$backend" == "both" || "$backend" == "wgpu" ]]; then
        if [[ "$wgpu_mode" == "native" ]]; then
            label="wgpu"
            args=("$dir" wgpu --wgpu-mode native)
        else
            label="wgpu-portable-compare"
            args=("$dir" wgpu --compare-wgpu-portable)
        fi

        echo "[$label] $dir"
        if ! "$example" "${args[@]}"; then
            failures+=("[$label] $dir")
            [[ "$continue_on_error" == "1" ]] || break
        fi
    fi
done

if ((${#failures[@]})); then
    echo
    echo "Failures:"
    printf '  %s\n' "${failures[@]}"
    exit 1
fi
