#!/usr/bin/env bash
# v0.15.2 scheduler clock focused smoke。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

if [[ "${FLOWRT_V0152_SCHEDULER_CLOCK_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.15.2 scheduler clock smoke dry run"
    exit 0
fi

echo "v0.15.2 scheduler clock smoke: script syntax"
run bash -n \
    scripts/test-v0152-scheduler-clock-smoke.sh \
    scripts/release-readiness/v0152-scheduler-clock.sh

echo "v0.15.2 scheduler clock smoke: realtime Rust scheduler clock"
run cargo test -p flowrt-codegen rust_shell_builds_scheduler_v2_task_plan_and_wakes_on_input_revision -j1

echo "v0.15.2 scheduler clock smoke: realtime C++ scheduler clock"
run cargo test -p flowrt-codegen cpp_shell_builds_scheduler_v2_task_plan_and_wakes_on_input_revision -j1

echo "v0.15.2 scheduler clock smoke: temporary island simulated replay clock"
run cargo test -p flowrt-codegen launch_manifest_and_selfdesc_expose_temporary_island_artifact_metadata -j1

echo "v0.15.2 scheduler clock smoke passed"
