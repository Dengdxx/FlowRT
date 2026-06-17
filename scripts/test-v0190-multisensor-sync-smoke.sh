#!/usr/bin/env bash
# v0.19.0 Multi-Sensor Synchronization focused smoke。
# 范围：RSDL `[[sync]]` 解析、SyncGroupIr 归一化、validator sync 规则、runtime
# `Synchronizer` 原语跨语言 conformance（Rust 单测 + C++ ctest 对同一 golden 事件序列
# 产出相同同步集），以及 codegen on_synchronized 接线的两语言真编译（golden 锁定输出，
# C++ 经 g++ -fsyntax-only、Rust 经 cargo check）。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0190-sync.XXXXXX")"
cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.19.0 sync smoke work dir: %s\n' "$work_dir" >&2
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

if [[ "${FLOWRT_V0190_SYNC_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.19.0 sync smoke dry run"
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

echo "v0.19.0 sync smoke: script syntax"
run bash -n scripts/test-v0190-multisensor-sync-smoke.sh

echo "v0.19.0 sync smoke: RSDL [[sync]] 解析 + task.sync"
run cargo test -p flowrt-rsdl sync_group -j1
run cargo test -p flowrt-rsdl sync_table -j1

echo "v0.19.0 sync smoke: SyncGroupIr 归一化（canonical + EntityRef）"
run cargo test -p flowrt-ir normalizes_sync_groups_canonically_and_links_task -j1

echo "v0.19.0 sync smoke: validator sync 组规则与防篡改"
run cargo test -p flowrt-validate sync_tests -j1

echo "v0.19.0 sync smoke: Rust Synchronizer 原语 conformance golden 向量"
run cargo test -p flowrt --lib synchronizer -j1

echo "v0.19.0 sync smoke: C++ Synchronizer 原语 conformance（同一 golden 向量）"
build_dir="build/cpp-v0190-sync-smoke"
run cmake -S runtime/cpp -B "$build_dir"
run cmake --build "$build_dir" --target flowrt_synchronizer_smoke
run ctest --test-dir "$build_dir" -R flowrt_synchronizer_smoke --output-on-failure

echo "v0.19.0 sync smoke: codegen on_synchronized golden 锁定（两语言）"
run cargo test -p flowrt-codegen golden_sync_fusion_rust -j1
run cargo test -p flowrt-codegen golden_sync_fusion_cpp -j1

echo "v0.19.0 sync smoke: C++ 生成 sync shell g++ 语法编译"
cpp_proj="$work_dir/sync_fusion_cpp"
run run_flowrt prepare "$corpus/sync_fusion_cpp/input.rsdl" --out-dir "$cpp_proj/flowrt"
run g++ -std=c++20 -fsyntax-only \
    -I "$cpp_proj/flowrt/cpp/include" \
    -I runtime/cpp/include \
    "$cpp_proj/flowrt/cpp/src/runtime_shell.cpp"

echo "v0.19.0 sync smoke: Rust 生成 sync shell cargo check"
rust_proj="$work_dir/sync_fusion_rust"
mkdir -p "$rust_proj/app/rust"
cp "$corpus/sync_fusion_rust/stub/mod.rs" "$rust_proj/app/rust/mod.rs"
run run_flowrt prepare "$corpus/sync_fusion_rust/input.rsdl" --out-dir "$rust_proj/flowrt"
printf '\n[patch.crates-io]\nflowrt = { path = "%s/runtime/rust" }\n' "$repo_root" \
    >> "$rust_proj/flowrt/build/Cargo.toml"
run env CARGO_TARGET_DIR="$rust_proj/cargo-target" \
    cargo check --manifest-path "$rust_proj/flowrt/build/Cargo.toml"

echo "v0.19.0 sync smoke passed"
