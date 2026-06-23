#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/prepare-ci-iox2-cpp-sdk.sh [--prefix DIR] [--work-dir DIR] [--force]

Build the locked iceoryx2-cxx C++ SDK into a local prefix for source-tree CI
smoke tests. The script prints the prefix path on stdout; progress goes to
stderr so callers can capture stdout safely.
EOF
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
prefix="$repo_root/build/ci-iox2-cpp-sdk/prefix"
work_dir="$repo_root/build/ci-iox2-cpp-sdk/work"
force=0

while [[ "$#" -gt 0 ]]; do
    case "$1" in
        --prefix)
            prefix="${2:?missing value for --prefix}"
            shift 2
            ;;
        --work-dir)
            work_dir="${2:?missing value for --work-dir}"
            shift 2
            ;;
        --force)
            force=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            printf 'unknown argument: %s\n' "$1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

for tool in cmake git dpkg-architecture; do
    command -v "$tool" >/dev/null || {
        printf '%s is required to prepare the CI iceoryx2-cxx SDK\n' "$tool" >&2
        exit 1
    }
done

deps_lock="$repo_root/scripts/deps.lock"
if [[ ! -f "$deps_lock" ]]; then
    printf 'dependency lock file not found: %s\n' "$deps_lock" >&2
    exit 1
fi

read -r lock_tag lock_url lock_commit < <(
    awk '$1 == "git" && $2 == "iceoryx2" { print $3, $4, $5; exit }' "$deps_lock"
)
if [[ -z "${lock_tag:-}" || -z "${lock_url:-}" || -z "${lock_commit:-}" ]]; then
    printf 'deps.lock: missing git lock entry for iceoryx2\n' >&2
    exit 1
fi

multiarch="$(dpkg-architecture -qDEB_HOST_MULTIARCH)"
config_path="$prefix/lib/${multiarch}/cmake/iceoryx2-cxx/iceoryx2-cxxConfig.cmake"
library_path="$prefix/lib/${multiarch}/libiceoryx2_cxx.a"

if [[ "$force" -eq 1 ]]; then
    rm -rf "$prefix" "$work_dir/build"
fi

if [[ -f "$config_path" && -f "$library_path" ]]; then
    printf 'using cached CI iceoryx2-cxx SDK: %s\n' "$prefix" >&2
    printf '%s\n' "$prefix"
    exit 0
fi

src_dir="$work_dir/iceoryx2"
build_dir="$work_dir/build"
mkdir -p "$work_dir" "$(dirname "$prefix")"

if [[ -d "$src_dir/.git" ]]; then
    printf 'updating iceoryx2 source snapshot %s\n' "$lock_tag" >&2
    git -C "$src_dir" fetch --depth 1 origin "refs/tags/${lock_tag}:refs/tags/${lock_tag}" >&2
    git -C "$src_dir" checkout --detach "$lock_tag" >&2
else
    rm -rf "$src_dir"
    printf 'cloning iceoryx2 source snapshot %s\n' "$lock_tag" >&2
    git clone --depth 1 --branch "$lock_tag" "$lock_url" "$src_dir" >&2
fi

actual_commit="$(git -C "$src_dir" rev-parse HEAD)"
if [[ "$actual_commit" != "$lock_commit" ]]; then
    printf 'FATAL: iceoryx2 tag %s commit mismatch\n  expected: %s\n  actual:   %s\n' \
        "$lock_tag" "$lock_commit" "$actual_commit" >&2
    exit 1
fi

rm -rf "$build_dir" "$prefix"
cmake -S "$src_dir" -B "$build_dir" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$prefix" \
    -DCMAKE_INSTALL_LIBDIR="lib/${multiarch}" \
    -DBUILD_CXX=ON \
    -DBUILD_EXAMPLES=OFF \
    -DBUILD_TESTING=OFF >&2
cmake --build "$build_dir" -j1 >&2
cmake --install "$build_dir" >&2

if [[ ! -f "$config_path" || ! -f "$library_path" ]]; then
    printf 'FATAL: prepared iceoryx2-cxx SDK is incomplete under %s\n' "$prefix" >&2
    printf 'missing config: %s\nmissing library: %s\n' "$config_path" "$library_path" >&2
    exit 1
fi

printf '%s\n' "$prefix"
