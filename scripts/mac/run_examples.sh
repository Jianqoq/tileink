#!/usr/bin/env bash
set -euo pipefail

wgpu_mode="${1:-both}"
case "$wgpu_mode" in native|portable|both) ;; *) echo "usage: run_examples.sh [native|portable|both]" >&2; exit 2 ;; esac
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/quiet_runner.sh"
cd "$script_dir/../.."
log_path="$(new_quiet_run_log examples)"
metadata_path="$log_path.metadata.json"
trap 'rm -f "$metadata_path"' EXIT

write_quiet_progress "Build example [wgpu_examples]" 1 3
invoke_quiet_command "Build example [wgpu_examples]" "$log_path" "" cargo build --release --example wgpu_examples
write_quiet_progress "Read Cargo metadata" 2 3
invoke_quiet_command "Cargo metadata" "$log_path" "$metadata_path" cargo metadata --format-version 1 --no-deps
target_dir="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["target_directory"])' "$metadata_path")"
example="$target_dir/release/examples/wgpu_examples"
[[ -x "$example" ]] || { echo "Built executable not found: $example" >&2; exit 1; }

if [[ "$wgpu_mode" == native ]]; then
    label="Run examples [native]"
    command=(env -u TILEINK_WGPU_COMPARE_PORTABLE TILEINK_WGPU_MODE=native "$example")
else
    label="Run examples [native + portable pixel compare]"
    command=(env TILEINK_WGPU_MODE=native TILEINK_WGPU_COMPARE_PORTABLE=1 "$example")
fi
write_quiet_progress "$label" 3 3
invoke_quiet_command "$label" "$log_path" "" "${command[@]}"
complete_quiet_run "examples; outputs are in examples/wgpu/out" "$log_path"
