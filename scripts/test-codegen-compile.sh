#!/usr/bin/env bash
# FlowRT codegen 编译网：对 golden corpus 子集真编译生成 shell，抓字符串断言漏掉的语法/编译错。
#
# 背景：codegen 单测全是字符串断言，不编译生成代码；v0.17.0/v0.18.0 连续两版各漏一处真实编译
# 错（缺 trait import、Rust turbofish 泛型 arity）。本网把「生成工程真编译」纳入开发回路：
# C++ shell 经 g++ -fsyntax-only；Rust crate 经 cargo check（用 repo [patch.crates-io] 解析
# flowrt，无需 full build）。
#
# 定位：取代历史按版本散落的 test-v0XX-*.sh smoke，作分支完整、单一、可扩展的编译门禁。
# case 列表来自 scripts/evidence-matrix.toml，不在本脚本里维护第二份覆盖表。
#
# 复用 golden corpus（crates/flowrt-codegen/tests/golden/<case>/input.rsdl）：同一套契约既被
# golden 等价锁定输出，又在此真编译。Rust case 的用户实现取 <case>/stub/mod.rs（crate:: 路径）。
#
# 覆盖 golden corpus 中已生成 Rust/C++ runtime shell 的 case。覆盖自检会拒绝新增
# runtime_shell snapshot 后忘记纳入证据矩阵，或矩阵残留已经删除的 stale case。
# Rust generated Cargo workspace 先用带重试的 fetch 解析依赖，再用 locked offline check
# 证明生成物编译，避免 crates.io 瞬断和真实编译错误混在同一个失败点里。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
corpus="crates/flowrt-codegen/tests/golden"

tmp_root="${TMPDIR:-$repo_root/target/flowrt-codegen-compile-tmp}"
mkdir -p "$tmp_root"
work_dir="$(mktemp -d "$tmp_root/flowrt-codegen-compile.XXXXXX")"
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
export CARGO_NET_RETRY="${CARGO_NET_RETRY:-10}"
export CARGO_HTTP_TIMEOUT="${CARGO_HTTP_TIMEOUT:-600}"
export CARGO_HTTP_MULTIPLEXING="${CARGO_HTTP_MULTIPLEXING:-false}"

cargo_network_attempts="${FLOWRT_CODEGEN_COMPILE_CARGO_ATTEMPTS:-5}"
retry_cargo_network_command() {
    local label="$1"
    shift
    local attempt=1
    local status=0
    while true; do
        if "$@"; then
            return 0
        else
            status="$?"
        fi
        if ((attempt >= cargo_network_attempts)); then
            return "$status"
        fi
        printf 'warning: %s failed with status %s; retrying (%s/%s)\n' \
            "$label" "$status" "$((attempt + 1))" "$cargo_network_attempts" >&2
        sleep "$((attempt * 5))"
        attempt="$((attempt + 1))"
    done
}

if [[ "${FLOWRT_CODEGEN_COMPILE_RETRY_SELF_TEST:-0}" == "1" ]]; then
    set +e
    retry_output="$(
        retry_cargo_network_command "retry self-test" bash -c 'exit 7' 2>&1
    )"
    retry_status="$?"
    set -e
    if [[ "$retry_status" -ne 7 ]]; then
        printf 'retry self-test expected status 7, got %s\n' "$retry_status" >&2
        exit 1
    fi
    if [[ "$retry_output" != *"status 7"* ]]; then
        printf 'retry self-test did not preserve failed command status in diagnostics\n' >&2
        printf '%s\n' "$retry_output" >&2
        exit 1
    fi
    echo "retry self-test passed"
    exit 0
fi

list_compile_cases() {
    python3 - scripts/evidence-matrix.toml <<'PY'
import sys
from pathlib import Path

import tomllib

doc = tomllib.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
for entry in doc.get("case", []):
    if "syntax_compile" not in entry.get("evidence", []):
        continue
    for language in entry.get("languages", []):
        if language in {"cpp", "rust"}:
            print(language, entry["name"])
PY
}

echo "codegen compile net: script syntax"
run bash -n scripts/test-codegen-compile.sh
run bash -n scripts/check-codegen-compile-coverage.sh
run scripts/check-codegen-compile-coverage.sh

prepare_case() {
    local case="$1" proj="$2"
    local inject_args=()
    if [[ -f "$corpus/$case/inject.toml" ]]; then
        inject_args=(--inject "$corpus/$case/inject.toml")
    fi
    run run_flowrt prepare "$corpus/$case/input.rsdl" --out-dir "$proj/flowrt" "${inject_args[@]}"
}

install_generated_rust_stubs() {
    local case="$1" proj="$2"
    local source_dir="$corpus/$case/expected/app/stubs/rust"
    local dest_dir="$proj/app/rust"
    local runtime_shell="$proj/flowrt/rust/src/runtime_shell.rs"
    mkdir -p "$dest_dir"

    if [[ -f "$corpus/$case/stub/mod.rs" ]]; then
        cp "$corpus/$case/stub/mod.rs" "$dest_dir/mod.rs"
        return
    fi

    if [[ ! -d "$source_dir" ]]; then
        printf 'missing Rust stub source for compile case: %s\n' "$case" >&2
        return 1
    fi

    local stub_files=()
    while IFS= read -r stub; do
        stub_files+=("$stub")
        awk '
            /^pub fn build_app\(\)/ { skip_build_app = 1 }
            !skip_build_app {
                gsub(/flowrt_app::/, "crate::")
                print
            }
        ' "$stub" > "$dest_dir/$(basename "$stub")"
    done < <(find "$source_dir" -maxdepth 1 -type f -name '*.rs' | sort)

    if [[ "${#stub_files[@]}" -eq 0 ]]; then
        printf 'empty Rust stub source for compile case: %s\n' "$case" >&2
        return 1
    fi

    : > "$dest_dir/mod.rs"
    for stub in "${stub_files[@]}"; do
        printf 'pub mod %s;\n' "$(basename "$stub" .rs)" >> "$dest_dir/mod.rs"
    done
    printf '\n' >> "$dest_dir/mod.rs"
    printf 'pub fn build_app() -> crate::App {\n' >> "$dest_dir/mod.rs"
    printf '    crate::App::new(\n' >> "$dest_dir/mod.rs"

    local traits=()
    while IFS= read -r trait_name; do
        traits+=("$trait_name")
    done < <(
        awk '
            /pub fn new\(/ { in_new = 1; next }
            in_new && /\) -> Self/ { exit }
            in_new {
                if ($0 ~ /Box<dyn [A-Za-z0-9_]+/) {
                    line = $0
                    sub(/^.*Box<dyn /, "", line)
                    sub(/[^A-Za-z0-9_].*$/, "", line)
                    print line
                }
            }
        ' "$runtime_shell"
    )

    if [[ "${#traits[@]}" -eq 0 ]]; then
        printf 'failed to derive App::new component order for compile case: %s\n' "$case" >&2
        return 1
    fi

    local trait_name stub module struct_name
    for trait_name in "${traits[@]}"; do
        stub=""
        for candidate in "${stub_files[@]}"; do
            if grep -q "impl flowrt_app::components::${trait_name} for" "$candidate"; then
                stub="$candidate"
                break
            fi
        done
        if [[ -z "$stub" ]]; then
            printf 'failed to find Rust stub implementing %s for compile case: %s\n' "$trait_name" "$case" >&2
            return 1
        fi
        module="$(basename "$stub" .rs)"
        struct_name="$(sed -n -E 's/.*pub struct ([A-Za-z0-9_]+).*/\1/p' "$stub" | head -n1)"
        if [[ -z "$struct_name" ]]; then
            printf 'failed to find Rust stub struct in %s\n' "$stub" >&2
            return 1
        fi
        printf '        Box::new(%s::%s::default()),\n' "$module" "$struct_name" >> "$dest_dir/mod.rs"
    done
    printf '    )\n' >> "$dest_dir/mod.rs"
    printf '}\n' >> "$dest_dir/mod.rs"
}

# C++：plain prepare（契约自带 island mode + boundary）后对生成 shell 做 -fsyntax-only。
# 单文件语法校验即可捕获 turbofish/模板/缺声明类编译错，无需用户实现或 full build。
compile_cpp() {
    local case="$1" proj="$work_dir/$1"
    echo "codegen compile net: [cpp] $case"
    prepare_case "$case" "$proj"
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
    local manifest="$proj/flowrt/build/Cargo.toml"
    local target_dir="$proj/cargo-target"
    echo "codegen compile net: [rust] $case"
    prepare_case "$case" "$proj"
    install_generated_rust_stubs "$case" "$proj"
    printf '\n[patch.crates-io]\nflowrt = { path = "%s/runtime/rust" }\n' \
        "$repo_root" >> "$manifest"
    # 隔离 target-dir：仓库 .cargo/config.toml 把 build.target-dir 钉到共享 target/。
    # 不隔离时，每个生成 crate 都叫 flowrt_app，会命中共享 target 的旧 fingerprint，cargo
    # 跳过重编（"Finished" 无 "Checking"）→ 漏掉生成代码的真实编译错。每 case 独立 target
    # 强制从零编译，真正校验本次生成的 shell。
    run retry_cargo_network_command "cargo fetch [$case]" \
        env CARGO_TARGET_DIR="$target_dir" \
        cargo fetch --manifest-path "$manifest"
    run env CARGO_TARGET_DIR="$target_dir" \
        cargo check --locked --offline --manifest-path "$manifest"
}

while read -r language case_name; do
    case "$language" in
        cpp)
            compile_cpp "$case_name"
            ;;
        rust)
            compile_rust "$case_name"
            ;;
        *)
            printf 'unsupported compile language from evidence matrix: %s\n' "$language" >&2
            exit 1
            ;;
    esac
done < <(list_compile_cases)

echo "codegen compile net passed"
