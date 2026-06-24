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
# 0.27.1 起 fixed-size typed literal 统一生成 sparse overlay seed，不再依赖 struct literal 文本。
python3 - "$shell" <<'PY'
from pathlib import Path
import sys

source = Path(sys.argv[1]).read_text()


def function_body(name: str) -> str:
    marker = f"fn {name}"
    start = source.find(marker)
    if start < 0:
        raise SystemExit(f"ERROR: 生成 shell 缺少函数 {name}")
    brace = source.find("{", start)
    if brace < 0:
        raise SystemExit(f"ERROR: 函数 {name} 缺少函数体")
    depth = 0
    for index in range(brace, len(source)):
        char = source[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return source[brace + 1:index]
    raise SystemExit(f"ERROR: 函数 {name} 函数体未闭合")


def has_bind_1_seed_publish(body: str) -> bool:
    compact = " ".join(body.split())
    return (
        "app.bind_1.lock()" in compact
        and ".publish_at(" in compact
        and "State::default()" in compact
        and ".x = 0f64" in compact
        and "}, 0)" in compact
    )


if not has_bind_1_seed_publish(function_body("run_process_plant_proc")):
    raise SystemExit("ERROR: 跨进程 feedback 源端 init 播种缺失")

if has_bind_1_seed_publish(function_body("run_process_ctrl_proc")):
    raise SystemExit("ERROR: 消费进程不应本地播种跨进程 feedback init")
PY
echo "  ✓ 源进程播种 init、消费进程不播种"

echo "v0.21.4 cross-process feedback smoke passed"
