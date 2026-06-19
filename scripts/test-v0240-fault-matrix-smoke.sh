#!/usr/bin/env bash
# v0.24.0 focused smoke。
# 范围：fault matrix parser/check/run、final status snapshot、supervisor aggregation 和 demo。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

if [[ "${FLOWRT_V0240_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.24.0 focused smoke dry run"
    exit 0
fi

export FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK="${FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK:-1}"

echo "v0.24.0 focused smoke: script syntax"
run bash -n scripts/test-v0240-fault-matrix-smoke.sh

echo "v0.24.0 focused smoke: parser, check and evaluator"
run cargo test -p flowrt-cli fault_matrix

echo "v0.24.0 focused smoke: final status snapshot codegen"
run cargo test -p flowrt-codegen status_out

echo "v0.24.0 focused smoke: supervisor snapshot aggregation"
run cargo test -p flowrt supervisor::tests::command_tests
run cargo test -p flowrt supervisor::tests::lifecycle_tests

echo "v0.24.0 focused smoke: demo matrix"
run cargo run -p flowrt-cli -- fault-matrix check examples/fault_matrix_demo/fault-matrix.toml
run cargo run -p flowrt-cli -- fault-matrix run examples/fault_matrix_demo/fault-matrix.toml \
    --out-dir target/flowrt-fault-matrix \
    --report target/fault-matrix-report.json

echo "v0.24.0 focused smoke passed"
