#!/usr/bin/env bash
# v0.21.0 Lifecycle State Machine focused smoke。
# 范围：instance `failure_policy` 解析/IR normalize/validator 仅放行 fail_fast、跨语言
# `LifecycleState` 枚举与 introspection 记录、生成 shell 在生命周期段记录状态、既有 golden
# 锁定记录输出且两语言真编译（C++ g++ -fsyntax-only、Rust cargo check），并跑 C++
# lifecycle ctest（本版改 runtime）。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0210-lifecycle.XXXXXX")"
cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.21.0 lifecycle smoke work dir: %s\n' "$work_dir" >&2
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

if [[ "${FLOWRT_V0210_LIFECYCLE_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.21.0 lifecycle smoke dry run"
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

echo "v0.21.0 lifecycle smoke: script syntax"
run bash -n scripts/test-v0210-lifecycle-state-machine-smoke.sh

echo "v0.21.0 lifecycle smoke: RSDL failure_policy 解析"
run cargo test -p flowrt-rsdl failure_policy -j1

echo "v0.21.0 lifecycle smoke: IR 故障策略 normalize"
run cargo test -p flowrt-ir failure_policy -j1

echo "v0.21.0 lifecycle smoke: validator 仅放行 fail_fast"
run cargo test -p flowrt-validate failure_policy -j1
run cargo test -p flowrt-validate accepts_instance_fail_fast_policy -j1

echo "v0.21.0 lifecycle smoke: runtime LifecycleState 枚举与 introspection 记录"
run cargo test -p flowrt --lib lifecycle:: -j1
run cargo test -p flowrt records_instance_lifecycle_state_and_derives_diagnostic -j1

echo "v0.21.0 lifecycle smoke: codegen 记录断言（两语言）"
run cargo test -p flowrt-codegen lifecycle_states -j1

echo "v0.21.0 lifecycle smoke: golden 锁定记录输出（两语言）"
run cargo test -p flowrt-codegen golden_feedback_v2_rust -j1
run cargo test -p flowrt-codegen golden_feedback_v2_cpp -j1

echo "v0.21.0 lifecycle smoke: C++ lifecycle ctest"
cpp_build="$work_dir/cpp-build"
run cmake -S runtime/cpp -B "$cpp_build"
run cmake --build "$cpp_build" --target flowrt_lifecycle_smoke
run ctest --test-dir "$cpp_build" --output-on-failure -R flowrt_lifecycle_smoke

echo "v0.21.0 lifecycle smoke: C++ 生成 shell g++ 语法编译（含生命周期记录）"
cpp_proj="$work_dir/feedback_v2_cpp"
run run_flowrt prepare "$corpus/feedback_v2_cpp/input.rsdl" --out-dir "$cpp_proj/flowrt"
run g++ -std=c++20 -fsyntax-only \
    -I "$cpp_proj/flowrt/cpp/include" \
    -I runtime/cpp/include \
    "$cpp_proj/flowrt/cpp/src/runtime_shell.cpp"

echo "v0.21.0 lifecycle smoke: Rust 生成 shell cargo check（含生命周期记录）"
rust_proj="$work_dir/feedback_v2_rust"
mkdir -p "$rust_proj/app/rust"
cp "$corpus/feedback_v2_rust/stub/mod.rs" "$rust_proj/app/rust/mod.rs"
run run_flowrt prepare "$corpus/feedback_v2_rust/input.rsdl" --out-dir "$rust_proj/flowrt"
printf '\n[patch.crates-io]\nflowrt = { path = "%s/runtime/rust" }\n' "$repo_root" \
    >> "$rust_proj/flowrt/build/Cargo.toml"
run env CARGO_TARGET_DIR="$rust_proj/cargo-target" \
    cargo check --manifest-path "$rust_proj/flowrt/build/Cargo.toml"

echo "v0.21.0 lifecycle smoke passed"
