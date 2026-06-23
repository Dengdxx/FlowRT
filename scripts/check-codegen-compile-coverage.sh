#!/usr/bin/env bash
# 确认 codegen 编译网覆盖所有会生成 Rust/C++ runtime shell 的 golden case。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

scripts/check-evidence-matrix.sh
