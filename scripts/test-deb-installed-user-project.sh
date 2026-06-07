#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-deb-install-test.XXXXXX")"

cleanup() {
    rm -rf "$work_dir"
}
trap cleanup EXIT

out_dir="$work_dir/dist"
"$repo_root/scripts/package-deb.sh" --output-dir "$out_dir"
package="$(find "$out_dir" -maxdepth 1 -type f -name 'flowrt_*_*.deb' | sort | head -n 1)"
if [[ -z "$package" ]]; then
    printf 'package-deb.sh did not produce a flowrt deb package\n' >&2
    exit 1
fi

root="$work_dir/root"
dpkg-deb -x "$package" "$root"
version="$(dpkg-deb -f "$package" Version)"

flowrt="$root/opt/flowrt/${version}/bin/flowrt"
if [[ ! -x "$flowrt" ]]; then
    printf 'installed flowrt binary is missing: %s\n' "$flowrt" >&2
    exit 1
fi
private_prefix="$root/opt/flowrt/${version}"

user_root="$work_dir/user-projects"
mkdir -p "$user_root"
cp -a "$repo_root/examples/import_demo" "$user_root/import_demo"
cp -a "$repo_root/examples/cpp_counter_demo" "$user_root/cpp_counter_demo"
cp -a "$repo_root/examples/mixed_iox2_demo" "$user_root/mixed_iox2_demo"
cp -a "$repo_root/examples/mixed_zenoh_demo" "$user_root/mixed_zenoh_demo"
rm -rf "$user_root/import_demo/flowrt" "$user_root/cpp_counter_demo/flowrt" \
    "$user_root/mixed_iox2_demo/flowrt" "$user_root/mixed_zenoh_demo/flowrt"

export CARGO_HOME="$work_dir/cargo-home"
export CARGO_NET_OFFLINE=true

"$flowrt" build --launcher "$user_root/import_demo/rsdl/robot.rsdl"
test -x "$user_root/import_demo/flowrt/build/target/debug/import-demo-flowrt-app"

"$flowrt" build --launcher "$user_root/cpp_counter_demo/rsdl/robot.rsdl"
test -x "$user_root/cpp_counter_demo/flowrt/build/cmake/cpp_counter_demo_cpp_app"

"$flowrt" prepare "$user_root/mixed_iox2_demo/rsdl/robot.rsdl"
cmake -S "$user_root/mixed_iox2_demo/flowrt/build" \
    -B "$user_root/mixed_iox2_demo/flowrt/build/cmake" \
    -DFLOWRT_CPP_RUNTIME_DIR="$private_prefix"

"$flowrt" prepare "$user_root/mixed_zenoh_demo/rsdl/robot.rsdl"
cmake -S "$user_root/mixed_zenoh_demo/flowrt/build" \
    -B "$user_root/mixed_zenoh_demo/flowrt/build/cmake" \
    -DFLOWRT_CPP_RUNTIME_DIR="$private_prefix"

if grep -R "$repo_root/runtime/rust" "$user_root/import_demo/flowrt/build/Cargo.toml" \
    "$user_root/cpp_counter_demo/flowrt/build/Cargo.toml"; then
    printf 'generated Cargo manifests unexpectedly reference the FlowRT source repository\n' >&2
    exit 1
fi

if [[ ! -f "$user_root/import_demo/flowrt/build/.cargo/config.toml" ]]; then
    printf 'generated Rust app is missing offline Cargo config\n' >&2
    exit 1
fi
grep -q 'offline = true' "$user_root/import_demo/flowrt/build/.cargo/config.toml"

if grep -R "FLOWRT_CPP_RUNTIME_DIR=${repo_root}/runtime/cpp" "$user_root/cpp_counter_demo/flowrt/build/cmake" 2>/dev/null; then
    printf 'generated CMake cache unexpectedly references the FlowRT source repository\n' >&2
    exit 1
fi

if grep -R "FLOWRT_CPP_RUNTIME_DIR=${repo_root}/runtime/cpp" "$user_root/cpp_counter_demo/flowrt/build/CMakeLists.txt" 2>/dev/null; then
    printf 'generated CMakeLists.txt unexpectedly references the FlowRT source repository\n' >&2
    exit 1
fi

if grep -Eq '^[[:space:]]*option\([[:space:]]*FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK[[:space:]]+"[^"]*"[[:space:]]+ON[[:space:]]*\)' "$user_root/cpp_counter_demo/flowrt/build/CMakeLists.txt" 2>/dev/null; then
    printf 'generated CMakeLists.txt has FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK defaulting to ON\n' >&2
    exit 1
fi

printf 'installed deb user-project smoke passed: %s\n' "$package"
