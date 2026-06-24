#!/usr/bin/env bash
# v0.27.1 debt closure hardening focused smoke。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

if [[ "${FLOWRT_V0271_DEBT_CLOSURE_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.27.1 debt closure smoke dry run"
    exit 0
fi

echo "v0.27.1 debt closure smoke: script syntax and evidence matrix"
run bash -n scripts/test-v0271-debt-closure-smoke.sh
run bash -n scripts/check-evidence-matrix.sh
run bash -n scripts/test-codegen-compile.sh
run bash -n scripts/test-cpp-static-quality.sh
run env FLOWRT_CODEGEN_COMPILE_CARGO_ATTEMPTS=2 FLOWRT_CODEGEN_COMPILE_RETRY_SELF_TEST=1 \
    scripts/test-codegen-compile.sh
run scripts/check-evidence-matrix.sh

echo "v0.27.1 debt closure smoke: feedback typed literal"
run cargo test -p flowrt-validate -j1 feedback_init
run cargo test -p flowrt-codegen -j1 emit_seeds_feedback_nested_struct_and_fixed_array_init_both_languages
run cargo test -p flowrt-codegen -j1 -- golden_feedback_v2_rust golden_feedback_v2_cpp

echo "v0.27.1 debt closure smoke: typed route transport errors"
run cargo test -p flowrt -j1 route_transport_error_maps_kind_to_counters_and_backend_error
run cargo test -p flowrt-cli -j1 live_status_summary_displays_channel_input_and_route_diagnostics

echo "v0.27.1 debt closure smoke: operation observation replay verification"
run cargo test -p flowrt-record -j1 read_operation_observation_trace_extracts_payload_bytes_and_terminal_error
run cargo test -p flowrt-cli -j1 operation_observation

echo "v0.27.1 debt closure smoke: C ABI readonly string params"
run cargo test -p flowrt-validate -j1 accepts_c_readonly_string_params
run cargo test -p flowrt -j1 c_component_param_snapshot_v1_abi_layout_exposes_typed_values
run cargo test -p flowrt-codegen -j1 generated_c_params_callback_receives_readonly_snapshot
run cargo test -p flowrt-codegen -j1 golden_c_params_cpp

echo "v0.27.1 debt closure smoke: generated compile net"
run scripts/test-codegen-compile.sh

echo "v0.27.1 debt closure smoke: C++ static quality"
run scripts/test-cpp-static-quality.sh

echo "v0.27.1 debt closure smoke passed"
