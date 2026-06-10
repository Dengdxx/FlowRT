#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
flowrt="${FLOWRT_BIN:-flowrt}"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v082-smoke.XXXXXX")"

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
        other_platform="linux-arm64"
        other_rust_target="aarch64-unknown-linux-gnu"
        ;;
    arm64)
        flowrt_platform="linux-arm64"
        other_platform="linux-amd64"
        other_rust_target="x86_64-unknown-linux-gnu"
        ;;
    *)
        printf 'unsupported smoke test architecture: %s\n' "$(dpkg --print-architecture)" >&2
        exit 1
        ;;
esac

private_prefix="$("$flowrt" --version | awk '{print $2}' | sed 's#^#/opt/flowrt/#')"
if [[ ! -x "$private_prefix/bin/flowrt" ]]; then
    printf 'failed to resolve installed FlowRT private prefix: %s\n' "$private_prefix" >&2
    exit 1
fi

native_manifest="$private_prefix/targets/$flowrt_platform/flowrt-target-sdk.toml"
other_manifest="$private_prefix/targets/$other_platform/flowrt-target-sdk.toml"
test -f "$native_manifest"
test -f "$other_manifest"
grep -q 'platform = "'"$flowrt_platform"'"' "$native_manifest"
grep -q 'complete = true' "$native_manifest"
grep -q 'host_mirror = true' "$native_manifest"
grep -q 'platform = "'"$other_platform"'"' "$other_manifest"
grep -q 'complete = false' "$other_manifest"

export FLOWRT_CACHE_DIR="$work_dir/flowrt-cache"

if "$flowrt" deps --backend inproc --target "$other_platform" --check \
    >"$work_dir/deps-check.out" 2>&1; then
    printf 'deps --check unexpectedly succeeded for an empty cross-target cache\n' >&2
    exit 1
fi
grep -q "platform \`$other_platform\` / Rust target \`$other_rust_target\`" \
    "$work_dir/deps-check.out"
grep -q "flowrt deps --backend inproc --build-mode release --target $other_platform" \
    "$work_dir/deps-check.out"

cpp_demo="$work_dir/cpp_counter_demo"
cp -a "$repo_root/examples/cpp_counter_demo" "$cpp_demo"
rm -rf "$cpp_demo/flowrt"

if "$flowrt" build --target "$other_platform" "$cpp_demo/rsdl/robot.rsdl" \
    >"$work_dir/cpp-cross.out" 2>&1; then
    printf 'C++ cross build unexpectedly succeeded with an incomplete target SDK\n' >&2
    exit 1
fi
grep -q "FlowRT target SDK for $other_platform is incomplete" "$work_dir/cpp-cross.out"
grep -q "install a package that embeds this target SDK" "$work_dir/cpp-cross.out"

printf 'v0.8.2 installed smoke passed\n'
