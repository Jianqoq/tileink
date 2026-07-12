#!/usr/bin/env bash
set -euo pipefail

wgpu_mode=both
cargo_args=()
while (($#)); do
    case "$1" in
        --wgpu-mode) shift; [[ $# -gt 0 ]] || { echo "--wgpu-mode requires a value" >&2; exit 2; }; wgpu_mode="$1" ;;
        native|portable|both) wgpu_mode="$1" ;;
        --) shift; cargo_args+=("$@"); break ;;
        *) cargo_args+=("$1") ;;
    esac
    shift
done

case "$wgpu_mode" in native|portable|both) ;; *) echo "usage: run_tests.sh [native|portable|both] [-- cargo test args...]" >&2; exit 2 ;; esac
for argument in "${cargo_args[@]}"; do
    [[ "$argument" != --test-threads* ]] || { echo "Test thread count is fixed at 1; do not pass --test-threads" >&2; exit 2; }
done

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
source "$script_dir/quiet_runner.sh"
cd "$repo_root"

[[ "$wgpu_mode" == both ]] && modes=(native portable) || modes=("$wgpu_mode")
log_path="$(new_quiet_run_log tests)"
if ((${#cargo_args[@]})); then total=${#modes[@]}; else total=$((${#modes[@]} * 6)); fi
step=0

run_test() {
    local mode="$1" label="$2" filter="$3"; shift 3
    local args=(test --release)
    [[ -z "$filter" ]] || args+=("$filter")
    args+=(-- "$@" --test-threads=1)
    ((++step))
    write_quiet_progress "WGPU release tests [$mode, $label, single-threaded]" "$step" "$total"
    invoke_quiet_command "WGPU release tests [$mode, $label, single-threaded]" "$log_path" "" \
        env TILEINK_RUN_WGPU_TESTS=1 TILEINK_WGPU_MODE="$mode" cargo "${args[@]}"
}

run_focused_test() {
    local mode="$1"; shift
    local cargo=() harness=() destination=cargo
    for argument in "$@"; do
        if [[ "$argument" == -- && "$destination" == cargo ]]; then destination=harness
        elif [[ "$destination" == cargo ]]; then cargo+=("$argument")
        else harness+=("$argument")
        fi
    done
    local args=(test --release)
    ((${#cargo[@]} == 0)) || args+=("${cargo[@]}")
    args+=(--)
    ((${#harness[@]} == 0)) || args+=("${harness[@]}")
    args+=(--test-threads=1)
    ((++step))
    write_quiet_progress "WGPU release tests [$mode, focused, single-threaded]" "$step" "$total"
    invoke_quiet_command "WGPU release tests [$mode, focused, single-threaded]" "$log_path" "" \
        env TILEINK_RUN_WGPU_TESTS=1 TILEINK_WGPU_MODE="$mode" cargo "${args[@]}"
}

for mode in "${modes[@]}"; do
    start=$SECONDS
    if ((${#cargo_args[@]})); then
        run_focused_test "$mode" "${cargo_args[@]}"
    else
        run_test "$mode" core "" --skip 'svg::tests::' --skip 'wgpu::renderer::tests::'
        run_test "$mode" svg-unit 'svg::tests::'
        run_test "$mode" persistent-renderer 'wgpu::renderer::tests::persistent_'
        run_test "$mode" low-level-renderer 'wgpu::renderer::tests::wgpu_' --skip 'wgpu_renderer_samples_'
        run_test "$mode" renderer-sampling 'wgpu::renderer::tests::wgpu_renderer_samples_'
        run_test "$mode" renderer-misc 'wgpu::renderer::tests::' --skip persistent_ --skip wgpu_
    fi
    printf 'Finished [%s] in %ss\n' "$mode" "$((SECONDS - start))" >>"$log_path"
done

complete_quiet_run "release tests" "$log_path"
