#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-deb-test.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT

host_architecture="$(dpkg --print-architecture)"
case "$host_architecture" in
    amd64)
        mismatched_architecture="arm64"
        ;;
    arm64)
        mismatched_architecture="amd64"
        ;;
    *)
        mismatched_architecture=""
        ;;
esac

if [[ -n "$mismatched_architecture" ]]; then
    mismatch_log="$work_dir/mismatched-architecture.log"
    if "$repo_root/scripts/package-deb.sh" \
        --output-dir "$work_dir/mismatch" \
        --architecture "$mismatched_architecture" \
        >"$mismatch_log" 2>&1; then
        printf 'package-deb unexpectedly accepted mismatched architecture %s on %s\n' \
            "$mismatched_architecture" "$host_architecture" >&2
        exit 1
    fi
    grep -q '只支持原生架构打包' "$mismatch_log"
fi

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
    "${prefix}/share/flowrt/runtime/rust/examples/zenoh_service_client.rs"
    "${prefix}/share/flowrt/runtime/rust/examples/zenoh_service_server.rs"
    "${prefix}/share/flowrt/runtime/rust/src/lib.rs"
    "${prefix}/share/flowrt/crates/flowrt-record/Cargo.toml"
    "${prefix}/share/flowrt/crates/flowrt-record/src/lib.rs"
    "${prefix}/share/cargo/config.toml"
    "${prefix}/share/cargo/vendor"
    "${prefix}/share/cargo/vendor/.flowrt-vendor.sha256"
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

record_check="$work_dir/flowrt-record"
mkdir -p "$record_check"
dpkg-deb --fsys-tarfile "$package" | tar -xO "${prefix}/share/flowrt/crates/flowrt-record/Cargo.toml" \
    > "$record_check/Cargo.toml"
mkdir -p "$record_check/src"
dpkg-deb --fsys-tarfile "$package" | tar -xO "${prefix}/share/flowrt/crates/flowrt-record/src/lib.rs" \
    > "$record_check/src/lib.rs"
cargo metadata --format-version 1 --manifest-path "$record_check/Cargo.toml" --no-deps >/dev/null

runtime_check="$work_dir/runtime-check"
mkdir -p "$runtime_check"
dpkg-deb --fsys-tarfile "$package" | tar -xf - -C "$runtime_check" \
    "${prefix}/share/flowrt/runtime/rust" \
    "${prefix}/share/flowrt/crates/flowrt-record"
cargo metadata \
    --format-version 1 \
    --manifest-path "$runtime_check/${prefix#./}/share/flowrt/runtime/rust/Cargo.toml" \
    --no-deps >/dev/null

copyright="$work_dir/copyright"
dpkg-deb --fsys-tarfile "$package" | tar -xO ./usr/share/doc/flowrt/copyright > "$copyright"
grep -q 'License: MIT' "$copyright"
grep -q 'Source: https://github.com/Dengdxx/FlowRT' "$copyright"
if grep -Eq 'example\.invalid|placeholder|public release|MIT-or-Apache' "$copyright"; then
    printf 'package copyright still contains placeholder release metadata\n' >&2
    exit 1
fi

printf 'deb package smoke passed: %s\n' "$package"
