#!/usr/bin/env bash
# v0.23.3 Global Tick Determinism focused smoke。
# 范围：global_tick 的 IR/validator/codegen/supervisor 基座，以及示例 prepare 生成物。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0233-global-tick.XXXXXX")"
cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.23.3 global tick smoke work dir: %s\n' "$work_dir" >&2
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

if [[ "${FLOWRT_V0233_GLOBAL_TICK_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.23.3 global tick determinism smoke dry run"
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

echo "v0.23.3 global tick smoke: script syntax"
run bash -n scripts/test-v0233-global-tick-determinism-smoke.sh

echo "v0.23.3 global tick smoke: IR / validator / codegen / supervisor"
run cargo test -p flowrt-ir normalizes_global_tick_determinism_profile
run cargo test -p flowrt-validate global_tick
run cargo test -p flowrt-codegen global_tick
run cargo test -p flowrt supervisor::tests::global_tick

echo "v0.23.3 global tick smoke: example prepare"
example="examples/global_tick_demo/rsdl/robot.rsdl"
out_dir="$work_dir/global_tick_demo/flowrt"
run run_flowrt check "$example"
run run_flowrt prepare "$example" --out-dir "$out_dir"

launch="$out_dir/launch/launch.json"
shell="$out_dir/rust/src/runtime_shell.rs"
if ! grep -q '"mode": "global_tick"' "$launch"; then
    echo "ERROR: launch manifest 缺 global_tick determinism" >&2
    exit 1
fi
if ! grep -q 'flowrt_run_tick(grant: flowrt::ExternalTick)' "$shell"; then
    echo "ERROR: Rust generated shell 缺 flowrt_run_tick 外部步进入口" >&2
    exit 1
fi

echo "v0.23.3 global tick determinism smoke passed"
