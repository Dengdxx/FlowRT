#!/usr/bin/env bash
# v0.25.1 transport evidence focused smoke。
# 明确区分 golden/prepare 证据与真实 SDK build/run 证据，并补齐 zenoh/iox2
# generated transport app 的真实 build 路径。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0251-transport-evidence.XXXXXX")"
cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.25.1 transport evidence smoke work dir: %s\n' "$work_dir" >&2
        return
    fi
    rm -rf "$work_dir"
}
trap cleanup EXIT

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

if [[ "${FLOWRT_V0251_TRANSPORT_EVIDENCE_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.25.1 transport evidence smoke dry run"
    exit 0
fi

if [[ -n "${FLOWRT_BIN:-}" ]]; then
    flowrt_cmd=("$FLOWRT_BIN")
else
    flowrt_cmd=(cargo run -q -p flowrt-cli --)
    export FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=1
fi

run_flowrt() {
    "${flowrt_cmd[@]}" "$@"
}

native_target_platform() {
    case "$(uname -m)" in
        x86_64|amd64) printf 'linux-amd64\n' ;;
        aarch64|arm64) printf 'linux-arm64\n' ;;
        *)
            printf 'unsupported smoke architecture: %s\n' "$(uname -m)" >&2
            return 1
            ;;
    esac
}

copy_demo_for_native_target() {
    local demo="$1" dest="$2" target_platform
    target_platform="$(native_target_platform)"
    mkdir -p "$dest"
    cp -R "$demo/app" "$dest/app"
    cp -R "$demo/rsdl" "$dest/rsdl"
    run sed -i -E \
        "s/platform = \"linux-(amd64|arm64)\"/platform = \"$target_platform\"/g" \
        "$dest/rsdl/robot.rsdl"
}

build_transport_demo() {
    local label="$1" backend="$2" demo="$3"
    local demo_work="$work_dir/$label"
    echo "v0.25.1 transport evidence smoke: real $backend generated build"
    copy_demo_for_native_target "$demo" "$demo_work"
    run run_flowrt deps "$demo_work/rsdl/robot.rsdl" --backend "$backend" --build-mode debug
    run run_flowrt build "$demo_work/rsdl/robot.rsdl" --out-dir "$demo_work/flowrt" --build-mode debug
}

echo "v0.25.1 transport evidence smoke: script syntax"
run bash -n scripts/test-v0251-transport-evidence-smoke.sh

echo "v0.25.1 transport evidence smoke: v0.25.0 baseline dry run"
run env FLOWRT_V0250_IOX2_SERVICE_OPERATION_SMOKE_DRY_RUN=1 \
    scripts/test-v0250-iox2-service-operation-smoke.sh

if [[ "${FLOWRT_V0251_REQUIRE_TRANSPORT_SDK:-0}" == "1" ]]; then
    build_transport_demo "zenoh_service_demo_real" "zenoh" "examples/zenoh_service_demo"
    build_transport_demo "iox2_service_demo_real" "iox2" "examples/iox2_service_demo"
else
    echo "v0.25.1 transport evidence smoke: skip real transport builds (set FLOWRT_V0251_REQUIRE_TRANSPORT_SDK=1)"
fi

echo "v0.25.1 transport evidence smoke passed"
