#!/usr/bin/env bash
# 版本一致性和发布就绪检查脚本
#
# 用法：scripts/check-release-readiness.sh [VERSION]
#
# 不传 VERSION 时从根 Cargo.toml 读取 workspace version。
# 检查项：
#   1. 所有版本来源是否一致（Cargo.toml、runtime/rust/Cargo.toml、
#      runtime/cpp/CMakeLists.txt、Cargo.lock、CHANGELOG.md、README.md）
#   2. CHANGELOG.md 对应版本段是否存在且格式正确
#   3. release notes 是否可以抽取且非空
#   4. CI release job 版本校验是否覆盖全部版本来源
#
# 任何检查失败都会给出清晰错误信息并以非零状态退出。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
errors=0

warn() {
    printf '\033[33m警告: %s\033[0m\n' "$1" >&2
}

fail() {
    printf '\033[31m错误: %s\033[0m\n' "$1" >&2
    errors=$((errors + 1))
}

pass() {
    printf '\033[32m  ✓ %s\033[0m\n' "$1"
}

info() {
    printf '\033[36m  → %s\033[0m\n' "$1"
}

# ── 读取期望版本 ──────────────────────────────────────────────

expected_version="${1:-}"
if [[ -z "$expected_version" ]]; then
    expected_version="$(
        awk '
            $1 == "version" && $2 == "=" {
                gsub(/"/, "", $3);
                print $3;
                exit;
            }
        ' "$repo_root/Cargo.toml"
    )"
fi

if [[ -z "$expected_version" ]]; then
    fail "无法从 Cargo.toml 读取 workspace version，请传入 VERSION 参数"
    exit 1
fi

expected_tag="v${expected_version}"

printf '发布就绪检查: 版本 %s (tag %s)\n' "$expected_version" "$expected_tag"
printf '%s\n' '────────────────────────────────────────'

# ── 1. 版本来源一致性 ────────────────────────────────────────

printf '\n[1/6] 版本来源一致性\n'

check_version_in_file() {
    local label="$1"
    local file="$2"
    local pattern="$3"

    if [[ ! -f "$file" ]]; then
        fail "$label: 文件不存在 — $file"
        return
    fi

    local actual
    actual="$(grep -oP "$pattern" "$file" | head -1)"
    if [[ -z "$actual" ]]; then
        fail "$label: 未找到版本号（模式: $pattern）"
    elif [[ "$actual" != "$expected_version" ]]; then
        fail "$label: 版本不一致 — 期望 $expected_version，实际 $actual"
    else
        pass "$label = $actual"
    fi
}

check_version_in_file \
    "Cargo.toml (workspace)" \
    "$repo_root/Cargo.toml" \
    '(?<=^version = ")[0-9]+\.[0-9]+\.[0-9]+'

check_version_in_file \
    "runtime/rust/Cargo.toml" \
    "$repo_root/runtime/rust/Cargo.toml" \
    '(?<=^version = ")[0-9]+\.[0-9]+\.[0-9]+'

check_version_in_file \
    "runtime/cpp/CMakeLists.txt" \
    "$repo_root/runtime/cpp/CMakeLists.txt" \
    '(?<=FLOWRT_RUNTIME_CPP_VERSION ")[0-9]+\.[0-9]+\.[0-9]+'

# Cargo.lock: 检查 flowrt 包的版本
cargo_lock_version="$(
    awk '
        /^name = "flowrt"$/ { found = 1; next }
        found && /^version = "/ {
            gsub(/"/, "", $3);
            print $3;
            exit;
        }
    ' "$repo_root/Cargo.lock"
)"
if [[ "$cargo_lock_version" == "$expected_version" ]]; then
    pass "Cargo.lock (flowrt) = $cargo_lock_version"
else
    fail "Cargo.lock: flowrt 版本不一致 — 期望 $expected_version，实际 $cargo_lock_version"
fi

# ── 2. CHANGELOG 版本段格式 ──────────────────────────────────

printf '\n[2/6] CHANGELOG.md 版本段格式\n'

changelog_heading="## ${expected_tag} -"
if grep -qF "$changelog_heading" "$repo_root/CHANGELOG.md"; then
    pass "CHANGELOG.md 包含版本段 '$changelog_heading ...'"

    # 检查日期格式
    heading_line="$(grep -F "$changelog_heading" "$repo_root/CHANGELOG.md" | head -1)"
    if echo "$heading_line" | grep -qP '^## v[0-9]+\.[0-9]+\.[0-9]+ - [0-9]{4}-[0-9]{2}-[0-9]{2}$'; then
        pass "版本段格式正确: $heading_line"
    else
        warn "版本段格式不完全匹配 '## vX.Y.Z - YYYY-MM-DD': $heading_line"
    fi
else
    # 也检查不带日期的变体
    if grep -qP "^## ${expected_tag}(\s|$)" "$repo_root/CHANGELOG.md"; then
        warn "CHANGELOG.md 包含版本段但缺少日期: '## ${expected_tag}'"
        warn "格式应为 '## ${expected_tag} - YYYY-MM-DD'"
    else
        fail "CHANGELOG.md 缺少版本段 '## ${expected_tag} - ...'"
    fi
fi

# 检查"未发布"段不应在版本段之后
unreleased_line="$(grep -n '^## 未发布' "$repo_root/CHANGELOG.md" | head -1 | cut -d: -f1)"
version_line="$(grep -n "^## ${expected_tag}" "$repo_root/CHANGELOG.md" | head -1 | cut -d: -f1)"
if [[ -n "$unreleased_line" && -n "$version_line" ]]; then
    if [[ "$unreleased_line" -gt "$version_line" ]]; then
        fail "'## 未发布' 段位于版本段之后（行 $unreleased_line > 行 $version_line），应在版本段之前"
    else
        pass "'## 未发布' 段位于版本段之前"
    fi
fi

# ── 3. Release notes 抽取 ────────────────────────────────────

printf '\n[3/6] Release notes 抽取\n'

extract_script="$repo_root/scripts/extract-release-notes.sh"
if [[ ! -x "$extract_script" ]]; then
    fail "release notes 抽取脚本不存在或不可执行: $extract_script"
else
    notes="$("$extract_script" "$expected_tag" "$repo_root/CHANGELOG.md" 2>&1)" && notes_status=0 || notes_status=$?
    if [[ "$notes_status" -ne 0 ]]; then
        fail "release notes 抽取失败（退出码 $notes_status）: $notes"
    elif [[ -z "$notes" ]]; then
        fail "release notes 抽取结果为空"
    else
        line_count="$(echo "$notes" | wc -l | tr -d ' ')"
        pass "release notes 抽取成功（$line_count 行）"
    fi
fi

# ── 4. CI release job 版本校验覆盖 ───────────────────────────

printf '\n[4/6] CI release job 版本校验覆盖\n'

ci_file="$repo_root/.github/workflows/ci.yml"
if [[ ! -f "$ci_file" ]]; then
    fail "CI 配置不存在: $ci_file"
else
    # 检查 release job 是否校验了 tag 与 Cargo.toml 版本
    if grep -q 'cargo_version' "$ci_file"; then
        pass "release job 包含 tag vs Cargo.toml 版本校验"
    else
        fail "release job 缺少 tag vs Cargo.toml 版本校验"
    fi

    # 检查 release job 是否校验了 release notes 非空
    if grep -q 'release-notes' "$ci_file" && grep -q 'fail_on_unmatched_files' "$ci_file"; then
        pass "release job 包含 release notes 和 artifact 校验"
    else
        warn "release job 的 release notes 校验可能不完整"
    fi

    # 检查是否校验了 runtime/rust/Cargo.toml 版本
    if grep -q 'runtime/rust/Cargo.toml' "$ci_file"; then
        pass "release job 校验了 runtime/rust/Cargo.toml 版本"
    else
        warn "release job 未校验 runtime/rust/Cargo.toml 版本一致性"
    fi

    # 检查是否校验了 runtime/cpp/CMakeLists.txt 版本
    if grep -q 'CMakeLists.txt' "$ci_file" && grep -q 'FLOWRT_RUNTIME_CPP_VERSION' "$ci_file"; then
        pass "release job 校验了 CMakeLists.txt 版本"
    else
        warn "release job 未校验 runtime/cpp/CMakeLists.txt 版本一致性"
    fi
fi

# ── 5. README 安装示例版本 ───────────────────────────────────

printf '\n[5/6] README.md 安装示例\n'

readme_file="$repo_root/README.md"
if [[ -f "$readme_file" ]]; then
    readme_version="$(grep -oP 'flowrt_[0-9]+\.[0-9]+\.[0-9]+_amd64\.deb' "$readme_file" | head -1 | grep -oP '[0-9]+\.[0-9]+\.[0-9]+')"
    if [[ "$readme_version" == "$expected_version" ]]; then
        pass "README.md 安装示例版本 = $readme_version"
    elif [[ -z "$readme_version" ]]; then
        info "README.md 中未找到版本化的 deb 文件名（可能是正常模板）"
    else
        warn "README.md 安装示例版本 = $readme_version，期望 $expected_version"
    fi
fi

# ── 6. Tag 与版本匹配（运行时检测） ─────────────────────────

printf '\n[6/6] Git tag 检查\n'

if git -C "$repo_root" tag -l "$expected_tag" | grep -q .; then
    info "tag $expected_tag 已存在"
else
    info "tag $expected_tag 尚未创建（发布时由 CI 创建）"
fi

# ── 汇总 ─────────────────────────────────────────────────────

printf '\n%s\n' '────────────────────────────────────────'

if [[ "$errors" -gt 0 ]]; then
    printf '\033[31m发现 %d 个错误，请修复后再发布。\033[0m\n' "$errors"
    exit 1
else
    printf '\033[32m全部检查通过。\033[0m\n'
    exit 0
fi
