#!/usr/bin/env bash
# v0.23.2 C++ clang-tidy focused smoke 的兼容入口。
#
# 长期规则从 v0.27.1 起集中在 scripts/test-cpp-static-quality.sh；本脚本保留旧 release gate
# registry 入口和旧 dry-run 环境变量，避免历史 gate 漂移成第二套 clang-tidy 规则。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ "${FLOWRT_V0232_CLANG_TIDY_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.23.2 C++ clang-tidy smoke dry run"
    exit 0
fi

scripts/test-cpp-static-quality.sh
