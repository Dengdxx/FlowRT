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

required_paths=(
    './usr/bin/flowrt'
    './usr/share/flowrt/runtime/rust/Cargo.toml'
    './usr/share/flowrt/runtime/rust/src/lib.rs'
    './usr/include/flowrt/runtime.hpp'
    './usr/share/doc/flowrt/copyright'
    './usr/share/doc/flowrt/changelog.gz'
)

for path in "${required_paths[@]}"; do
    if ! grep -Fq "$path" "$contents"; then
        printf 'package is missing required path: %s\n' "$path" >&2
        exit 1
    fi
done

multiarch="$(dpkg-architecture -qDEB_HOST_MULTIARCH)"
cmake_config="./usr/lib/${multiarch}/cmake/flowrt_runtime/flowrt_runtimeConfig.cmake"
if ! grep -Fq "$cmake_config" "$contents"; then
    printf 'package is missing multiarch CMake config: %s\n' "$cmake_config" >&2
    exit 1
fi

printf 'deb package smoke passed: %s\n' "$package"
