#!/usr/bin/env bash
set -euo pipefail

if [[ "${FLOWRT_V0231_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.23.1 route health smoke dry run"
    exit 0
fi

echo "v0.23.1 route health smoke: Rust runtime route facts"
cargo test -p flowrt route_backend_health --lib

echo "v0.23.1 route health smoke: CLI route health output"
cargo test -p flowrt-cli live_status_summary_displays_channel_input_and_route_diagnostics

echo "v0.23.1 route health smoke: codegen transport publish health"
cargo test -p flowrt-codegen backend_route_health

echo "v0.23.1 route health smoke: C++ introspection parity"
cmake -S runtime/cpp -B build/cpp
cmake --build build/cpp
ctest --test-dir build/cpp -R introspection_smoke --output-on-failure

echo "v0.23.1 route health smoke passed"
