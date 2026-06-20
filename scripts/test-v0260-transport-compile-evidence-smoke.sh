#!/usr/bin/env bash
# v0.26.0 transport compile evidence focused smoke。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

if [[ "${FLOWRT_V0260_TRANSPORT_COMPILE_EVIDENCE_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.26.0 transport compile evidence smoke dry run"
    exit 0
fi

echo "v0.26.0 transport compile evidence smoke: script syntax"
run bash -n scripts/test-v0260-transport-compile-evidence-smoke.sh
run bash -n scripts/test-codegen-compile.sh

echo "v0.26.0 transport compile evidence smoke: codegen compile net"
run scripts/test-codegen-compile.sh

echo "v0.26.0 transport compile evidence smoke: transport golden"
run cargo test -p flowrt-codegen -j1 -- \
    golden_bounded_channel_iox2 \
    golden_cross_process_feedback \
    golden_zenoh_service \
    golden_iox2_service \
    golden_bounded_service_iox2 \
    golden_zenoh_operation \
    golden_iox2_operation \
    golden_bounded_operation_iox2

echo "v0.26.0 transport compile evidence smoke passed"
