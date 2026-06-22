#!/usr/bin/env bash
# 确认 codegen 编译网覆盖所有会生成 Rust/C++ runtime shell 的 golden case。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

compile_script="scripts/test-codegen-compile.sh"
corpus="crates/flowrt-codegen/tests/golden"

if [[ ! -f "$compile_script" ]]; then
    printf 'compile script not found: %s\n' "$compile_script" >&2
    exit 1
fi

expected="$(mktemp "${TMPDIR:-/tmp}/flowrt-codegen-compile-expected.XXXXXX")"
actual="$(mktemp "${TMPDIR:-/tmp}/flowrt-codegen-compile-actual.XXXXXX")"
cleanup() {
    rm -f "$expected" "$actual"
}
trap cleanup EXIT

find "$corpus" -mindepth 1 -maxdepth 1 -type d | sort | while IFS= read -r case_dir; do
    case_name="$(basename "$case_dir")"
    if [[ -f "$case_dir/expected/rust/src/runtime_shell.rs" ]]; then
        printf 'rust %s\n' "$case_name"
    fi
    if [[ -f "$case_dir/expected/cpp/src/runtime_shell.cpp" ]]; then
        printf 'cpp %s\n' "$case_name"
    fi
done | sort > "$expected"

awk '
    /^compile_(rust|cpp)[[:space:]]+[A-Za-z0-9_]+[[:space:]]*$/ {
        lang = $1
        sub(/^compile_/, "", lang)
        print lang " " $2
    }
' "$compile_script" | sort > "$actual"

missing="$(comm -23 "$expected" "$actual")"
extra="$(comm -13 "$expected" "$actual")"

if [[ -n "$missing" || -n "$extra" ]]; then
    if [[ -n "$missing" ]]; then
        printf 'missing codegen compile golden cases:\n%s\n' "$missing" >&2
    fi
    if [[ -n "$extra" ]]; then
        printf 'stale codegen compile golden cases:\n%s\n' "$extra" >&2
    fi
    exit 1
fi

echo "codegen compile coverage covers all generated runtime shell golden cases"
