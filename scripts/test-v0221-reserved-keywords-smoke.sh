#!/usr/bin/env bash
# v0.22.1 Reserved Keyword Naming focused smoke。
# 范围：validator 拒绝会被 codegen emit 为 Rust/C++ 标识符的 RSDL 名称撞关键字，
# 同时保留 profile.default 等非标识符名称合法；并守住同版本顺带关闭的
# exposed-but-fake 显式 opt-in（Operation feedback="fifo"、显式
# result_retention_ms、FrameDescriptor record_payload=true）的 fail-fast 拒绝。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

if [[ "${FLOWRT_V0221_RESERVED_KEYWORDS_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.22.1 reserved keyword smoke dry run"
    exit 0
fi

echo "v0.22.1 reserved keyword smoke: script syntax"
run bash -n scripts/test-v0221-reserved-keywords-smoke.sh

echo "v0.22.1 reserved keyword smoke: validator keyword gates"
run cargo test -p flowrt-validate -j1 -- name_tests

echo "v0.22.1 reserved keyword smoke: unsupported explicit opt-in gates"
run cargo test -p flowrt-validate -j1 -- \
    rejects_operation_policies_not_supported_by_generated_runtime \
    rejects_frame_descriptor_payload_recording_until_data_plane_is_modeled

echo "v0.22.1 reserved keyword smoke passed"
