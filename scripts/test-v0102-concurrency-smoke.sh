#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0102-concurrency.XXXXXX")"

cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.10.2 concurrency smoke work dir: %s\n' "$work_dir" >&2
        return
    fi
    rm -rf "$work_dir"
}
trap cleanup EXIT

if [[ -n "${FLOWRT_BIN:-}" ]]; then
    flowrt_cmd=("$FLOWRT_BIN")
    repo_cli=0
elif [[ -f "$repo_root/Cargo.toml" ]]; then
    flowrt_cmd=(cargo run -p flowrt-cli --)
    repo_cli=1
elif command -v flowrt >/dev/null; then
    flowrt_cmd=(flowrt)
    repo_cli=0
else
    printf 'flowrt command is required; set FLOWRT_BIN or run from the FlowRT repository\n' >&2
    exit 1
fi

run_flowrt() {
    (cd "$repo_root" && "${flowrt_cmd[@]}" "$@")
}

smoke_target_platform="${FLOWRT_SMOKE_TARGET_PLATFORM:-linux-amd64}"
case "$smoke_target_platform" in
    linux-amd64|linux-arm64) ;;
    *)
        printf 'unsupported FLOWRT_SMOKE_TARGET_PLATFORM: %s\n' "$smoke_target_platform" >&2
        exit 1
        ;;
esac

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
export FLOWRT_CACHE_DIR="${FLOWRT_CACHE_DIR:-$work_dir/flowrt-cache}"
if [[ "$repo_cli" == "1" ]]; then
    export FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=1
fi

printf 'v0.10.2 concurrency smoke: focused Rust/codegen tests\n'
(
    cd "$repo_root"
    cargo test -p flowrt-codegen concurrency -j1
    cargo test -p flowrt-codegen rust_iox2 -j1
    cargo test -p flowrt-codegen backend -j1
    cargo test -p flowrt executor -j1
)

printf 'v0.10.2 concurrency smoke: C++ runtime executor tests\n'
cmake -S "$repo_root/runtime/cpp" -B "$work_dir/cpp-runtime" -G Ninja
cmake --build "$work_dir/cpp-runtime" -j1
ctest --test-dir "$work_dir/cpp-runtime" --output-on-failure

printf 'v0.10.2 concurrency smoke: generated Rust shell check\n'
rust_demo="$work_dir/import_demo"
mkdir -p "$rust_demo"
cp -R "$repo_root/examples/import_demo/." "$rust_demo/"
find "$rust_demo/rsdl" -type f -name '*.rsdl' -print0 |
    xargs -0 sed -i -E "s/platform = \"linux-(amd64|arm64)\"/platform = \"$smoke_target_platform\"/g"
run_flowrt prepare "$rust_demo/rsdl/robot.rsdl"
cargo check \
    --manifest-path "$rust_demo/flowrt/build/Cargo.toml" \
    -j1 \
    --config "patch.crates-io.flowrt.path=\"$repo_root/runtime/rust\""

printf 'v0.10.2 concurrency smoke: generated C++ shell build\n'
cpp_demo="$work_dir/cpp_counter_demo"
mkdir -p "$cpp_demo"
cp -R "$repo_root/examples/cpp_counter_demo/." "$cpp_demo/"
find "$cpp_demo/rsdl" -type f -name '*.rsdl' -print0 |
    xargs -0 sed -i -E "s/platform = \"linux-(amd64|arm64)\"/platform = \"$smoke_target_platform\"/g"
run_flowrt prepare "$cpp_demo/rsdl/robot.rsdl"
cmake \
    -S "$cpp_demo/flowrt/build" \
    -B "$cpp_demo/flowrt/build/cmake-v0102-smoke" \
    -G Ninja \
    -DFLOWRT_CPP_RUNTIME_DIR="$repo_root/runtime/cpp" \
    -DFLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=ON
cmake --build "$cpp_demo/flowrt/build/cmake-v0102-smoke" -j1
ctest --test-dir "$cpp_demo/flowrt/build/cmake-v0102-smoke" --output-on-failure

printf 'v0.10.2 concurrency smoke passed\n'
