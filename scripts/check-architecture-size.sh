#!/usr/bin/env bash
# 检查源码单文件规模，避免继续堆叠难以维护的巨型文件。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

rust_line_limit="${FLOWRT_ARCH_SIZE_RUST_LIMIT:-2500}"
cpp_line_limit="${FLOWRT_ARCH_SIZE_CPP_LIMIT:-2500}"
shell_line_limit="${FLOWRT_ARCH_SIZE_SHELL_LIMIT:-1200}"

errors=0
checked_count=0

fail() {
    printf '错误: %s\n' "$1" >&2
    errors=$((errors + 1))
}

info() {
    printf '→ %s\n' "$1"
}

file_kind_and_limit() {
    case "$1" in
        *.rs)
            printf 'Rust 源码|%s\n' "$rust_line_limit"
            ;;
        *.c | *.cc | *.cpp | *.cxx | *.h | *.hh | *.hpp | *.hxx)
            printf 'C/C++ 源码|%s\n' "$cpp_line_limit"
            ;;
        *.sh)
            printf 'shell 脚本|%s\n' "$shell_line_limit"
            ;;
        *)
            return 1
            ;;
    esac
}

line_count_for() {
    local path="$1"
    wc -l <"$path" | tr -d ' '
}

cd "$repo_root"

info "源码规模阈值：Rust=${rust_line_limit} 行，C/C++=${cpp_line_limit} 行，shell=${shell_line_limit} 行"

while IFS= read -r -d '' path; do
    metadata="$(file_kind_and_limit "$path")"
    kind="${metadata%%|*}"
    limit="${metadata#*|}"
    lines="$(line_count_for "$path")"
    checked_count=$((checked_count + 1))

    if [[ "$lines" -le "$limit" ]]; then
        continue
    fi

    fail "$path 为 ${kind}，当前 ${lines} 行超过阈值 ${limit} 行；请在本版本拆分文件"
done < <(
    git ls-files -z -- \
        '*.rs' \
        '*.c' \
        '*.cc' \
        '*.cpp' \
        '*.cxx' \
        '*.h' \
        '*.hh' \
        '*.hpp' \
        '*.hxx' \
        '*.sh' \
        ':(exclude)crates/flowrt-codegen/tests/golden/**'
)

if [[ "$errors" -gt 0 ]]; then
    printf '架构规模检查失败：发现 %d 个问题。\n' "$errors" >&2
    exit 1
fi

printf '架构规模检查通过：已检查 %d 个 tracked 源码/脚本文件，未发现超阈值文件。\n' "$checked_count"
