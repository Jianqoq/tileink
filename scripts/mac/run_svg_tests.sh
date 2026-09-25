#!/usr/bin/env bash
set -euo pipefail
type=all
continue_on_error=0
while (($#)); do
    case "$1" in
        --type|-t) shift; type="${1:?type required}" ;;
        --continue-on-error) continue_on_error=1 ;;
        *) echo "usage: run_svg_tests.sh [--type all|filters|masking|paint-servers|painting|shapes|structure|text] [--continue-on-error]" >&2; exit 2 ;;
    esac
    shift
done
case "$type" in all|filters|masking|paint-servers|painting|shapes|structure|text) ;; *) exit 2 ;; esac
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
source "$script_dir/quiet_runner.sh"
cd "$repo_root"
log_path="$(new_quiet_run_log "svg-$type")"
metadata_path="$log_path.metadata.json"
trap 'rm -f "$metadata_path"' EXIT
invoke_quiet_command 'Cargo metadata' "$log_path" "$metadata_path" cargo metadata --format-version 1 --no-deps
target_dir="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["target_directory"])' "$metadata_path")"
invoke_quiet_command 'Build SVG fixture renderer' "$log_path" '' cargo build --release --no-default-features --features metal --example svg_fixture_render
renderer="$target_dir/release/examples/svg_fixture_render"
[[ -x "$renderer" ]] || { echo "Missing SVG renderer: $renderer" >&2; exit 1; }
tests_root="$repo_root/src/svg/tests"
if [[ "$type" == all ]]; then
    type_dirs=()
    while IFS= read -r dir; do type_dirs+=("$dir"); done < <(find "$tests_root" -mindepth 1 -maxdepth 1 -type d | sort)
else type_dirs=("$tests_root/$type"); fi
failures=()
for i in "${!type_dirs[@]}"; do
    dir="${type_dirs[$i]}"
    label="SVG [$(basename "$dir")]"
    write_quiet_progress "$label" "$((i + 1))" "${#type_dirs[@]}"
    if ! invoke_quiet_command "$label" "$log_path" '' "$renderer" "$dir"; then
        failures+=("$dir")
        ((continue_on_error)) || exit 1
    fi
done
if ((${#failures[@]})); then printf 'Failed: %s\n' "${failures[*]}" >&2; exit 1; fi
complete_quiet_run "SVG tests [$type]" "$log_path"
