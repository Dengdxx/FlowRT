#!/usr/bin/env bash
# v0.20.0 Feedback Loops / Cyclic Graphs focused smoke。
# 范围：RSDL `[[bind.dataflow]]` 的 `feedback` 解析、validator 反馈边规则（无环校验剔除
# feedback 边、latest-only / 同进程 / 必须真正闭环），以及 codegen 单位延迟语义——拓扑断环
# 加启动期零初值播种，两语言 golden 锁定输出且真编译（C++ 经 g++ -fsyntax-only、Rust 经
# cargo check）。runtime 零改动，故无 C++ ctest。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0200-feedback.XXXXXX")"
cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.20.0 feedback smoke work dir: %s\n' "$work_dir" >&2
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

if [[ "${FLOWRT_V0200_FEEDBACK_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.20.0 feedback smoke dry run"
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

echo "v0.20.0 feedback smoke: script syntax"
run bash -n scripts/test-v0200-feedback-loop-smoke.sh

echo "v0.20.0 feedback smoke: RSDL [[bind.dataflow]] feedback 解析"
run cargo test -p flowrt-rsdl parses_dataflow_feedback_flag -j1

echo "v0.20.0 feedback smoke: validator 反馈边规则（无环剔除/latest/同进程/必须闭环）"
run cargo test -p flowrt-validate feedback -j1

echo "v0.20.0 feedback smoke: codegen 拓扑断环 + 零初值播种断言"
run cargo test -p flowrt-codegen emit_seeds_feedback_channel_and_breaks_cycle -j1

echo "v0.20.0 feedback smoke: codegen feedback golden 锁定（两语言）"
run cargo test -p flowrt-codegen golden_feedback_loop_rust -j1
run cargo test -p flowrt-codegen golden_feedback_loop_cpp -j1

echo "v0.20.0 feedback smoke: C++ 生成 feedback shell g++ 语法编译"
cpp_proj="$work_dir/feedback_loop_cpp"
run run_flowrt prepare "$corpus/feedback_loop_cpp/input.rsdl" --out-dir "$cpp_proj/flowrt"
run g++ -std=c++20 -fsyntax-only \
    -I "$cpp_proj/flowrt/cpp/include" \
    -I runtime/cpp/include \
    "$cpp_proj/flowrt/cpp/src/runtime_shell.cpp"

echo "v0.20.0 feedback smoke: Rust 生成 feedback shell cargo check"
rust_proj="$work_dir/feedback_loop_rust"
mkdir -p "$rust_proj/app/rust"
cp "$corpus/feedback_loop_rust/stub/mod.rs" "$rust_proj/app/rust/mod.rs"
run run_flowrt prepare "$corpus/feedback_loop_rust/input.rsdl" --out-dir "$rust_proj/flowrt"
printf '\n[patch.crates-io]\nflowrt = { path = "%s/runtime/rust" }\n' "$repo_root" \
    >> "$rust_proj/flowrt/build/Cargo.toml"
run env CARGO_TARGET_DIR="$rust_proj/cargo-target" \
    cargo check --manifest-path "$rust_proj/flowrt/build/Cargo.toml"

echo "v0.20.0 feedback smoke passed"
