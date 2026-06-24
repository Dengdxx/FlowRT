#!/usr/bin/env bash
set -euo pipefail

# v0.29.0 入库示例 module layout smoke。

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0290-example-layout.XXXXXX")"

cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.29.0 example layout smoke work dir: %s\n' "$work_dir" >&2
    else
        rm -rf "$work_dir"
    fi
}
trap cleanup EXIT

run() {
    local cmd="$1"
    shift
    printf '+ %q' "$cmd"
    for arg in "$@"; do
        printf ' %q' "$arg"
    done
    printf '\n'
    "$cmd" "$@"
}

if [[ "${1:-}" == "--dry-run" ]]; then
    echo "v0.29.0 example module layout smoke dry run"
    exit 0
fi

echo "v0.29.0 example module layout smoke: tracked workspace_demo layout"
run test -f "$repo_root/examples/workspace_demo/app/rust/mod.rs"
run test -f "$repo_root/examples/workspace_demo/app/perception/rust/processor.rs"
run test -f "$repo_root/examples/workspace_demo/app/control/cpp/src/processor.cpp"
run test -f "$repo_root/examples/workspace_demo/app/control/cpp/inc/control_gain.hpp"
run test ! -e "$repo_root/examples/workspace_demo/app/cpp"
run test ! -e "$repo_root/examples/workspace_demo/app/control/cpp/processor.cpp"

project="$work_dir/workspace_demo"
cp -a "$repo_root/examples/workspace_demo" "$project"
rm -rf "$project/flowrt"

echo "v0.29.0 example module layout smoke: prepare"
run cargo run -p flowrt-cli -- prepare "$project/rsdl/robot.rsdl"
run grep -F '"user_file_path": "app/perception/rust/processor.rs"' \
    "$project/flowrt/app/app_api.json"
run grep -F '"user_file_path": "app/control/cpp/src/processor.cpp"' \
    "$project/flowrt/app/app_api.json"
run grep -F '"path": "app/stubs/control/cpp/src/processor.cpp"' \
    "$project/flowrt/app/app_api.json"
run grep -F 'user file: `app/control/cpp/src/processor.cpp`' \
    "$project/flowrt/app/implementation.md"
run grep -F 'reference stub: `app/stubs/control/cpp/src/processor.cpp`' \
    "$project/flowrt/app/implementation.md"
run grep -F '"${FLOWRT_USER_APP_ROOT}/*/cpp/src/*.cpp"' \
    "$project/flowrt/build/CMakeLists.txt"
run grep -F '"${FLOWRT_USER_APP_ROOT}/*/cpp/inc"' \
    "$project/flowrt/build/CMakeLists.txt"

echo "v0.29.0 example module layout smoke: build"
run env FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=1 cargo run -p flowrt-cli -- deps \
    "$project/rsdl/robot.rsdl" --backend iox2 --build-mode release
sdk_prefix="$("$repo_root/scripts/prepare-ci-iox2-cpp-sdk.sh")"
run env FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=1 CMAKE_PREFIX_PATH="$sdk_prefix" \
    cargo run -p flowrt-cli -- build --launcher "$project/rsdl/robot.rsdl"
run test -x "$project/flowrt/build/bin/release/workspace-demo-flowrt-app"
run test -x "$project/flowrt/build/bin/release/workspace_demo_cpp_app"
run test -x "$project/flowrt/build/bin/release/workspace-demo-flowrt-supervisor"

echo "v0.29.0 example module layout smoke passed"
