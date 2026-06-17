#!/usr/bin/env bash
# v0.21.1 Instance Fault Isolation + Restart focused smoke。
# 范围：RSDL `[instance.<name>.fault]` 子表/扁平糖解析、IR normalize（默认填充 + 双写拒绝 +
# 非 restart 带参拒绝）、validator 放行 isolate/restart 并拒绝 degrade/退避越界、executor
# suspend/resume 与重启时序确定性、codegen recoverable_instances 与故障/重启 golden、
# C++ runtime executor 隔离 ctest，并对新 golden case 的生成故障 shell 真编译
# （C++ g++ -fsyntax-only、Rust cargo check）。本版改 runtime/codegen。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0211-fault.XXXXXX")"
cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.21.1 fault smoke work dir: %s\n' "$work_dir" >&2
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

if [[ "${FLOWRT_V0211_FAULT_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.21.1 fault smoke dry run"
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

echo "v0.21.1 fault smoke: script syntax"
run bash -n scripts/test-v0211-fault-isolation-restart-smoke.sh

echo "v0.21.1 fault smoke: RSDL fault 子表/扁平糖解析"
run cargo test -p flowrt-rsdl failure_policy -j1

echo "v0.21.1 fault smoke: IR fault normalize（默认填充 + 双写拒绝 + 非 restart 带参拒绝）"
run cargo test -p flowrt-ir -j1 -- fault failure_policy restart

echo "v0.21.1 fault smoke: validator 放行 isolate/restart、拒绝 degrade/越界"
run cargo test -p flowrt-validate rejects_instance_degrade_failure_policy -j1
run cargo test -p flowrt-validate accepts_instance_isolate_and_restart_policies -j1
run cargo test -p flowrt-validate rejects_restart_backoff_out_of_range -j1

echo "v0.21.1 fault smoke: executor suspend/resume 与重启时序确定性"
run cargo test -p flowrt --lib suspend_task -j1
run cargo test -p flowrt --lib suspended_periodic_task_does_not_admit_on_advance -j1
run cargo test -p flowrt --lib restart_ -j1

echo "v0.21.1 fault smoke: codegen recoverable_instances 与故障/重启 golden"
run cargo test -p flowrt-codegen recoverable_instances -j1
run cargo test -p flowrt-codegen golden_instance_fault_restart_rust -j1
run cargo test -p flowrt-codegen golden_instance_fault_restart_cpp -j1

echo "v0.21.1 fault smoke: C++ runtime executor 隔离 ctest"
cpp_build="$work_dir/cpp-build"
run cmake -S runtime/cpp -B "$cpp_build"
run cmake --build "$cpp_build" --target flowrt_runtime_smoke
run ctest --test-dir "$cpp_build" --output-on-failure -R flowrt_runtime_smoke

echo "v0.21.1 fault smoke: C++ 生成故障 shell g++ 语法编译"
cpp_proj="$work_dir/instance_fault_restart_cpp"
run run_flowrt prepare "$corpus/instance_fault_restart_cpp/input.rsdl" --out-dir "$cpp_proj/flowrt"
run g++ -std=c++20 -fsyntax-only \
    -I "$cpp_proj/flowrt/cpp/include" \
    -I runtime/cpp/include \
    "$cpp_proj/flowrt/cpp/src/runtime_shell.cpp"

echo "v0.21.1 fault smoke: Rust 生成故障 shell cargo check"
rust_proj="$work_dir/instance_fault_restart_rust"
mkdir -p "$rust_proj/app/rust"
cp "$corpus/instance_fault_restart_rust/stub/mod.rs" "$rust_proj/app/rust/mod.rs"
run run_flowrt prepare "$corpus/instance_fault_restart_rust/input.rsdl" --out-dir "$rust_proj/flowrt"
printf '\n[patch.crates-io]\nflowrt = { path = "%s/runtime/rust" }\n' "$repo_root" \
    >> "$rust_proj/flowrt/build/Cargo.toml"
run env CARGO_TARGET_DIR="$rust_proj/cargo-target" \
    cargo check --manifest-path "$rust_proj/flowrt/build/Cargo.toml"

echo "v0.21.1 fault smoke passed"
