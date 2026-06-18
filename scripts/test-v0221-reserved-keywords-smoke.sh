#!/usr/bin/env bash
# v0.22.1 Reserved Keyword Naming focused smoke。
# 范围：validator 拒绝会被 codegen emit 为 Rust/C++ 标识符的 RSDL 名称撞关键字，
# 同时保留 profile.default 等非标识符名称合法。

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

echo "v0.22.1 reserved keyword smoke passed"
