#!/usr/bin/env bash
# v0.21.3 Graph Health Aggregation + Controlled Stop focused smoke。
# 范围：RSDL [graph].health.on_faulted 解析与重复拒绝、IR normalize 图级反应枚举、
# validator 放行 stop 并拒绝无 isolate/restart 的 stop、codegen recoverable + 受控停机
# golden，并对新 golden case 的生成 shell 真编译（C++ g++ -fsyntax-only、Rust cargo check）。
# 本版除 codegen/validate 外还改 runtime introspection（graph_health 聚合观测），故含
# C++ lifecycle ctest。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0213-graph-health.XXXXXX")"
cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.21.3 graph health smoke work dir: %s\n' "$work_dir" >&2
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

if [[ "${FLOWRT_V0213_GRAPH_HEALTH_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.21.3 graph health smoke dry run"
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

echo "v0.21.3 graph health smoke: script syntax"
run bash -n scripts/test-v0213-graph-health-smoke.sh

echo "v0.21.3 graph health smoke: RSDL [graph].health 解析与重复拒绝"
run cargo test -p flowrt-rsdl graph_health -j1

echo "v0.21.3 graph health smoke: IR normalize 图级反应枚举"
run cargo test -p flowrt-ir graph_health -j1

echo "v0.21.3 graph health smoke: validator 放行/拒绝受控停机策略"
run cargo test -p flowrt-validate graph_health -j1

echo "v0.21.3 graph health smoke: codegen recoverable 与受控停机 golden"
run cargo test -p flowrt-codegen -j1 -- recoverable_instances golden_graph_health_stop

echo "v0.21.3 graph health smoke: runtime graph_health 聚合观测"
run cargo test -p flowrt --lib -j1 -- introspection::tests::records_instance_lifecycle_state_and_derives_diagnostic introspection::tests::health_fields_serialize_roundtrip

echo "v0.21.3 graph health smoke: C++ 生成受控停机 shell g++ 语法编译"
cpp_proj="$work_dir/graph_health_stop_cpp"
run run_flowrt prepare "$corpus/graph_health_stop_cpp/input.rsdl" --out-dir "$cpp_proj/flowrt"
run g++ -std=c++20 -fsyntax-only \
    -I "$cpp_proj/flowrt/cpp/include" \
    -I runtime/cpp/include \
    "$cpp_proj/flowrt/cpp/src/runtime_shell.cpp"

echo "v0.21.3 graph health smoke: C++ runtime lifecycle ctest（图级聚合）"
run cmake -S runtime/cpp -B "$work_dir/build-cpp" -DCMAKE_BUILD_TYPE=Debug
run cmake --build "$work_dir/build-cpp" --target flowrt_lifecycle_smoke
run ctest --test-dir "$work_dir/build-cpp" -R flowrt_lifecycle_smoke --output-on-failure

echo "v0.21.3 graph health smoke: Rust 生成受控停机 shell cargo check"
rust_proj="$work_dir/graph_health_stop_rust"
mkdir -p "$rust_proj/app/rust"
cp "$corpus/graph_health_stop_rust/stub/mod.rs" "$rust_proj/app/rust/mod.rs"
run run_flowrt prepare "$corpus/graph_health_stop_rust/input.rsdl" --out-dir "$rust_proj/flowrt"
printf '\n[patch.crates-io]\nflowrt = { path = "%s/runtime/rust" }\n' "$repo_root" \
    >> "$rust_proj/flowrt/build/Cargo.toml"
run env CARGO_TARGET_DIR="$rust_proj/cargo-target" \
    cargo check --manifest-path "$rust_proj/flowrt/build/Cargo.toml"

echo "v0.21.3 graph health smoke passed"
