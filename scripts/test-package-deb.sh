#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-deb-test.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT

out_dir="$work_dir/dist"
"$repo_root/scripts/package-deb.sh" --output-dir "$out_dir"

mapfile -t packages < <(find "$out_dir" -maxdepth 1 -type f -name 'flowrt_*_*.deb' | sort)
if [[ "${#packages[@]}" -ne 1 ]]; then
    printf 'expected exactly one flowrt deb package, found %s\n' "${#packages[@]}" >&2
    exit 1
fi

package="${packages[0]}"
package_name="$(dpkg-deb -f "$package" Package)"
architecture="$(dpkg-deb -f "$package" Architecture)"
version="$(dpkg-deb -f "$package" Version)"
if [[ "$package_name" != "flowrt" ]]; then
    printf 'expected Package=flowrt, got %s\n' "$package_name" >&2
    exit 1
fi
if [[ -z "$architecture" ]]; then
    printf 'package architecture must not be empty\n' >&2
    exit 1
fi

contents="$work_dir/contents.txt"
dpkg-deb -c "$package" > "$contents"
prefix="./opt/flowrt/${version}"
multiarch="$(dpkg-architecture -qDEB_HOST_MULTIARCH)"

required_paths=(
    './usr/bin/flowrt'
    "${prefix}/bin/flowrt"
    "${prefix}/share/flowrt/runtime/rust/Cargo.toml"
    "${prefix}/share/flowrt/runtime/rust/src/lib.rs"
    "${prefix}/share/cargo/config.toml"
    "${prefix}/share/cargo/vendor"
    "${prefix}/lib/${multiarch}/cmake/iceoryx2-cxx/iceoryx2-cxxConfig.cmake"
    "${prefix}/lib/${multiarch}/libiceoryx2_cxx.a"
    "${prefix}/lib/cmake/zenohc/zenohcConfig.cmake"
    "${prefix}/lib/cmake/zenohcxx/zenohcxxConfig.cmake"
    "${prefix}/lib/libzenohc.so"
    "${prefix}/include/zenoh.h"
    "${prefix}/include/zenoh.hxx"
    "${prefix}/share/doc/flowrt/third-party/iceoryx2.LICENSE"
    "${prefix}/share/doc/flowrt/third-party/zenoh-c.LICENSE"
    "${prefix}/share/doc/flowrt/third-party/zenoh-cpp.LICENSE"
    "${prefix}/include/flowrt/runtime.hpp"
    './usr/share/doc/flowrt/copyright'
    './usr/share/doc/flowrt/changelog.gz'
)

for path in "${required_paths[@]}"; do
    if ! grep -Fq "$path" "$contents"; then
        printf 'package is missing required path: %s\n' "$path" >&2
        exit 1
    fi
done

cmake_config="${prefix}/lib/${multiarch}/cmake/flowrt_runtime/flowrt_runtimeConfig.cmake"
if ! grep -Fq "$cmake_config" "$contents"; then
    printf 'package is missing multiarch CMake config: %s\n' "$cmake_config" >&2
    exit 1
fi

usr_bin_entry="$(dpkg-deb --fsys-tarfile "$package" | tar -tvf - ./usr/bin/flowrt)"
if [[ "$usr_bin_entry" != *"./usr/bin/flowrt -> /opt/flowrt/${version}/bin/flowrt"* ]]; then
    printf 'usr/bin/flowrt must be an absolute symlink into private prefix, got: %s\n' "$usr_bin_entry" >&2
    exit 1
fi

cargo_config="$work_dir/cargo-config.toml"
dpkg-deb --fsys-tarfile "$package" | tar -xO "${prefix}/share/cargo/config.toml" > "$cargo_config"
grep -q 'replace-with = "flowrt-vendor"' "$cargo_config"
grep -q 'offline = true' "$cargo_config"

copyright="$work_dir/copyright"
dpkg-deb --fsys-tarfile "$package" | tar -xO ./usr/share/doc/flowrt/copyright > "$copyright"
grep -q 'License: MIT' "$copyright"
grep -q 'Source: https://github.com/Dengdxx/FlowRT' "$copyright"
if grep -Eq 'example\.invalid|placeholder|public release|MIT-or-Apache' "$copyright"; then
    printf 'package copyright still contains placeholder release metadata\n' >&2
    exit 1
fi

printf 'deb package smoke passed: %s\n' "$package"
