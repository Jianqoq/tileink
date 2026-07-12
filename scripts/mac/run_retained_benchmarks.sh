#!/usr/bin/env bash
set -euo pipefail

benchmark=all filter= save_baseline= baseline= quick=0
while (($#)); do
    case "$1" in
        --benchmark|-b) shift; [[ $# -gt 0 ]] || { echo "$0: --benchmark requires a value" >&2; exit 2; }; benchmark="$1" ;;
        --filter|-f) shift; [[ $# -gt 0 ]] || { echo "$0: --filter requires a value" >&2; exit 2; }; filter="$1" ;;
        --save-baseline) shift; [[ $# -gt 0 ]] || { echo "$0: --save-baseline requires a value" >&2; exit 2; }; save_baseline="$1" ;;
        --baseline) shift; [[ $# -gt 0 ]] || { echo "$0: --baseline requires a value" >&2; exit 2; }; baseline="$1" ;;
        --quick) quick=1 ;;
        *) echo "usage: run_retained_benchmarks.sh [--benchmark all|scale|dirty-ratio|stress] [--filter FILTER] [--save-baseline NAME|--baseline NAME] [--quick]" >&2; exit 2 ;;
    esac
    shift
done
[[ -z "$save_baseline" || -z "$baseline" ]] || { echo "Use either --save-baseline or --baseline, not both" >&2; exit 2; }
case "$benchmark" in
    all) targets=(retained_scale retained_dirty_ratio retained_stress) ;;
    scale) targets=(retained_scale) ;;
    dirty-ratio) targets=(retained_dirty_ratio) ;;
    stress) targets=(retained_stress) ;;
    *) echo "unknown benchmark: $benchmark" >&2; exit 2 ;;
esac

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$script_dir/quiet_runner.sh"
cd "$script_dir/../.."
log_path="$(new_quiet_run_log retained-benchmarks)"
for i in "${!targets[@]}"; do
    target="${targets[$i]}"
    args=(bench --bench "$target" --)
    [[ -z "$filter" ]] || args+=("$filter")
    [[ -z "$save_baseline" ]] || args+=(--save-baseline "$save_baseline")
    [[ -z "$baseline" ]] || args+=(--baseline "$baseline")
    ((quick == 0)) || args+=(--quick)
    write_quiet_progress "Criterion benchmark [$target]" "$((i + 1))" "${#targets[@]}"
    invoke_quiet_command "Criterion benchmark [$target]" "$log_path" "" cargo "${args[@]}"
done
complete_quiet_run "retained benchmarks" "$log_path"
