#!/usr/bin/env bash
# v0.21.4 Cross-Process Feedback Loops focused smoke。
# 范围：validator 放行跨进程 latest feedback / 拒绝跨进程 fifo feedback、codegen 跨进程
# feedback golden，并对生成 shell 断言「source 进程启动期播 init 过 transport」（消费进程不播）。
# 跨进程 feedback 走 zenoh transport，编译需 zenoh SDK，故本 smoke 不做 compile-net（与既有
# inproc-only 编译网一致），改以生成文本断言验证接线。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0214-xproc-feedback.XXXXXX")"
cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.21.4 cross-process feedback smoke work dir: %s\n' "$work_dir" >&2
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

if [[ "${FLOWRT_V0214_XPROC_FEEDBACK_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.21.4 cross-process feedback smoke dry run"
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

export FLOWRT_CACHE_DIR="${FLOWRT_CACHE_DIR:-$work_dir/flowrt-cache}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
corpus="crates/flowrt-codegen/tests/golden"

echo "v0.21.4 cross-process feedback smoke: script syntax"
run bash -n scripts/test-v0214-cross-process-feedback-smoke.sh

echo "v0.21.4 cross-process feedback smoke: validator 放行/拒绝跨进程 feedback"
run cargo test -p flowrt-validate -j1 -- \
    accepts_cross_process_latest_feedback rejects_cross_process_fifo_feedback

echo "v0.21.4 cross-process feedback smoke: codegen 跨进程 feedback golden"
run cargo test -p flowrt-codegen -j1 -- golden_cross_process_feedback

echo "v0.21.4 cross-process feedback smoke: 生成 shell 验证 source 进程播种过 transport"
proj="$work_dir/cross_process_feedback_rust"
run run_flowrt prepare "$corpus/cross_process_feedback_rust/input.rsdl" --out-dir "$proj/flowrt"
shell="$proj/flowrt/rust/src/runtime_shell.rs"

# 源进程（plant_proc，bind_1=plant.state 的 source）启动期对 zenoh channel 播 init（时间戳 0）。
if ! grep -q "publish_at(State { x: 0f64 }, 0)" "$shell"; then
    echo "ERROR: 跨进程 feedback 源端 init 播种缺失" >&2
    exit 1
fi
# 消费进程（ctrl_proc）不得播种 bind_1（消费端经 transport 接收，不本地播种）。
ctrl_body="$(awk '/fn run_process_ctrl_proc/{f=1} f{print} /fn run_process_plant_proc/{f=0}' "$shell")"
if grep -q "publish_at(State { x: 0f64 }, 0)" <<<"$ctrl_body"; then
    echo "ERROR: 消费进程不应本地播种跨进程 feedback init" >&2
    exit 1
fi
echo "  ✓ 源进程播种 init、消费进程不播种"

echo "v0.21.4 cross-process feedback smoke passed"
