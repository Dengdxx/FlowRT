#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 1 ]]; then
    printf 'usage: %s path/to/flowrt_VERSION_ARCH.deb\n' "$0" >&2
    exit 2
fi

package="$1"
if [[ ! -f "$package" ]]; then
    printf 'deb package not found: %s\n' "$package" >&2
    exit 1
fi

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-deb-target-sdk-test.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT

architecture="$(dpkg-deb -f "$package" Architecture)"
version="$(dpkg-deb -f "$package" Version)"
case "$architecture" in
    amd64)
        platform="linux-amd64"
        other_architecture="arm64"
        other_platform="linux-arm64"
        other_complete="true"
        ;;
    arm64)
        platform="linux-arm64"
        other_architecture="amd64"
        other_platform="linux-amd64"
        other_complete="false"
        ;;
    *)
        printf 'unsupported package architecture: %s\n' "$architecture" >&2
        exit 1
        ;;
esac

multiarch="$(dpkg-architecture -a"$architecture" -qDEB_HOST_MULTIARCH)"
other_multiarch="$(dpkg-architecture -a"$other_architecture" -qDEB_HOST_MULTIARCH)"
prefix="./opt/flowrt/${version}"
target_prefix="${prefix}/targets/${platform}"
other_target_prefix="${prefix}/targets/${other_platform}"

contents="$work_dir/contents.txt"
dpkg-deb -c "$package" > "$contents"

required_paths=(
    "${target_prefix}/flowrt-target-sdk.toml"
    "${target_prefix}/include/flowrt/runtime.hpp"
    "${target_prefix}/include/zenoh.h"
    "${target_prefix}/include/zenoh.hxx"
    "${target_prefix}/lib/libzenohc.so"
    "${target_prefix}/lib/${multiarch}/libiceoryx2_cxx.a"
    "${target_prefix}/lib/${multiarch}/cmake/flowrt_runtime/flowrt_runtimeConfig.cmake"
    "${target_prefix}/lib/${multiarch}/cmake/iceoryx2-cxx/iceoryx2-cxxConfig.cmake"
    "${target_prefix}/cmake/flowrt_runtime/flowrt_runtimeConfig.cmake"
    "${target_prefix}/cmake/iceoryx2-cxx/iceoryx2-cxxConfig.cmake"
    "${target_prefix}/cmake/zenohc/zenohcConfig.cmake"
    "${target_prefix}/cmake/zenohcxx/zenohcxxConfig.cmake"
    "${target_prefix}/pkgconfig/zenohc.pc"
    "${target_prefix}/pkgconfig/zenohcxx.pc"
    "${other_target_prefix}/flowrt-target-sdk.toml"
)

if [[ "$other_complete" == "true" ]]; then
    required_paths+=(
        "${other_target_prefix}/include/flowrt/runtime.hpp"
        "${other_target_prefix}/include/zenoh.h"
        "${other_target_prefix}/include/zenoh.hxx"
        "${other_target_prefix}/lib/libzenohc.so"
        "${other_target_prefix}/lib/${other_multiarch}/libiceoryx2_cxx.a"
        "${other_target_prefix}/lib/${other_multiarch}/cmake/flowrt_runtime/flowrt_runtimeConfig.cmake"
        "${other_target_prefix}/lib/${other_multiarch}/cmake/iceoryx2-cxx/iceoryx2-cxxConfig.cmake"
        "${other_target_prefix}/cmake/flowrt_runtime/flowrt_runtimeConfig.cmake"
        "${other_target_prefix}/cmake/iceoryx2-cxx/iceoryx2-cxxConfig.cmake"
        "${other_target_prefix}/cmake/zenohc/zenohcConfig.cmake"
        "${other_target_prefix}/cmake/zenohcxx/zenohcxxConfig.cmake"
        "${other_target_prefix}/pkgconfig/zenohc.pc"
        "${other_target_prefix}/pkgconfig/zenohcxx.pc"
    )
fi

for path in "${required_paths[@]}"; do
    if ! grep -Fq "$path" "$contents"; then
        printf 'package is missing target SDK path: %s\n' "$path" >&2
        exit 1
    fi
done

target_manifest="$work_dir/flowrt-target-sdk.toml"
dpkg-deb --fsys-tarfile "$package" | tar -xO "${target_prefix}/flowrt-target-sdk.toml" \
    > "$target_manifest"
grep -q 'platform = "'"$platform"'"' "$target_manifest"
grep -q 'multiarch = "'"$multiarch"'"' "$target_manifest"
grep -q 'complete = true' "$target_manifest"
grep -q 'host_mirror = true' "$target_manifest"
grep -q '"flowrt-cpp-runtime"' "$target_manifest"
grep -q '"iceoryx2-cxx"' "$target_manifest"
grep -q '"zenoh-c"' "$target_manifest"
grep -q '"zenoh-cpp"' "$target_manifest"

target_pkgconfig="$work_dir/zenohc.pc"
dpkg-deb --fsys-tarfile "$package" | tar -xO "${target_prefix}/pkgconfig/zenohc.pc" \
    > "$target_pkgconfig"
grep -q "prefix=/opt/flowrt/${version}/targets/${platform}" "$target_pkgconfig"

other_target_manifest="$work_dir/other-flowrt-target-sdk.toml"
dpkg-deb --fsys-tarfile "$package" | tar -xO "${other_target_prefix}/flowrt-target-sdk.toml" \
    > "$other_target_manifest"
grep -q 'platform = "'"$other_platform"'"' "$other_target_manifest"
if [[ "$other_complete" == "true" ]]; then
    grep -q 'multiarch = "'"$other_multiarch"'"' "$other_target_manifest"
    grep -q 'complete = true' "$other_target_manifest"
    grep -q 'host_mirror = false' "$other_target_manifest"
    grep -q 'reason = "cross-target-sdk"' "$other_target_manifest"
    grep -q '"flowrt-cpp-runtime"' "$other_target_manifest"
    grep -q '"iceoryx2-cxx"' "$other_target_manifest"
    grep -q '"zenoh-c"' "$other_target_manifest"
    grep -q '"zenoh-cpp"' "$other_target_manifest"

    other_pkgconfig="$work_dir/other-zenohc.pc"
    dpkg-deb --fsys-tarfile "$package" | tar -xO "${other_target_prefix}/pkgconfig/zenohc.pc" \
        > "$other_pkgconfig"
    grep -q "prefix=/opt/flowrt/${version}/targets/${other_platform}" "$other_pkgconfig"
else
    grep -q 'complete = false' "$other_target_manifest"
    grep -q 'reason = "not-built-in-this-native-package"' "$other_target_manifest"
fi

printf 'deb target SDK layout smoke passed: %s\n' "$package"
