#!/usr/bin/env bash
set -euo pipefail
export RAYON_NUM_THREADS=1
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
source "$script_dir/quiet_runner.sh"
cd "$repo_root"
log_path="$(new_quiet_run_log tests)"
args=(test --release --no-default-features --features metal)
while (($#)) && [[ "$1" != -- ]]; do
    args+=("$1")
    shift
done
if (($#)); then shift; fi
args+=(-- "$@" --test-threads=1)
write_quiet_progress 'Metal release tests [single-threaded]' 1 1
invoke_quiet_command 'Metal release tests [single-threaded]' "$log_path" '' cargo "${args[@]}"
complete_quiet_run 'release tests' "$log_path"
