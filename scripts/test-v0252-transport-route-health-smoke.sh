#!/usr/bin/env bash
# v0.25.2 transport route health focused smoke。
# 覆盖 transport publish 失败到 route drop/backpressure/overflow counters 的投影，
# 以及 Rust/C++ generated transport dataflow shell 的 route health 接线。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

if [[ "${FLOWRT_V0252_TRANSPORT_ROUTE_HEALTH_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.25.2 transport route health smoke dry run"
    exit 0
fi

echo "v0.25.2 transport route health smoke: script syntax"
run bash -n scripts/test-v0252-transport-route-health-smoke.sh

echo "v0.25.2 transport route health smoke: Rust introspection counters"
run cargo test -p flowrt route_transport_error_updates_policy_counter_and_backend_error -j1

echo "v0.25.2 transport route health smoke: transport codegen route facts"
run cargo test -p flowrt-codegen backend_route_health_is_recorded -j1

echo "v0.25.2 transport route health smoke: transport dataflow golden"
run cargo test -p flowrt-codegen -j1 -- golden_bounded_channel_iox2 golden_cross_process_feedback

echo "v0.25.2 transport route health smoke: C++ introspection counters"
run cmake -S runtime/cpp -B build/cpp-v0252-route-health
run cmake --build build/cpp-v0252-route-health --target flowrt_runtime_introspection_smoke
run build/cpp-v0252-route-health/flowrt_runtime_introspection_smoke

echo "v0.25.2 transport route health smoke passed"
