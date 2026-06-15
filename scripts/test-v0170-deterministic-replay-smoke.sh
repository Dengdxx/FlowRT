#!/usr/bin/env bash
# v0.17.0 deterministic replay (runtime-native) focused smoke。
# 范围限 Rust 侧：runtime 原生确定性回放内核、record→replay 闭合、生成 Rust 调度走回放驱动。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

if [[ "${FLOWRT_V0170_REPLAY_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.17.0 deterministic replay smoke dry run"
    exit 0
fi

echo "v0.17.0 deterministic replay smoke: script syntax"
run bash -n scripts/test-v0170-deterministic-replay-smoke.sh

echo "v0.17.0 deterministic replay smoke: ReplayDriver 逐周期步进 + TimeDriver/SteppedDriver"
run cargo test -p flowrt --lib time_driver -j1

echo "v0.17.0 deterministic replay smoke: MCAP 回放时间线 reader + runtime 装配桥"
run cargo test -p flowrt-record read_replay_timeline -j1
run cargo test -p flowrt --lib replay -j1

echo "v0.17.0 deterministic replay smoke: boundary 激励录制闭合 record→replay"
run cargo test -p flowrt --lib publish_boundary_input_records -j1

echo "v0.17.0 deterministic replay smoke: 生成 Rust 调度走 runtime 原生回放"
run cargo test -p flowrt-codegen \
    launch_manifest_and_selfdesc_expose_temporary_island_artifact_metadata -j1

echo "v0.17.0 deterministic replay smoke passed"
