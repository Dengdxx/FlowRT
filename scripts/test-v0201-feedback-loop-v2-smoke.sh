#!/usr/bin/env bash
# v0.20.1 Feedback Loops v2 focused smoke。
# 范围：反馈回边 `init`（literal 初值）解析、validator 放宽规则（latest/fifo、init 类型
# 校验、fifo 两端等周期）、以及 codegen 单位延迟 v2 语义——literal 播种与 fifo 按 depth
# 播种 N 份，两语言 golden 锁定输出且真编译（C++ g++ -fsyntax-only、Rust cargo check）。
# runtime 零改动，故无 C++ ctest。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0201-feedback-v2.XXXXXX")"
cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.20.1 feedback v2 smoke work dir: %s\n' "$work_dir" >&2
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

if [[ "${FLOWRT_V0201_FEEDBACK_V2_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.20.1 feedback v2 smoke dry run"
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

echo "v0.20.1 feedback v2 smoke: script syntax"
run bash -n scripts/test-v0201-feedback-loop-v2-smoke.sh

echo "v0.20.1 feedback v2 smoke: RSDL init/depth 解析"
run cargo test -p flowrt-rsdl parses_dataflow_feedback_init_and_depth -j1

echo "v0.20.1 feedback v2 smoke: validator 反馈边规则（latest/fifo/init 类型/等周期）"
run cargo test -p flowrt-validate feedback -j1

echo "v0.20.1 feedback v2 smoke: codegen literal + fifo N 播种断言"
run cargo test -p flowrt-codegen emit_seeds_feedback -j1

echo "v0.20.1 feedback v2 smoke: codegen feedback golden 锁定（两语言）"
run cargo test -p flowrt-codegen golden_feedback_v2_rust -j1
run cargo test -p flowrt-codegen golden_feedback_v2_cpp -j1

echo "v0.20.1 feedback v2 smoke: C++ 生成 feedback v2 shell g++ 语法编译"
cpp_proj="$work_dir/feedback_v2_cpp"
run run_flowrt prepare "$corpus/feedback_v2_cpp/input.rsdl" --out-dir "$cpp_proj/flowrt"
run g++ -std=c++20 -fsyntax-only \
    -I "$cpp_proj/flowrt/cpp/include" \
    -I runtime/cpp/include \
    "$cpp_proj/flowrt/cpp/src/runtime_shell.cpp"

echo "v0.20.1 feedback v2 smoke: Rust 生成 feedback v2 shell cargo check"
rust_proj="$work_dir/feedback_v2_rust"
mkdir -p "$rust_proj/app/rust"
cp "$corpus/feedback_v2_rust/stub/mod.rs" "$rust_proj/app/rust/mod.rs"
run run_flowrt prepare "$corpus/feedback_v2_rust/input.rsdl" --out-dir "$rust_proj/flowrt"
printf '\n[patch.crates-io]\nflowrt = { path = "%s/runtime/rust" }\n' "$repo_root" \
    >> "$rust_proj/flowrt/build/Cargo.toml"
run env CARGO_TARGET_DIR="$rust_proj/cargo-target" \
    cargo check --manifest-path "$rust_proj/flowrt/build/Cargo.toml"

echo "v0.20.1 feedback v2 smoke passed"
