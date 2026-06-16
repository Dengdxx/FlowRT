#!/usr/bin/env bash
# FlowRT codegen 编译网：对 golden corpus 子集真编译生成 shell，抓字符串断言漏掉的语法/编译错。
#
# 背景：codegen 单测全是字符串断言，不编译生成代码；v0.17.0/v0.18.0 连续两版各漏一处真实编译
# 错（缺 trait import、Rust turbofish 泛型 arity）。本网把「生成工程真编译」纳入开发回路：
# C++ shell 经 g++ -fsyntax-only；Rust crate 经 cargo check（用 repo [patch.crates-io] 解析
# flowrt，无需 full build）。
#
# 定位：取代历史按版本散落的 test-v0XX-*.sh smoke，作分支完整、单一、可扩展的编译门禁——
# 新增 codegen 分支时在此加 case，不再每版新写 snowflake。
#
# 复用 golden corpus（crates/flowrt-codegen/tests/golden/<case>/input.rsdl）：同一套契约既被
# golden 等价锁定输出，又在此真编译。Rust case 的用户实现取 <case>/stub/mod.rs（crate:: 路径）。
#
# v1 覆盖 4 个 island case（两语言 × 普通/sample-time，含 0.18.0 出 bug 的 sample-time 分支）。
# graph/service 等非 island 契约的真编译覆盖待后续扩展（需多组件 stub 与非 island 构建）。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
corpus="crates/flowrt-codegen/tests/golden"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-codegen-compile.XXXXXX")"
cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved codegen compile net work dir: %s\n' "$work_dir" >&2
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

if [[ "${FLOWRT_CODEGEN_COMPILE_DRY_RUN:-0}" == "1" ]]; then
    echo "codegen compile net dry run"
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

echo "codegen compile net: script syntax"
run bash -n scripts/test-codegen-compile.sh

# C++：plain prepare（契约自带 island mode + boundary）后对生成 shell 做 -fsyntax-only。
# 单文件语法校验即可捕获 turbofish/模板/缺声明类编译错，无需用户实现或 full build。
compile_cpp() {
    local case="$1" proj="$work_dir/$1"
    echo "codegen compile net: [cpp] $case"
    run run_flowrt prepare "$corpus/$case/input.rsdl" --out-dir "$proj/flowrt"
    run g++ -std=c++20 -fsyntax-only \
        -I "$proj/flowrt/cpp/include" \
        -I runtime/cpp/include \
        "$proj/flowrt/cpp/src/runtime_shell.cpp"
}

# Rust：out-dir 为 $proj/flowrt，用户实现置于其 sibling $proj/app/rust/mod.rs（生成 lib.rs 经
# #[path = "../../../app/rust/mod.rs"] 内联）。追加 repo [patch] 让 cargo check 从仓库 runtime
# 解析 flowrt，避免 full build。
compile_rust() {
    local case="$1" proj="$work_dir/$1"
    echo "codegen compile net: [rust] $case"
    mkdir -p "$proj/app/rust"
    cp "$corpus/$case/stub/mod.rs" "$proj/app/rust/mod.rs"
    run run_flowrt prepare "$corpus/$case/input.rsdl" --out-dir "$proj/flowrt"
    printf '\n[patch.crates-io]\nflowrt = { path = "%s/runtime/rust" }\n' \
        "$repo_root" >> "$proj/flowrt/build/Cargo.toml"
    # 隔离 target-dir：仓库 .cargo/config.toml 把 build.target-dir 钉到共享 target/。
    # 不隔离时，每个生成 crate 都叫 flowrt_app，会命中共享 target 的旧 fingerprint，cargo
    # 跳过重编（"Finished" 无 "Checking"）→ 漏掉生成代码的真实编译错。每 case 独立 target
    # 强制从零编译，真正校验本次生成的 shell。
    run env CARGO_TARGET_DIR="$proj/cargo-target" \
        cargo check --manifest-path "$proj/flowrt/build/Cargo.toml"
}

compile_cpp island_cpp_onmsg
compile_cpp sensor_event_time_cpp
compile_rust island_rust_onmsg
compile_rust sensor_event_time_rust

echo "codegen compile net passed"
