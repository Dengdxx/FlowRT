#!/usr/bin/env bash
# v0.23.3 focused smoke。
# 范围：本版本集中收束的 determinism、failover、Operation、record、tracing 和 C ABI 切片。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

if [[ "${FLOWRT_V0233_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.23.3 focused smoke dry run"
    exit 0
fi

echo "v0.23.3 focused smoke: script syntax"
run bash -n scripts/test-v0233-scope-closure-smoke.sh
run bash -n scripts/test-v0233-global-tick-determinism-smoke.sh

echo "v0.23.3 focused smoke: global tick"
run scripts/test-v0233-global-tick-determinism-smoke.sh

echo "v0.23.3 focused smoke: validator"
run cargo test -p flowrt-validate global_tick
run cargo test -p flowrt-validate redundancy
run cargo test -p flowrt-validate injection
run cargo test -p flowrt-validate operation
run cargo test -p flowrt-validate c_v0

echo "v0.23.3 focused smoke: codegen"
run cargo test -p flowrt-codegen global_tick
run cargo test -p flowrt-codegen standby_failover
run cargo test -p flowrt-codegen zenoh_operation
run cargo test -p flowrt-codegen c_params

echo "v0.23.3 focused smoke: runtime"
run cargo test -p flowrt frame_descriptor
run cargo test -p flowrt tracing_exporter
run cargo test -p flowrt abi

echo "v0.23.3 focused smoke passed"
