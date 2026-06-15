#!/usr/bin/env bash
# v0.17.1 C++ replay parity focused smoke。
# 范围：C++ runtime 原生确定性回放（ReplayDriver / JSONL reader / boundary 激励录制）与 Rust 字节级
# parity，以及生成 C++ 调度走原生回放、flowrt-record 的 JSONL 回放源读写。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

if [[ "${FLOWRT_V0171_CPP_REPLAY_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.17.1 C++ replay parity smoke dry run"
    exit 0
fi

echo "v0.17.1 C++ replay parity smoke: script syntax"
run bash -n scripts/test-v0171-cpp-replay-parity-smoke.sh

echo "v0.17.1 C++ replay parity smoke: JSONL 回放源读写 + MCAP 时间线 reader"
run cargo test -p flowrt-record read_replay_timeline -j1

echo "v0.17.1 C++ replay parity smoke: 生成 C++ 调度走 runtime 原生回放"
run cargo test -p flowrt-codegen cpp_simulated_replay_shell_drops_wall_clock_wake -j1

echo "v0.17.1 C++ replay parity smoke: C++ ReplayDriver/JSONL reader/边界录制 ctest"
build_dir="build/cpp-v0171-replay-smoke"
run cmake -S runtime/cpp -B "$build_dir"
run cmake --build "$build_dir" --target flowrt_replay_smoke flowrt_runtime_introspection_smoke
run ctest --test-dir "$build_dir" -R 'flowrt_replay_smoke|flowrt_runtime_introspection_smoke' \
    --output-on-failure

echo "v0.17.1 C++ replay parity smoke passed"
