#!/usr/bin/env bash
# v0.23.0 Zenoh Service Transport focused smoke。
# 范围：validator 的 zenoh service server parallel gate、Rust/C++ generated
# zenoh service endpoint 接线、self-description / launch manifest service key_expr，
# 以及示例 contract 的 prepare 主路径。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0230-zenoh-service.XXXXXX")"
cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.23.0 zenoh service smoke work dir: %s\n' "$work_dir" >&2
        return
    fi
    rm -rf "$work_dir"
}
trap cleanup EXIT

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

if [[ "${FLOWRT_V0230_ZENOH_SERVICE_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.23.0 zenoh service smoke dry run"
    exit 0
fi

if [[ -n "${FLOWRT_BIN:-}" ]]; then
    flowrt_cmd=("$FLOWRT_BIN")
else
    flowrt_cmd=(cargo run -q -p flowrt-cli --)
    export FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=1
fi

run_flowrt() {
    "${flowrt_cmd[@]}" "$@"
}

echo "v0.23.0 zenoh service smoke: script syntax"
run bash -n scripts/test-v0230-zenoh-service-smoke.sh

echo "v0.23.0 zenoh service smoke: validator zenoh service gates"
run cargo test -p flowrt-validate -j1 -- zenoh_service

echo "v0.23.0 zenoh service smoke: generated Rust/C++ zenoh service snippets"
run cargo test -p flowrt-codegen -j1 -- zenoh_service

echo "v0.23.0 zenoh service smoke: generated zenoh service golden"
run cargo test -p flowrt-codegen -j1 -- golden_zenoh_service

echo "v0.23.0 zenoh service smoke: CLI self-description service key_expr display"
run cargo test -p flowrt-cli -j1 -- self_description_summary_displays_service_endpoints

echo "v0.23.0 zenoh service smoke: example contract check"
run run_flowrt check examples/zenoh_service_demo/rsdl/robot.rsdl

echo "v0.23.0 zenoh service smoke: example prepare + service key_expr"
demo_out="$work_dir/zenoh_service_demo/flowrt"
run run_flowrt prepare examples/zenoh_service_demo/rsdl/robot.rsdl --out-dir "$demo_out"
run grep -qF '"key_expr": "flowrt/service/plan_x5F_client.plan/request"' \
    "$demo_out/launch/launch.json"
run grep -qF '"key_expr": "flowrt/service/plan_x5F_client.plan/request"' \
    "$demo_out/selfdesc/selfdesc.json"

echo "v0.23.0 zenoh service smoke passed"
