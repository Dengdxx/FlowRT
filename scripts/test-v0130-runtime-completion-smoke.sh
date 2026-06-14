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
export FLOWRT_CACHE_DIR="${FLOWRT_CACHE_DIR:-$repo_root/.flowrt-cache/v0130-runtime-$smoke_target_platform}"
export FLOWRT_TICK_SLEEP_MS="${FLOWRT_TICK_SLEEP_MS:-5}"

if [[ -f "$repo_root/Cargo.toml" ]]; then
    export FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK="${FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK:-1}"
fi

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

if [[ "${FLOWRT_V0130_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    printf 'v0.13.0 runtime completion smoke dry run: target=%s cache=%s\n' \
        "$smoke_target_platform" "$FLOWRT_CACHE_DIR"
    exit 0
fi

cd "$repo_root"

printf 'v0.13.0 runtime completion smoke: temporary island/replay/clock\n'
cargo_test flowrt-cli replay
cargo_test flowrt-cli temporary
cargo_test flowrt-codegen island

printf 'v0.13.0 runtime completion smoke: abstract resource contract\n'
cargo_test flowrt-ir resource
cargo_test flowrt-validate resource
cargo_test flowrt-cli resource

printf 'v0.13.0 runtime completion smoke: external and I/O boundary health\n'
cargo_test flowrt-cli external
cargo_test flowrt-cli boundary
cargo_test flowrt supervisor

printf 'v0.13.0 runtime completion smoke: variable frame engineering\n'
cargo_test flowrt message_frame
cargo_test flowrt-codegen message_frame
cargo_test flowrt-cli echo

printf 'v0.13.0 runtime completion smoke: params runtime apply\n'
cargo_test flowrt params
cargo_test flowrt-codegen params
cargo_test flowrt-cli params

printf 'v0.13.0 runtime completion smoke: operation lifecycle/control authority\n'
cargo_test flowrt operation
cargo_test flowrt-codegen operation
cargo_test flowrt-cli operation

printf 'v0.13.0 runtime completion smoke: diagnostics/status/record\n'
cargo_test flowrt-cli diagnostics
cargo_test flowrt-cli status
cargo_test flowrt-cli record

printf 'v0.13.0 runtime completion smoke: deploy/cross hardening\n'
cargo_test flowrt-cli bundle
cargo_test flowrt-cli deploy
cargo_test flowrt-cli doctor
cargo_test flowrt-cli cross

printf 'v0.13.0 runtime completion smoke: C ABI future language boundary\n'
cargo_test flowrt abi

printf 'v0.13.0 runtime completion smoke passed\n'
