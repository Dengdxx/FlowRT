#!/usr/bin/env bash
# v0.27.0 Operation control-plane completion focused smoke。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

if [[ "${FLOWRT_V0270_OPERATION_CONTROL_PLANE_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.27.0 operation control-plane completion smoke dry run"
    exit 0
fi

echo "v0.27.0 operation control-plane completion smoke: script syntax"
run bash -n scripts/test-v0270-operation-control-plane-smoke.sh

echo "v0.27.0 operation control-plane completion smoke: CLI operation path"
run cargo test -p flowrt-cli -j1 operation_
run cargo test -p flowrt-cli -j1 remote_operation
run cargo test -p flowrt-cli -j1 fault_matrix

echo "v0.27.0 operation control-plane completion smoke: codegen selfdesc and operation path"
run cargo test -p flowrt-codegen -j1 emits_workspace_module_symbols_without_component_name_collisions
run cargo test -p flowrt-codegen -j1 bounded_variable_frame_tests_keep_samples_within_declared_bounds
run cargo test -p flowrt-codegen -j1 rust_operation_shell_starts_remote_operation_control_plane
run cargo test -p flowrt-codegen -j1 cpp_operation_components_are_generated
run cargo test -p flowrt-codegen -j1 golden_bounded_operation_iox2

echo "v0.27.0 operation control-plane completion smoke: runtime remote operation path"
run cargo test -p flowrt --features zenoh -j1 unsupported_operation_command_error_lists_supported_commands
run cargo test -p flowrt --features zenoh -j1 remote_operation_

echo "v0.27.0 operation control-plane completion smoke passed"
