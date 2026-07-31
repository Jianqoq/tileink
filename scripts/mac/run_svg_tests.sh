#!/usr/bin/env bash
set -euo pipefail

type=all backend=wgpu wgpu_mode=both continue_on_error=0
while (($#)); do
    case "$1" in
        --type|-t) shift; [[ $# -gt 0 ]] || { echo "--type requires a value" >&2; exit 2; }; type="$1" ;;
        --backend|-b) shift; [[ $# -gt 0 ]] || { echo "--backend requires a value" >&2; exit 2; }; backend="$1" ;;
        --wgpu-mode|-m) shift; [[ $# -gt 0 ]] || { echo "--wgpu-mode requires a value" >&2; exit 2; }; wgpu_mode="$1" ;;
        --continue-on-error) continue_on_error=1 ;;
        all|filters|masking|paint-servers|painting|shapes|structure|text) type="$1" ;;
        wgpu) backend="$1" ;;
        native|portable|both) wgpu_mode="$1" ;;
        *) echo "usage: run_svg_tests.sh [--type all|filters|masking|paint-servers|painting|shapes|structure|text] [--backend wgpu] [--wgpu-mode native|portable|both] [--continue-on-error]" >&2; exit 2 ;;
    esac
    shift
done
case "$type" in all|filters|masking|paint-servers|painting|shapes|structure|text) ;; *) echo "unknown type: $type" >&2; exit 2 ;; esac
[[ "$backend" == wgpu ]] || { echo "unknown backend: $backend" >&2; exit 2; }
case "$wgpu_mode" in native|portable|both) ;; *) echo "unknown wgpu mode: $wgpu_mode" >&2; exit 2 ;; esac

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
source "$script_dir/quiet_runner.sh"
cd "$repo_root"
log_path="$(new_quiet_run_log "svg-$type")"
metadata_path="$log_path.metadata.json"
trap 'rm -f "$metadata_path"' EXIT

write_quiet_progress "Read Cargo metadata"
invoke_quiet_command "Cargo metadata" "$log_path" "$metadata_path" cargo metadata --format-version 1 --no-deps
target_dir="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["target_directory"])' "$metadata_path")"
write_quiet_progress "Build SVG fixture renderer"
invoke_quiet_command "Build SVG fixture renderer" "$log_path" "" cargo build --release --example svg_fixture_render
renderer="$target_dir/release/examples/svg_fixture_render"
[[ -x "$renderer" ]] || { echo "Expected renderer executable was not created: $renderer" >&2; exit 1; }

tests_root="$repo_root/src/svg/tests"
if [[ "$type" == all ]]; then
    type_dirs=()
    while IFS= read -r dir; do type_dirs+=("$dir"); done < <(find "$tests_root" -mindepth 1 -maxdepth 1 -type d | sort)
else
    type_dirs=("$tests_root/$type")
fi

failures=()
for i in "${!type_dirs[@]}"; do
    dir="${type_dirs[$i]}"
    [[ -d "$dir" ]] || { echo "SVG test folder does not exist: $dir" >&2; exit 1; }
    if [[ "$wgpu_mode" == native ]]; then
        label="SVG [wgpu] $(basename "$dir")"
        args=("$dir" wgpu --wgpu-mode native)
        failure="[wgpu] $dir"
    else
        label="SVG [wgpu-portable-compare] $(basename "$dir")"
        args=("$dir" wgpu --compare-wgpu-portable)
        failure="[wgpu-portable-compare] $dir"
    fi
    write_quiet_progress "$label" "$((i + 1))" "${#type_dirs[@]}"
    if ! invoke_quiet_command "$label" "$log_path" "" "$renderer" "${args[@]}"; then
        failures+=("$failure")
        ((continue_on_error)) || { echo "SVG render failed: $failure" >&2; exit 1; }
    fi
done

if ((${#failures[@]})); then
    echo "Failures:" >&2
    printf '  %s\n' "${failures[@]}" >&2
    exit 1
fi
complete_quiet_run "SVG tests [$type]" "$log_path"
