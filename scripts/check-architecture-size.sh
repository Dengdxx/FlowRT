#!/usr/bin/env bash
# 检查源码单文件规模，避免继续堆叠难以维护的巨型文件。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

rust_line_limit="${FLOWRT_ARCH_SIZE_RUST_LIMIT:-2500}"
cpp_line_limit="${FLOWRT_ARCH_SIZE_CPP_LIMIT:-2500}"
shell_line_limit="${FLOWRT_ARCH_SIZE_SHELL_LIMIT:-1200}"

errors=0
legacy_count=0

declare -A legacy_max_lines=()
declare -A legacy_target_versions=()
declare -A legacy_reasons=()

fail() {
    printf '错误: %s\n' "$1" >&2
    errors=$((errors + 1))
}

info() {
    printf '→ %s\n' "$1"
}

add_legacy_file() {
    local path="$1"
    local max_lines="$2"
    local target_version="$3"
    local reason="$4"

    legacy_max_lines["$path"]="$max_lines"
    legacy_target_versions["$path"]="$target_version"
    legacy_reasons["$path"]="$reason"
}

# 这些文件是 0.14.1 架构修缮的待拆 legacy 文件。allowlist 记录路径、上限、
# 目标版本和中文原因；超过上限或拆到阈值以下都需要同步更新本表。
add_legacy_file \
    "crates/flowrt-cli/src/main.rs" \
    7474 \
    "v0.14.1" \
    "CLI 命令分发历史聚合文件；0.14.1 拆分命令入口和子命令边界"
add_legacy_file \
    "runtime/rust/src/introspection.rs" \
    5525 \
    "v0.14.1" \
    "runtime introspection 聚合状态与诊断 schema；0.14.1 拆分观测模型"
add_legacy_file \
    "runtime/rust/src/supervisor.rs" \
    5113 \
    "v0.14.1" \
    "supervisor 编排与诊断历史聚合；0.14.1 拆分进程管理边界"
add_legacy_file \
    "crates/flowrt-ir/src/normalize/mod.rs" \
    4275 \
    "v0.14.1" \
    "Contract IR normalization 历史聚合；0.14.1 拆分 package、graph 和 backend normalize"
add_legacy_file \
    "crates/flowrt-cli/src/tests/echo_params_tests.rs" \
    4206 \
    "v0.14.1" \
    "CLI echo/params 历史回归聚合；0.14.1 拆分测试模块"
add_legacy_file \
    "crates/flowrt-codegen/src/cpp_shell.rs" \
    4157 \
    "v0.14.1" \
    "C++ runtime shell codegen 历史聚合；0.14.1 按 task、backend 和 manifest 拆分"
add_legacy_file \
    "crates/flowrt-cli/src/tests/workspace_tests.rs" \
    3948 \
    "v0.14.1" \
    "workspace CLI 回归聚合；0.14.1 拆分 init、add 和 prepare 用例"
add_legacy_file \
    "runtime/cpp/include/flowrt/introspection.hpp" \
    3812 \
    "v0.14.1" \
    "C++ introspection 公开头历史聚合；0.14.1 拆出 diagnostics 与 status POD 边界"
add_legacy_file \
    "crates/flowrt-cli/src/introspection.rs" \
    3405 \
    "v0.14.1" \
    "CLI live status/introspection 历史聚合；0.14.1 拆分输出格式和 socket 读取"
add_legacy_file \
    "scripts/check-release-readiness.sh" \
    1500 \
    "v0.14.2" \
    "历史发布门禁聚合脚本；0.14.2 前拆出 focused gate registry"

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

is_tracked_file() {
    git ls-files --error-unmatch -- "$1" >/dev/null 2>&1
}

cd "$repo_root"

info "源码规模阈值：Rust=${rust_line_limit} 行，C/C++=${cpp_line_limit} 行，shell=${shell_line_limit} 行"

while IFS= read -r -d '' path; do
    metadata="$(file_kind_and_limit "$path")"
    kind="${metadata%%|*}"
    limit="${metadata#*|}"
    lines="$(line_count_for "$path")"

    if [[ "$lines" -le "$limit" ]]; then
        continue
    fi

    if [[ -n "${legacy_max_lines[$path]:-}" ]]; then
        legacy_count=$((legacy_count + 1))
        legacy_limit="${legacy_max_lines[$path]}"
        if [[ "$lines" -gt "$legacy_limit" ]]; then
            fail "$path 为 legacy 大文件，当前 ${lines} 行超过 allowlist 上限 ${legacy_limit} 行；基础阈值 ${limit} 行；目标版本 ${legacy_target_versions[$path]}；原因：${legacy_reasons[$path]}"
        else
            printf '允许 legacy 大文件: %s (%s，%s 行 > 阈值 %s 行，上限 %s 行；目标版本 %s；原因：%s)\n' \
                "$path" \
                "$kind" \
                "$lines" \
                "$limit" \
                "$legacy_limit" \
                "${legacy_target_versions[$path]}" \
                "${legacy_reasons[$path]}"
        fi
    else
        fail "$path 为 ${kind}，当前 ${lines} 行超过阈值 ${limit} 行；请拆分文件，或在脚本 allowlist 中写明中文理由、上限和目标版本"
    fi
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
        '*.sh'
)

for path in "${!legacy_max_lines[@]}"; do
    if ! is_tracked_file "$path"; then
        fail "allowlist 包含已不存在或未被 git tracked 的文件: $path；请移除该条目"
        continue
    fi

    metadata="$(file_kind_and_limit "$path")"
    limit="${metadata#*|}"
    lines="$(line_count_for "$path")"
    if [[ "$lines" -le "$limit" ]]; then
        fail "$path 已降至 ${lines} 行，不再超过阈值 ${limit} 行；请从 architecture allowlist 移除"
    fi
done

if [[ "$errors" -gt 0 ]]; then
    printf '架构规模检查失败：发现 %d 个问题。\n' "$errors" >&2
    exit 1
fi

printf '架构规模检查通过：%d 个 legacy 大文件仍在显式上限内。\n' "$legacy_count"
