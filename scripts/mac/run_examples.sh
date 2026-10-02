#!/usr/bin/env bash
set -euo pipefail
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
source "$script_dir/quiet_runner.sh"
cd "$repo_root"
log_path="$(new_quiet_run_log examples)"
write_quiet_progress 'Build Metal examples' 1 1
invoke_quiet_command 'Build Metal examples' "$log_path" '' cargo build --release --no-default-features --features metal --examples
complete_quiet_run 'Metal examples' "$log_path"
