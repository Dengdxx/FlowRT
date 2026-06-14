#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

smoke_target_platform="${FLOWRT_SMOKE_TARGET_PLATFORM:-linux-amd64}"
case "$smoke_target_platform" in
    linux-amd64|linux-arm64) ;;
    *)
        printf 'unsupported FLOWRT_SMOKE_TARGET_PLATFORM: %s\n' "$smoke_target_platform" >&2
        exit 1
        ;;
esac

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
export FLOWRT_BUILD_JOBS="${FLOWRT_BUILD_JOBS:-1}"
export FLOWRT_CACHE_DIR="${FLOWRT_CACHE_DIR:-$repo_root/.flowrt-cache/v0140-realtime-$smoke_target_platform}"

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

cargo_test() {
    local package="$1"
    local filter="$2"
    run cargo test -p "$package" "$filter" -j1
}

if [[ "${FLOWRT_V0140_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    printf 'v0.14.0 realtime scheduler smoke dry run: target=%s cache=%s\n' \
        "$smoke_target_platform" "$FLOWRT_CACHE_DIR"
    exit 0
fi

cd "$repo_root"

printf 'v0.14.0 realtime scheduler smoke: executor admission/completion\n'
cargo_test flowrt executor

printf 'v0.14.0 realtime scheduler smoke: generated Rust/C++ nonblocking scheduler\n'
cargo_test flowrt-codegen tasks

printf 'v0.14.0 realtime scheduler smoke: status timing fields and replay clock\n'
cargo_test flowrt-cli selfdesc_status
cargo_test flowrt introspection

printf 'v0.14.0 realtime scheduler smoke: C ABI task timing layout\n'
cargo_test flowrt abi

printf 'v0.14.0 realtime scheduler smoke passed\n'
