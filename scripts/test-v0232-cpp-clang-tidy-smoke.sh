#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0232-clang-tidy.XXXXXX")"
cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.23.2 clang-tidy smoke work dir: %s\n' "$work_dir" >&2
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

if [[ "${FLOWRT_V0232_CLANG_TIDY_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.23.2 C++ clang-tidy smoke dry run"
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
clang_tidy_checks='-*,bugprone-unused-raii,bugprone-use-after-move,performance-for-range-copy,performance-move-constructor-init,performance-no-automatic-move,modernize-use-nullptr,modernize-use-override,modernize-use-equals-default,modernize-use-equals-delete,modernize-redundant-void-arg,readability-duplicate-include,readability-misleading-indentation,readability-redundant-access-specifiers,readability-redundant-control-flow,readability-redundant-declaration,readability-redundant-function-ptr-dereference,readability-redundant-member-init,readability-redundant-smartptr-get,readability-redundant-string-cstr,-clang-analyzer-*'

echo "v0.23.2 C++ clang-tidy smoke: script syntax"
run bash -n scripts/test-v0232-cpp-clang-tidy-smoke.sh

echo "v0.23.2 C++ clang-tidy smoke: runtime/cpp"
runtime_build="$work_dir/runtime-cpp"
run cmake -S runtime/cpp -B "$runtime_build" -G Ninja -DCMAKE_EXPORT_COMPILE_COMMANDS=ON
mapfile -t runtime_tus < <(
    python3 - "$runtime_build/compile_commands.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    for entry in sorted(json.load(handle), key=lambda item: item["file"]):
        print(entry["file"])
PY
)
run "$clang_tidy" -p "$runtime_build" --checks="$clang_tidy_checks" --warnings-as-errors='*' \
    -header-filter="$header_filter" "${runtime_tus[@]}"

echo "v0.23.2 C++ clang-tidy smoke: generated cpp_counter_demo shell"
demo="$work_dir/cpp_counter_demo"
mkdir -p "$demo"
cp -R examples/cpp_counter_demo/. "$demo/"
run run_flowrt prepare "$demo/rsdl/robot.rsdl"
generated_build="$demo/flowrt/build/cmake-clang-tidy"
run cmake -S "$demo/flowrt/build" -B "$generated_build" -G Ninja \
    -DFLOWRT_CPP_RUNTIME_DIR="$repo_root/runtime/cpp" \
    -DFLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=ON \
    -DCMAKE_EXPORT_COMPILE_COMMANDS=ON
run "$clang_tidy" -p "$generated_build" --checks="$clang_tidy_checks" --warnings-as-errors='*' \
    -header-filter="$header_filter" "$demo/flowrt/cpp/src/runtime_shell.cpp"

echo "v0.23.2 C++ clang-tidy smoke passed"
