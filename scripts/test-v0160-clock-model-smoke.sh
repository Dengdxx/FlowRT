#!/usr/bin/env bash
# v0.16.0 clock model & deterministic replay focused smoke。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

if [[ "${FLOWRT_V0160_CLOCK_MODEL_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.16.0 clock model smoke dry run"
    exit 0
fi

echo "v0.16.0 clock model smoke: script syntax"
run bash -n scripts/test-v0160-clock-model-smoke.sh

echo "v0.16.0 clock model smoke: clock source 一等概念（IR canonical JSON）"
run cargo test -p flowrt-ir clock_source_realtime_omitted_and_simulated_replay_roundtrips -j1

echo "v0.16.0 clock model smoke: clock source 派生与 validator 拒绝"
run cargo test -p flowrt-validate clock -j1

echo "v0.16.0 clock model smoke: context.now()/dt() 规范时间入口"
run cargo test -p flowrt context_now_and_dt_read_runtime_clock -j1

echo "v0.16.0 clock model smoke: simulated_replay 调度去除 wall-clock 绑定（Rust）"
run cargo test -p flowrt-codegen \
    launch_manifest_and_selfdesc_expose_temporary_island_artifact_metadata -j1

echo "v0.16.0 clock model smoke: simulated_replay 调度去除 wall-clock 绑定（C++）"
run cargo test -p flowrt-codegen cpp_simulated_replay_shell_drops_wall_clock_wake -j1

echo "v0.16.0 clock model smoke passed"
