#!/usr/bin/env bash
set -euo pipefail

build_only=0
for argument in "$@"; do
    [[ "$argument" == --build-only ]] || { echo "usage: run_winit_tiger.sh [--build-only]" >&2; exit 2; }
    build_only=1
done
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/quiet_runner.sh"
cd "$script_dir/../.."
log_path="$(new_quiet_run_log winit-tiger)"
if ((build_only)); then label="Build winit SVG tiger"; command=build; else label="Run winit SVG tiger"; command=run; fi
write_quiet_progress "$label" 1 1
invoke_quiet_command "$label" "$log_path" "" cargo "$command" --release --example winit_svg_tiger
complete_quiet_run "winit SVG tiger" "$log_path"
