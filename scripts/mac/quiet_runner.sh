#!/usr/bin/env bash

# Shared runner for macOS entrypoints. Keeping command output in a log makes long
# release and rendering runs readable while preserving actionable failure details.
new_quiet_run_log() {
    local name="${1//[^A-Za-z0-9_.-]/-}"
    mktemp "${TMPDIR:-/tmp}/tileink-${name}-$(date +%Y%m%d-%H%M%S)-XXXXXX"
}

write_quiet_progress() {
    local label="$1" step="${2:-}" total="${3:-}"
    if [[ -n "$step" && -n "$total" ]]; then
        printf '[%s/%s] %s\n' "$step" "$total" "$label"
    else
        printf '[..] %s\n' "$label"
    fi
}

write_quiet_failure() {
    local label="$1" log_path="$2"
    printf '[failed] %s\nLog: %s\n---- last 80 log lines ----\n' "$label" "$log_path" >&2
    tail -n 80 "$log_path" >&2 || true
}

# Usage: invoke_quiet_command LABEL LOG [STDOUT_FILE] COMMAND [ARGS...]
invoke_quiet_command() {
    local label="$1" log_path="$2" stdout_path="$3"
    shift 3
    printf '\n===== %s =====\n' "$label" >>"$log_path"
    if [[ -n "$stdout_path" ]]; then
        if "$@" >"$stdout_path" 2>>"$log_path"; then return 0; else exit_code=$?; fi
    elif "$@" >>"$log_path" 2>&1; then
        return 0
    else
        exit_code=$?
    fi
    write_quiet_failure "$label" "$log_path"
    return "$exit_code"
}

complete_quiet_run() {
    printf '[done] %s\nLog: %s\n' "$1" "$2"
}
