#!/usr/bin/env bash
# v0.22.0 Deterministic Fault Injection + Determinism Verification focused smoke。
# 范围：故障注入场景的 IR overlay/normalize、validator 门（scheduled-only、单进程、需 boundary
# input、canonical）、CLI --inject 解析与接线、codegen 两语言注入门 + golden 锁定，并对注入 golden
# case 的生成 shell 真编译（C++ g++ -fsyntax-only、Rust cargo check）。
#
# determinism 验证策略：注入门是纯调用计数驱动（同输入 → 同注入点），其字节由 golden 锁定；底层
# record→replay / executor 确定性已由 v0.17/v0.18 内核测试证明，注入在其上确定性叠加。故本 smoke
# 经「golden 锁定确定性注入门 + 真编译」验证，不另做 CLI MCAP 往返（与既有回放 smoke 一致）。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0220-faultinj.XXXXXX")"
cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.22.0 fault injection smoke work dir: %s\n' "$work_dir" >&2
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

if [[ "${FLOWRT_V0220_FAULTINJ_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.22.0 fault injection smoke dry run"
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

echo "v0.22.0 fault injection smoke: script syntax"
run bash -n scripts/test-v0220-fault-injection-determinism-smoke.sh

echo "v0.22.0 fault injection smoke: IR overlay 解析与 canonical"
run cargo test -p flowrt-ir fault_injection -j1

echo "v0.22.0 fault injection smoke: validator 门（scheduled-only / 单进程 / boundary / canonical）"
run cargo test -p flowrt-validate fault_injection -j1

echo "v0.22.0 fault injection smoke: CLI --inject 解析与接线"
run cargo test -p flowrt-cli fault_injection -j1

echo "v0.22.0 fault injection smoke: codegen 注入门 + golden 锁定"
run cargo test -p flowrt-codegen -j1 -- tests::fault_injection golden_fault_injection

echo "v0.22.0 fault injection smoke: C++ 生成注入 shell g++ 语法编译"
cpp_proj="$work_dir/fault_injection_restart_cpp"
run run_flowrt prepare "$corpus/fault_injection_restart_cpp/input.rsdl" \
    --out-dir "$cpp_proj/flowrt" \
    --inject "$corpus/fault_injection_restart_cpp/inject.toml"
run g++ -std=c++20 -fsyntax-only \
    -I "$cpp_proj/flowrt/cpp/include" \
    -I runtime/cpp/include \
    "$cpp_proj/flowrt/cpp/src/runtime_shell.cpp"

echo "v0.22.0 fault injection smoke: Rust 生成注入 shell cargo check"
rust_proj="$work_dir/fault_injection_restart_rust"
mkdir -p "$rust_proj/app/rust"
cp "$corpus/fault_injection_restart_rust/stub/mod.rs" "$rust_proj/app/rust/mod.rs"
run run_flowrt prepare "$corpus/fault_injection_restart_rust/input.rsdl" \
    --out-dir "$rust_proj/flowrt" \
    --inject "$corpus/fault_injection_restart_rust/inject.toml"
printf '\n[patch.crates-io]\nflowrt = { path = "%s/runtime/rust" }\n' "$repo_root" \
    >> "$rust_proj/flowrt/build/Cargo.toml"
run env CARGO_TARGET_DIR="$rust_proj/cargo-target" \
    cargo check --manifest-path "$rust_proj/flowrt/build/Cargo.toml"

echo "v0.22.0 fault injection smoke passed"
