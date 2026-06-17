#!/usr/bin/env bash
# v0.21.2 Degrade Data Semantics focused smoke。
# 范围：RSDL failure_policy 解析、IR normalize degrade 枚举、validator 放行 degrade、
# codegen recoverable_instances 收录 degrade 与故障/降级 golden，并对新 golden case 的
# 生成降级 shell 真编译（C++ g++ -fsyntax-only、Rust cargo check）。本版仅改 codegen/validate，
# 不动 runtime/executor，故不含 C++ runtime ctest。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0212-degrade.XXXXXX")"
cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.21.2 degrade smoke work dir: %s\n' "$work_dir" >&2
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

if [[ "${FLOWRT_V0212_DEGRADE_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.21.2 degrade smoke dry run"
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

echo "v0.21.2 degrade smoke: script syntax"
run bash -n scripts/test-v0212-degrade-smoke.sh

echo "v0.21.2 degrade smoke: RSDL failure_policy 解析"
run cargo test -p flowrt-rsdl failure_policy -j1

echo "v0.21.2 degrade smoke: IR normalize degrade 枚举"
run cargo test -p flowrt-ir -j1 -- fault failure_policy

echo "v0.21.2 degrade smoke: validator 放行 degrade"
run cargo test -p flowrt-validate accepts_instance_degrade_failure_policy -j1

echo "v0.21.2 degrade smoke: codegen recoverable_instances 收录 degrade 与降级 golden"
run cargo test -p flowrt-codegen -j1 -- recoverable_instances golden_instance_degrade

echo "v0.21.2 degrade smoke: C++ 生成降级 shell g++ 语法编译"
cpp_proj="$work_dir/instance_degrade_cpp"
run run_flowrt prepare "$corpus/instance_degrade_cpp/input.rsdl" --out-dir "$cpp_proj/flowrt"
run g++ -std=c++20 -fsyntax-only \
    -I "$cpp_proj/flowrt/cpp/include" \
    -I runtime/cpp/include \
    "$cpp_proj/flowrt/cpp/src/runtime_shell.cpp"

echo "v0.21.2 degrade smoke: Rust 生成降级 shell cargo check"
rust_proj="$work_dir/instance_degrade_rust"
mkdir -p "$rust_proj/app/rust"
cp "$corpus/instance_degrade_rust/stub/mod.rs" "$rust_proj/app/rust/mod.rs"
run run_flowrt prepare "$corpus/instance_degrade_rust/input.rsdl" --out-dir "$rust_proj/flowrt"
printf '\n[patch.crates-io]\nflowrt = { path = "%s/runtime/rust" }\n' "$repo_root" \
    >> "$rust_proj/flowrt/build/Cargo.toml"
run env CARGO_TARGET_DIR="$rust_proj/cargo-target" \
    cargo check --manifest-path "$rust_proj/flowrt/build/Cargo.toml"

echo "v0.21.2 degrade smoke passed"
