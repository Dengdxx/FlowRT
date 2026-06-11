#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
flowrt="${FLOWRT_BIN:-flowrt}"
if [[ "$flowrt" == */* && "$flowrt" != /* ]]; then
    flowrt="$repo_root/$flowrt"
fi
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v086-cross-sdk.XXXXXX")"

cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.8.6 cross SDK smoke work dir: %s\n' "$work_dir" >&2
        return
    fi
    rm -rf "$work_dir"
}
trap cleanup EXIT

require_command() {
    command -v "$1" >/dev/null || {
        printf '%s is required for v0.8.6 cross SDK smoke\n' "$1" >&2
        exit 1
    }
}

sdk_has_expected_pkg_config() {
    local sdk_root="$1"
    test -f "$sdk_root/lib/aarch64-linux-gnu/pkgconfig/libjpeg.pc" &&
        test -f "$sdk_root/lib/aarch64-linux-gnu/pkgconfig/kleidiai.pc"
}

prepare_public_arm64_sdk() {
    local sdk_root="$1"
    local build_dir="$work_dir/cross-sdk-deps-build"
    local jobs="${FLOWRT_BUILD_JOBS:-1}"
    local cmake_args=(
        -S "$repo_root/examples/cross_sdk_deps"
        -B "$build_dir"
        -DCMAKE_BUILD_TYPE=Release
        -DFLOWRT_CROSS_SDK_PREFIX="$sdk_root"
        -DFLOWRT_CROSS_BUILD_JOBS="$jobs"
    )
    if command -v ninja >/dev/null; then
        cmake_args+=(-G Ninja)
    fi

    cmake "${cmake_args[@]}"
    cmake --build "$build_dir" --target flowrt_public_arm64_sdk --parallel "$jobs"
    sdk_has_expected_pkg_config "$sdk_root"
}

init_demo_toolchain() {
    local demo_dir="$1"
    local sdk_root="$2"
    local demo_name="$3"

    (
        cd "$demo_dir"
        "$flowrt" toolchain init \
            --target linux-arm64 \
            --sdk-overlay "$sdk_root" \
            --force > "$work_dir/$demo_name.toolchain-init.out"
        "$flowrt" toolchain show --target linux-arm64 > "$work_dir/$demo_name.toolchain-show.out"
    )
    grep -q 'platform: linux-arm64' "$work_dir/$demo_name.toolchain-show.out"
    grep -q "$sdk_root" "$work_dir/$demo_name.toolchain-show.out"
    grep -q 'runtime_dependency_policy: bundle' "$work_dir/$demo_name.toolchain-show.out"
}

build_demo() {
    local demo_name="$1"
    local package_name="$2"
    local pkg_config_module="$3"
    local sdk_root="$4"

    local demo_dir="$work_dir/$demo_name"
    cp -a "$repo_root/examples/$demo_name" "$demo_dir"
    rm -rf "$demo_dir/flowrt" "$demo_dir/.flowrt"
    init_demo_toolchain "$demo_dir" "$sdk_root" "$demo_name"

    if ! (
        cd "$demo_dir"
        "$flowrt" doctor rsdl/robot.rsdl --target linux-arm64 > "$work_dir/$demo_name.doctor.out"
    ); then
        sed -n '1,260p' "$work_dir/$demo_name.doctor.out" >&2
        exit 1
    fi
    grep -q 'target platform: linux-arm64' "$work_dir/$demo_name.doctor.out"
    grep -q 'runtime dependency policy: bundle' "$work_dir/$demo_name.doctor.out"
    grep -q 'target SDK: ok' "$work_dir/$demo_name.doctor.out"
    grep -q 'sdk overlay: ok' "$work_dir/$demo_name.doctor.out"
    grep -q "module=$pkg_config_module status=found" "$work_dir/$demo_name.doctor.out"

    if ! "$flowrt" deps "$demo_dir/rsdl/robot.rsdl" \
        --target linux-arm64 \
        --build-mode release \
        --backend inproc \
        > "$work_dir/$demo_name.deps.out" 2>&1; then
        sed -n '1,260p' "$work_dir/$demo_name.deps.out" >&2
        exit 1
    fi

    if ! "$flowrt" build --target linux-arm64 --launcher "$demo_dir/rsdl/robot.rsdl" \
        > "$work_dir/$demo_name.build.out" 2>&1; then
        sed -n '1,260p' "$work_dir/$demo_name.build.out" >&2
        exit 1
    fi
    grep -q 'build summary: target=linux-arm64 mode=release' "$work_dir/$demo_name.build.out"
    grep -q 'pkg-config=' "$work_dir/$demo_name.build.out"

    local binary="$demo_dir/flowrt/build/bin/linux-arm64/release/${package_name}_cpp_app"
    test -x "$binary"
    file "$binary" | grep -Eiq 'AArch64|aarch64'
    readelf -h "$binary" | grep -q 'Machine:[[:space:]]*AArch64'
    printf '%s\n' "$binary" > "$work_dir/$demo_name.binary"
}

require_command "$flowrt"
require_command cmake
require_command git
require_command aarch64-linux-gnu-gcc
require_command aarch64-linux-gnu-g++
require_command pkg-config
require_command file
require_command readelf

"$flowrt" --version
printf 'v0.8.6 cross SDK smoke work dir: %s\n' "$work_dir"

sdk_root="${FLOWRT_PUBLIC_ARM64_SDK_OVERLAY:-$work_dir/public-arm64-sdk}"
if ! sdk_has_expected_pkg_config "$sdk_root"; then
    prepare_public_arm64_sdk "$sdk_root"
fi

export FLOWRT_CACHE_DIR="${FLOWRT_CACHE_DIR:-$work_dir/flowrt-cache}"

build_demo libjpeg_cross_demo libjpeg_cross_demo libjpeg "$sdk_root"
build_demo kleidiai_cross_demo kleidiai_cross_demo kleidiai "$sdk_root"

printf 'v0.8.6 cross SDK demo smoke passed\n'
printf 'libjpeg binary: %s\n' "$(cat "$work_dir/libjpeg_cross_demo.binary")"
printf 'kleidiai binary: %s\n' "$(cat "$work_dir/kleidiai_cross_demo.binary")"
