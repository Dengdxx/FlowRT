#!/usr/bin/env bash
# FlowRT C++ 静态质量长期门禁：runtime、generated shell 和 ABI/POD 三类 profile 分开执行。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

tmp_root="${TMPDIR:-/tmp}"
work_dir="$(mktemp -d "$tmp_root/flowrt-cpp-static-quality.XXXXXX")"
cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved C++ static quality work dir: %s\n' "$work_dir" >&2
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

if [[ "${FLOWRT_CPP_STATIC_QUALITY_DRY_RUN:-0}" == "1" ]]; then
    echo "C++ static quality dry run"
    exit 0
fi

if [[ -n "${FLOWRT_CLANG_TIDY:-}" ]]; then
    clang_tidy="$FLOWRT_CLANG_TIDY"
elif command -v clang-tidy >/dev/null 2>&1; then
    clang_tidy="$(command -v clang-tidy)"
elif command -v clang-tidy-18 >/dev/null 2>&1; then
    clang_tidy="$(command -v clang-tidy-18)"
else
    printf 'clang-tidy is required; install clang-tidy or set FLOWRT_CLANG_TIDY=/path/to/clang-tidy\n' >&2
    exit 1
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

header_filter='(.*/runtime/cpp/include/flowrt/.*|.*/flowrt/cpp/(include|src)/.*)'
runtime_checks='-*,bugprone-unused-raii,bugprone-use-after-move,performance-for-range-copy,performance-move-constructor-init,performance-no-automatic-move,modernize-use-nullptr,modernize-use-override,modernize-use-equals-default,modernize-use-equals-delete,modernize-redundant-void-arg,readability-duplicate-include,readability-misleading-indentation,readability-redundant-access-specifiers,readability-redundant-control-flow,readability-redundant-declaration,readability-redundant-function-ptr-dereference,readability-redundant-member-init,readability-redundant-smartptr-get,readability-redundant-string-cstr,-clang-analyzer-*'
generated_checks="$runtime_checks"
abi_checks="$runtime_checks"

list_runtime_tus() {
    python3 - "$1/compile_commands.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    for entry in sorted(json.load(handle), key=lambda item: item["file"]):
        print(entry["file"])
PY
}

list_generated_static_quality_cases() {
    python3 - scripts/evidence-matrix.toml <<'PY'
import sys
from pathlib import Path

import tomllib

doc = tomllib.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
for entry in doc.get("case", []):
    if entry.get("cpp_static_quality", False):
        print(entry["name"])
PY
}

echo "C++ static quality: script syntax"
run bash -n scripts/test-cpp-static-quality.sh
run scripts/check-evidence-matrix.sh

echo "C++ static quality: runtime profile"
runtime_build="$work_dir/runtime-cpp"
run cmake -S runtime/cpp -B "$runtime_build" -G Ninja -DCMAKE_EXPORT_COMPILE_COMMANDS=ON
mapfile -t runtime_tus < <(list_runtime_tus "$runtime_build")
run "$clang_tidy" -p "$runtime_build" --checks="$runtime_checks" --warnings-as-errors='*' \
    -header-filter="$header_filter" "${runtime_tus[@]}"

echo "C++ static quality: generated profile"
while IFS= read -r case_name; do
    [[ -n "$case_name" ]] || continue
    proj="$work_dir/generated/$case_name"
    mkdir -p "$proj"
    echo "C++ static quality: generated profile [$case_name]"
    inject_args=()
    if [[ -f "crates/flowrt-codegen/tests/golden/$case_name/inject.toml" ]]; then
        inject_args=(--inject "crates/flowrt-codegen/tests/golden/$case_name/inject.toml")
    fi
    run run_flowrt prepare "crates/flowrt-codegen/tests/golden/$case_name/input.rsdl" \
        --out-dir "$proj/flowrt" "${inject_args[@]}"
    run "$clang_tidy" --checks="$generated_checks" --warnings-as-errors='*' \
        -header-filter="$header_filter" "$proj/flowrt/cpp/src/runtime_shell.cpp" -- \
        -std=c++20 -I "$proj/flowrt/cpp/include" -I "$repo_root/runtime/cpp/include"
done < <(list_generated_static_quality_cases)

echo "C++ static quality: ABI/POD profile"
run "$clang_tidy" -p "$runtime_build" --checks="$abi_checks" --warnings-as-errors='*' \
    -header-filter='(.*/runtime/cpp/include/flowrt/abi\.h|.*/runtime/cpp/tests/runtime_smoke\.cpp)' \
    runtime/cpp/tests/runtime_smoke.cpp

echo "C++ static quality passed"
