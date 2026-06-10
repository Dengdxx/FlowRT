#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
flowrt="${FLOWRT_BIN:-flowrt}"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v083-smoke.XXXXXX")"

cleanup() {
    rm -rf "$work_dir"
}
trap cleanup EXIT

command -v "$flowrt" >/dev/null || {
    printf 'installed flowrt command is required\n' >&2
    exit 1
}
"$flowrt" --version

case "$(dpkg --print-architecture)" in
    amd64)
        flowrt_platform="linux-amd64"
        cross_platform="linux-arm64"
        cross_rust_target="aarch64-unknown-linux-gnu"
        cross_machine="AArch64"
        cross_file_pattern="AArch64|aarch64"
        ;;
    arm64)
        flowrt_platform="linux-arm64"
        cross_platform="linux-amd64"
        cross_rust_target="x86_64-unknown-linux-gnu"
        cross_machine="Advanced Micro Devices X86-64"
        cross_file_pattern="x86-64|x86_64|amd64"
        ;;
    *)
        printf 'unsupported smoke test architecture: %s\n' "$(dpkg --print-architecture)" >&2
        exit 1
        ;;
esac

private_prefix="${FLOWRT_PRIVATE_PREFIX:-$("$flowrt" --version | awk '{print $2}' | sed 's#^#/opt/flowrt/#')}"
if [[ ! -x "$private_prefix/bin/flowrt" ]]; then
    printf 'failed to resolve installed FlowRT private prefix: %s\n' "$private_prefix" >&2
    exit 1
fi

native_manifest="$private_prefix/targets/$flowrt_platform/flowrt-target-sdk.toml"
cross_manifest="$private_prefix/targets/$cross_platform/flowrt-target-sdk.toml"
test -f "$native_manifest"
test -f "$cross_manifest"
grep -q 'platform = "'"$flowrt_platform"'"' "$native_manifest"
grep -q 'complete = true' "$native_manifest"
grep -q 'host_mirror = true' "$native_manifest"
grep -q 'platform = "'"$cross_platform"'"' "$cross_manifest"

export FLOWRT_CACHE_DIR="$work_dir/flowrt-cache"

if [[ "$flowrt_platform" != "linux-amd64" ]]; then
    grep -q 'complete = false' "$cross_manifest"
    printf 'v0.8.3 installed smoke passed on %s; reverse %s target is intentionally not embedded\n' \
        "$flowrt_platform" "$cross_platform"
    exit 0
fi

command -v aarch64-linux-gnu-gcc >/dev/null || {
    printf 'aarch64-linux-gnu-gcc is required for v0.8.3 amd64->arm64 smoke\n' >&2
    exit 1
}
command -v aarch64-linux-gnu-g++ >/dev/null || {
    printf 'aarch64-linux-gnu-g++ is required for v0.8.3 amd64->arm64 smoke\n' >&2
    exit 1
}
command -v file >/dev/null || {
    printf 'file is required for v0.8.3 amd64->arm64 smoke\n' >&2
    exit 1
}
command -v readelf >/dev/null || {
    printf 'readelf is required for v0.8.3 amd64->arm64 smoke\n' >&2
    exit 1
}
if command -v rustup >/dev/null; then
    rustup target list --installed | grep -Fxq "$cross_rust_target" || {
        printf 'Rust target %s is required for v0.8.3 amd64->arm64 smoke\n' "$cross_rust_target" >&2
        exit 1
    }
fi

grep -q 'complete = true' "$cross_manifest"
grep -q 'host_mirror = false' "$cross_manifest"
grep -q 'reason = "cross-target-sdk"' "$cross_manifest"
test -f "$private_prefix/targets/$cross_platform/include/flowrt/runtime.hpp"
test -f "$private_prefix/targets/$cross_platform/include/zenoh.h"
test -f "$private_prefix/targets/$cross_platform/lib/aarch64-linux-gnu/libiceoryx2_cxx.a"
test -f "$private_prefix/targets/$cross_platform/cmake/flowrt_runtime/flowrt_runtimeConfig.cmake"
test -f "$private_prefix/targets/$cross_platform/cmake/iceoryx2-cxx/iceoryx2-cxxConfig.cmake"
test -f "$private_prefix/targets/$cross_platform/cmake/zenohc/zenohcConfig.cmake"
test -f "$private_prefix/targets/$cross_platform/pkgconfig/zenohc.pc"

cpp_demo="$work_dir/cpp_counter_demo"
cp -a "$repo_root/examples/cpp_counter_demo" "$cpp_demo"
rm -rf "$cpp_demo/flowrt"

fake_sdk="$work_dir/fake-board-sdk"
mkdir -p "$fake_sdk/include" "$fake_sdk/cmake/FakeBoard" \
    "$fake_sdk/lib/aarch64-linux-gnu/pkgconfig"
printf '#pragma once\n#define FLOWRT_FAKE_BOARD_SDK 1\n' > "$fake_sdk/include/fake_board.h"
cat > "$fake_sdk/cmake/FakeBoard/FakeBoardConfig.cmake" <<'EOF'
add_library(FakeBoard::fake INTERFACE IMPORTED)
EOF
cat > "$fake_sdk/lib/aarch64-linux-gnu/pkgconfig/fake-board.pc" <<EOF
prefix=$fake_sdk
includedir=\${prefix}/include
libdir=\${prefix}/lib/aarch64-linux-gnu

Name: fake-board
Description: FlowRT v0.8.3 fake target SDK overlay fixture
Version: 0.0.0
Cflags: -I\${includedir}
Libs: -L\${libdir}
EOF
mkdir -p "$cpp_demo/.flowrt"
cat > "$cpp_demo/.flowrt/toolchains.toml" <<EOF
[toolchain.$cross_platform]
sdk_overlays = ["$fake_sdk"]
cmake_prefix_paths = ["$fake_sdk"]
pkg_config_libdirs = ["$fake_sdk/lib/aarch64-linux-gnu/pkgconfig"]
runtime_dependency_policy = "external"
EOF

(
    cd "$cpp_demo"
    "$flowrt" doctor --target "$cross_platform" > "$work_dir/doctor.out"
)
grep -q "target platform: $cross_platform" "$work_dir/doctor.out"
grep -q "runtime dependency policy: external" "$work_dir/doctor.out"
grep -q "target SDK: ok" "$work_dir/doctor.out"
grep -q "sdk overlay: ok" "$work_dir/doctor.out"

if ! "$flowrt" build --target "$cross_platform" "$cpp_demo/rsdl/robot.rsdl" \
    > "$work_dir/cpp-cross.out" 2>&1; then
    sed -n '1,260p' "$work_dir/cpp-cross.out" >&2
    exit 1
fi
cpp_bin="$cpp_demo/flowrt/build/bin/$cross_platform/release/cpp_counter_demo_cpp_app"
test -x "$cpp_bin"
file_output="$(file "$cpp_bin")"
if ! grep -Eiq "$cross_file_pattern" <<<"$file_output"; then
    printf 'unexpected cross binary file output: %s\n' "$file_output" >&2
    exit 1
fi
readelf_header="$(readelf -h "$cpp_bin")"
if ! grep -q "Machine:[[:space:]]*$cross_machine" <<<"$readelf_header"; then
    printf 'unexpected cross binary ELF header:\n%s\n' "$readelf_header" >&2
    exit 1
fi

printf 'v0.8.3 installed smoke passed\n'
