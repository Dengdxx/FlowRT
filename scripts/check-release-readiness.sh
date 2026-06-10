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
#   5. v0.5.0 focused CI gate 是否覆盖关键运行时能力
#   6. v0.6.0 focused CI gate 是否覆盖 Operation 和 record-only 主线
#   7. v0.7.0 focused CI gate 是否覆盖 external process、bundle/deploy 和安装后 smoke
#   8. v0.8.0 focused CI gate 是否覆盖 I/O boundary、variable frame、FrameDescriptor、
#      ROS2 typed bridge、diagnostics 和安装后 smoke
#   9. v0.8.1 focused CI gate 是否覆盖标准 FrameDescriptor 示例、echo、record、
#      安装后 smoke 和 microbench
#   10. v0.8.3 focused CI gate 是否覆盖 amd64 host 到 arm64 target 的完整交叉编译、
#       完整 target SDK layout smoke、安装后真实 cross smoke 和 package/release 依赖
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

first_match() {
    local pattern="$1"
    local file="$2"
    grep -oP "$pattern" "$file" | sed -n '1p' || true
}

first_line_number() {
    local pattern="$1"
    local file="$2"
    grep -nP "$pattern" "$file" | sed -n '1s/:.*//p' || true
}

require_ci_text() {
    local label="$1"
    local needle="$2"
    local file="$3"

    if grep -qF -- "$needle" "$file"; then
        pass "$label"
    else
        fail "$label: CI 缺少 '$needle'"
    fi
}

require_ci_text_count_at_least() {
    local label="$1"
    local needle="$2"
    local file="$3"
    local min_count="$4"
    local count

    count="$(grep -F -- "$needle" "$file" | wc -l | tr -d ' ')"
    if [[ "$count" -ge "$min_count" ]]; then
        pass "$label"
    else
        fail "$label: CI 中 '$needle' 出现 $count 次，期望至少 $min_count 次"
    fi
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

expected_version="${expected_version#v}"

if [[ -z "$expected_version" ]]; then
    fail "无法从 Cargo.toml 读取 workspace version，请传入 VERSION 参数"
    exit 1
fi

if ! [[ "$expected_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    fail "VERSION 必须是 X.Y.Z 或 vX.Y.Z，实际为: $expected_version"
    exit 1
fi

expected_tag="v${expected_version}"

printf '发布就绪检查: 版本 %s (tag %s)\n' "$expected_version" "$expected_tag"
printf '%s\n' '────────────────────────────────────────'

# ── 1. 版本来源一致性 ────────────────────────────────────────

printf '\n[1/12] 版本来源一致性\n'

check_version_in_file() {
    local label="$1"
    local file="$2"
    local pattern="$3"

    if [[ ! -f "$file" ]]; then
        fail "$label: 文件不存在 — $file"
        return
    fi

    local actual
    actual="$(first_match "$pattern" "$file")"
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

# Cargo.lock: 检查所有本仓库 flowrt* 包的版本，避免新增 workspace member 后漏检。
cargo_lock_mismatches="$(
    awk -v expected="$expected_version" '
        /^\[\[package\]\]/ {
            name = "";
            next;
        }
        /^name = "/ {
            name = $3;
            gsub(/"/, "", name);
            next;
        }
        /^version = "/ {
            version = $3;
            gsub(/"/, "", version);
            if (name ~ /^flowrt/ && version != expected) {
                printf "%s=%s\n", name, version;
            }
        }
    ' "$repo_root/Cargo.lock"
)"
if [[ -z "$cargo_lock_mismatches" ]]; then
    pass "Cargo.lock (flowrt*) = $expected_version"
else
    fail "Cargo.lock: flowrt* 版本不一致 — 期望 $expected_version，实际: $cargo_lock_mismatches"
fi

# ── 2. CHANGELOG 版本段格式 ──────────────────────────────────

printf '\n[2/12] CHANGELOG.md 版本段格式\n'

changelog_heading="## ${expected_tag} -"
if grep -qF "$changelog_heading" "$repo_root/CHANGELOG.md"; then
    pass "CHANGELOG.md 包含版本段 '$changelog_heading ...'"

    # 检查日期格式
    heading_line="$(grep -F "$changelog_heading" "$repo_root/CHANGELOG.md" | sed -n '1p' || true)"
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
unreleased_line="$(first_line_number '^## 未发布' "$repo_root/CHANGELOG.md")"
version_line="$(first_line_number "^## ${expected_tag}" "$repo_root/CHANGELOG.md")"
if [[ -n "$unreleased_line" && -n "$version_line" ]]; then
    if [[ "$unreleased_line" -gt "$version_line" ]]; then
        fail "'## 未发布' 段位于版本段之后（行 $unreleased_line > 行 $version_line），应在版本段之前"
    else
        pass "'## 未发布' 段位于版本段之前"
    fi
fi

# ── 3. Release notes 抽取 ────────────────────────────────────

printf '\n[3/12] Release notes 抽取\n'

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

printf '\n[4/12] CI release job 版本校验覆盖\n'

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
    if grep -q 'cargo_lock_mismatches' "$ci_file" && grep -qF 'flowrt* packages' "$ci_file"; then
        pass "release job 包含 Cargo.lock flowrt* 版本校验"
    else
        fail "release job 缺少 Cargo.lock flowrt* 版本校验"
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

# ── 5. v0.5.0 focused CI gate 覆盖 ───────────────────────────

printf '\n[5/12] v0.5.0 focused CI gate 覆盖\n'

if [[ ! -f "$ci_file" ]]; then
    fail "CI 配置不存在，无法检查 v0.5.0 focused gate"
else
    require_ci_text "CI 包含 v0.5.0 runtime smoke job" "v050-runtime-smoke:" "$ci_file"
    require_ci_text "v0.5.0 gate 覆盖 arm64 runner" "runner: ubuntu-24.04-arm" "$ci_file"
    require_ci_text "v0.5.0 gate 运行 supervisor readiness 测试" \
        "cargo test -p flowrt-cli build_runtime_tests -j1" "$ci_file"
    require_ci_text "v0.5.0 gate 运行 launch manifest 测试" \
        "cargo test -p flowrt-codegen launch -j1" "$ci_file"
    require_ci_text "v0.5.0 gate 运行 runtime supervisor 测试" \
        "cargo test -p flowrt supervisor -j1" "$ci_file"
    require_ci_text "v0.5.0 gate 运行远程参数 CLI 测试" \
        "cargo test -p flowrt-cli echo_params_tests -j1" "$ci_file"
    require_ci_text "v0.5.0 gate 运行 status/hz 健康展示测试" \
        "cargo test -p flowrt-cli selfdesc_status_hz_tests -j1" "$ci_file"
    require_ci_text "v0.5.0 gate 运行参数 codegen 测试" \
        "cargo test -p flowrt-codegen params -j1" "$ci_file"
    require_ci_text "v0.5.0 gate 运行 scheduler health codegen 测试" \
        "cargo test -p flowrt-codegen tasks -j1" "$ci_file"
    require_ci_text "v0.5.0 gate 运行 runtime introspection 测试" \
        "cargo test -p flowrt introspection -j1" "$ci_file"
    require_ci_text "package job 依赖 v0.5.0 focused gate" \
        "- v050-runtime-smoke" "$ci_file"
fi

# ── 6. v0.6.0 focused CI gate 覆盖 ───────────────────────────

printf '\n[6/12] v0.6.0 focused CI gate 覆盖\n'

if [[ ! -f "$ci_file" ]]; then
    fail "CI 配置不存在，无法检查 v0.6.0 focused gate"
else
    require_ci_text "CI 包含 v0.6.0 runtime smoke job" "v060-runtime-smoke:" "$ci_file"
    require_ci_text "v0.6.0 gate 覆盖 arm64 runner" "runner: ubuntu-24.04-arm" "$ci_file"
    require_ci_text "v0.6.0 gate 运行 Operation RSDL 测试" \
        "cargo test -p flowrt-rsdl operation -j1" "$ci_file"
    require_ci_text "v0.6.0 gate 运行 Operation IR 测试" \
        "cargo test -p flowrt-ir operation -j1" "$ci_file"
    require_ci_text "v0.6.0 gate 运行 Operation validator 测试" \
        "cargo test -p flowrt-validate operation -j1" "$ci_file"
    require_ci_text "v0.6.0 gate 运行 Operation codegen 测试" \
        "cargo test -p flowrt-codegen operation -j1" "$ci_file"
    require_ci_text "v0.6.0 gate 运行 Operation runtime 测试" \
        "cargo test -p flowrt operation -j1" "$ci_file"
    require_ci_text "v0.6.0 gate 运行 Operation CLI 测试" \
        "cargo test -p flowrt-cli operation -j1" "$ci_file"
    require_ci_text "v0.6.0 gate 运行 status 自描述测试" \
        "cargo test -p flowrt-cli selfdesc_status_hz_tests -j1" "$ci_file"
    require_ci_text "v0.6.0 gate 运行 record crate 测试" \
        "cargo test -p flowrt-record -j1" "$ci_file"
    require_ci_text "v0.6.0 gate 运行 record CLI 测试" \
        "cargo test -p flowrt-cli record -j1" "$ci_file"
    require_ci_text "v0.6.0 gate 运行 runtime introspection 测试" \
        "cargo test -p flowrt introspection -j1" "$ci_file"
    require_ci_text "package job 依赖 v0.6.0 focused gate" \
        "- v060-runtime-smoke" "$ci_file"
    require_ci_text "demo smoke 运行 v0.6.0 安装后 smoke" \
        "scripts/test-v060-installed-smoke.sh" "$ci_file"
fi

# ── 7. v0.7.0 focused CI gate 覆盖 ───────────────────────────

printf '\n[7/12] v0.7.0 focused CI gate 覆盖\n'

if [[ ! -f "$ci_file" ]]; then
    fail "CI 配置不存在，无法检查 v0.7.0 focused gate"
else
    require_ci_text "CI 包含 v0.7.0 external/deploy smoke job" "v070-runtime-smoke:" "$ci_file"
    require_ci_text "v0.7.0 gate 覆盖 arm64 runner" "runner: ubuntu-24.04-arm" "$ci_file"
    require_ci_text "v0.7.0 gate 运行 external RSDL 测试" \
        "cargo test -p flowrt-rsdl external_process -j1" "$ci_file"
    require_ci_text "v0.7.0 gate 运行 external IR 测试" \
        "cargo test -p flowrt-ir external_ -j1" "$ci_file"
    require_ci_text "v0.7.0 gate 运行 external validator 测试" \
        "cargo test -p flowrt-validate external_ -j1" "$ci_file"
    require_ci_text "v0.7.0 gate 运行 external codegen 测试" \
        "cargo test -p flowrt-codegen external_process -j1" "$ci_file"
    require_ci_text "v0.7.0 gate 运行 runtime external supervisor 测试" \
        "cargo test -p flowrt --lib external_ -j1" "$ci_file"
    require_ci_text "v0.7.0 gate 运行 bundle CLI 测试" \
        "cargo test -p flowrt-cli bundle -j1" "$ci_file"
    require_ci_text "v0.7.0 gate 运行 deploy CLI 测试" \
        "cargo test -p flowrt-cli deploy -j1" "$ci_file"
    require_ci_text "package job 依赖 v0.7.0 focused gate" \
        "- v070-runtime-smoke" "$ci_file"
    require_ci_text "demo smoke 运行 v0.7.0 安装后 smoke" \
        "scripts/test-v070-installed-smoke.sh" "$ci_file"
fi

# ── 8. v0.8.0 focused CI gate 覆盖 ───────────────────────────

printf '\n[8/12] v0.8.0 focused CI gate 覆盖\n'

if [[ ! -f "$ci_file" ]]; then
    fail "CI 配置不存在，无法检查 v0.8.0 focused gate"
else
    require_ci_text "CI 包含 v0.8.0 integration smoke job" "v080-runtime-smoke:" "$ci_file"
    require_ci_text "v0.8.0 gate 覆盖 arm64 runner" "runner: ubuntu-24.04-arm" "$ci_file"
    require_ci_text "v0.8.0 gate 运行 I/O boundary RSDL 测试" \
        "cargo test -p flowrt-rsdl io_boundary -j1" "$ci_file"
    require_ci_text "v0.8.0 gate 运行 ROS2 bridge RSDL 测试" \
        "cargo test -p flowrt-rsdl ros2 -j1" "$ci_file"
    require_ci_text "v0.8.0 gate 运行 FrameDescriptor IR 测试" \
        "cargo test -p flowrt-ir frame_descriptor -j1" "$ci_file"
    require_ci_text "v0.8.0 gate 运行 ROS2 bridge IR 测试" \
        "cargo test -p flowrt-ir ros2 -j1" "$ci_file"
    require_ci_text "v0.8.0 gate 运行 I/O boundary validator 测试" \
        "cargo test -p flowrt-validate io_boundary -j1" "$ci_file"
    require_ci_text "v0.8.0 gate 运行 ROS2 bridge validator 测试" \
        "cargo test -p flowrt-validate ros2 -j1" "$ci_file"
    require_ci_text "v0.8.0 gate 运行 message codegen 测试" \
        "cargo test -p flowrt-codegen message -j1" "$ci_file"
    require_ci_text "v0.8.0 gate 运行 launch codegen 测试" \
        "cargo test -p flowrt-codegen launch -j1" "$ci_file"
    require_ci_text "v0.8.0 gate 运行 FrameDescriptor codegen 测试" \
        "cargo test -p flowrt-codegen frame_descriptor -j1" "$ci_file"
    require_ci_text "v0.8.0 gate 运行 ROS2 bridge codegen 测试" \
        "cargo test -p flowrt-codegen ros2_bridge -j1" "$ci_file"
    require_ci_text "v0.8.0 gate 运行 diagnostics codegen 测试" \
        "cargo test -p flowrt-codegen introspection -j1" "$ci_file"
    require_ci_text "v0.8.0 gate 运行 FrameDescriptor runtime 测试" \
        "cargo test -p flowrt frame_descriptor -j1" "$ci_file"
    require_ci_text "v0.8.0 gate 运行 runtime introspection 测试" \
        "cargo test -p flowrt introspection -j1" "$ci_file"
    require_ci_text "v0.8.0 gate 运行 descriptor record 测试" \
        "cargo test -p flowrt-record descriptor -j1" "$ci_file"
    require_ci_text "v0.8.0 gate 运行 bundle CLI 测试" \
        "cargo test -p flowrt-cli bundle -j1" "$ci_file"
    require_ci_text "v0.8.0 gate 运行 deploy CLI 测试" \
        "cargo test -p flowrt-cli deploy -j1" "$ci_file"
    require_ci_text "v0.8.0 gate 运行 status/hz diagnostics 测试" \
        "cargo test -p flowrt-cli selfdesc_status_hz_tests -j1" "$ci_file"
    require_ci_text_count_at_least "package/release 依赖 v0.8.0 focused gate" \
        "- v080-runtime-smoke" "$ci_file" 2
    require_ci_text "demo smoke 运行 v0.8.0 安装后 smoke" \
        "scripts/test-v080-installed-smoke.sh" "$ci_file"
fi

# ── 9. v0.8.1 focused CI gate 覆盖 ───────────────────────────

printf '\n[9/12] v0.8.1 focused CI gate 覆盖\n'

if [[ ! -f "$ci_file" ]]; then
    fail "CI 配置不存在，无法检查 v0.8.1 focused gate"
else
    require_ci_text "CI 包含 v0.8.1 frame descriptor smoke job" "v081-runtime-smoke:" "$ci_file"
    require_ci_text "v0.8.1 gate 覆盖 arm64 runner" "runner: ubuntu-24.04-arm" "$ci_file"
    require_ci_text "v0.8.1 gate 运行 frame descriptor demo codegen smoke" \
        "cargo test -p flowrt-codegen frame_descriptor_demo_example_codegen_smoke -j1" "$ci_file"
    require_ci_text "v0.8.1 gate 运行 echo frame descriptor 格式化测试" \
        "cargo test -p flowrt-cli echo_formats_standard_frame_descriptor_payload_structurally -j1" "$ci_file"
    require_ci_text "v0.8.1 gate 运行 live status descriptor schema 测试" \
        "cargo test -p flowrt-cli live_status_summary_enriches_io_boundary_resource_descriptor_schema -j1" "$ci_file"
    require_ci_text "v0.8.1 gate 运行 record fake runtime 测试" \
        "cargo test -p flowrt-cli record_writes_mcap_from_fake_runtime -j1" "$ci_file"
    require_ci_text "v0.8.1 gate 运行 frame descriptor microbench" \
        "scripts/bench-frame-descriptor.sh" "$ci_file"
    require_ci_text_count_at_least "package/release 依赖 v0.8.1 focused gate" \
        "- v081-runtime-smoke" "$ci_file" 2
    require_ci_text "demo smoke 运行 v0.8.1 安装后 smoke" \
        "scripts/test-v081-installed-smoke.sh" "$ci_file"
fi

installed_smoke="$repo_root/scripts/test-v081-installed-smoke.sh"
if [[ -x "$installed_smoke" ]]; then
    pass "v0.8.1 安装后 smoke 脚本存在且可执行"
else
    fail "v0.8.1 安装后 smoke 脚本不存在或不可执行: $installed_smoke"
fi

bench_script="$repo_root/scripts/bench-frame-descriptor.sh"
if [[ -x "$bench_script" ]]; then
    pass "FrameDescriptor microbench 脚本存在且可执行"
else
    fail "FrameDescriptor microbench 脚本不存在或不可执行: $bench_script"
fi

# ── 10. v0.8.3 focused CI gate 覆盖 ──────────────────────────

printf '\n[10/12] v0.8.3 交叉编译 focused CI gate 覆盖\n'

if [[ ! -f "$ci_file" ]]; then
    fail "CI 配置不存在，无法检查 v0.8.3 focused gate"
else
    require_ci_text "CI 包含 v0.8.3 toolchain smoke job" \
        "v083-toolchain-smoke:" "$ci_file"
    require_ci_text "CI 包含 v0.8.3 installed cross smoke job" \
        "v083-cross-compile-smoke:" "$ci_file"
    require_ci_text "v0.8.3 gate 固定 amd64 host runner" \
        "runs-on: ubuntu-latest" "$ci_file"
    require_ci_text "v0.8.3 gate 覆盖 linux-arm64 target" \
        "linux-arm64" "$ci_file"
    require_ci_text "v0.8.3 gate 安装 Rust arm64 target" \
        "rustup target add aarch64-unknown-linux-gnu" "$ci_file"
    require_ci_text "v0.8.3 gate 安装 C/C++ 交叉编译器" \
        "gcc-aarch64-linux-gnu" "$ci_file"
    require_ci_text "v0.8.3 gate 安装 aarch64 g++" \
        "g++-aarch64-linux-gnu" "$ci_file"
    require_ci_text "v0.8.3 gate 安装 pkg-config" \
        "pkg-config" "$ci_file"
    require_ci_text "v0.8.3 gate 运行 toolchain focused tests" \
        "cargo test -p flowrt-cli toolchain_tests -j1" "$ci_file"
    require_ci_text "v0.8.3 gate 运行 build model focused tests" \
        "cargo test -p flowrt-cli build_runtime_tests -j1" "$ci_file"
    require_ci_text "v0.8.3 gate 运行 command focused tests" \
        "cargo test -p flowrt-cli command_tests -j1" "$ci_file"
    require_ci_text "v0.8.3 gate 运行 CMake target SDK focused tests" \
        "cargo test -p flowrt-cli cmake_ -j1" "$ci_file"
    require_ci_text "v0.8.3 gate 运行安装版真实 cross smoke" \
        "scripts/test-v083-installed-smoke.sh" "$ci_file"
    require_ci_text "v0.8.3 gate 验证 arm64 ELF" \
        "readelf" "$ci_file"
    require_ci_text "target SDK layout smoke 入 CI" \
        "scripts/test-deb-target-sdk-layout.sh" "$ci_file"
    require_ci_text "demo smoke 运行 v0.8.3 安装后 smoke" \
        "scripts/test-v083-installed-smoke.sh" "$ci_file"
    require_ci_text_count_at_least "package/release 依赖 v0.8.3 toolchain gate" \
        "- v083-toolchain-smoke" "$ci_file" 2
    require_ci_text "release 依赖 v0.8.3 installed cross gate" \
        "- v083-cross-compile-smoke" "$ci_file"
fi

target_sdk_layout_smoke="$repo_root/scripts/test-deb-target-sdk-layout.sh"
if [[ -x "$target_sdk_layout_smoke" ]]; then
    pass "target SDK layout smoke 脚本存在且可执行"
else
    fail "target SDK layout smoke 脚本不存在或不可执行: $target_sdk_layout_smoke"
fi

installed_v083_smoke="$repo_root/scripts/test-v083-installed-smoke.sh"
if [[ -x "$installed_v083_smoke" ]]; then
    pass "v0.8.3 安装后 smoke 脚本存在且可执行"
else
    fail "v0.8.3 安装后 smoke 脚本不存在或不可执行: $installed_v083_smoke"
fi

# ── 11. README 安装示例版本 ──────────────────────────────────

printf '\n[11/12] README.md 安装示例\n'

readme_file="$repo_root/README.md"
if [[ -f "$readme_file" ]]; then
    readme_version="$(first_match '(?<=^version=v)[0-9]+\.[0-9]+\.[0-9]+' "$readme_file")"
    if [[ -z "$readme_version" ]]; then
        readme_version="$(first_match '(?<=^version=)[0-9]+\.[0-9]+\.[0-9]+' "$readme_file")"
    fi
    readme_match="$(first_match 'flowrt_[0-9]+\.[0-9]+\.[0-9]+_amd64\.deb' "$readme_file")"
    if [[ -z "$readme_version" && -n "$readme_match" ]]; then
        readme_version="$(grep -oP '[0-9]+\.[0-9]+\.[0-9]+' <<<"$readme_match" | head -1)"
    fi
    if [[ "$readme_version" == "$expected_version" ]]; then
        pass "README.md 安装示例版本 = $readme_version"
    elif [[ -z "$readme_version" ]]; then
        info "README.md 中未找到版本化的 deb 文件名（可能是正常模板）"
    else
        fail "README.md 安装示例版本 = $readme_version，期望 $expected_version"
    fi
fi

# ── 12. Tag 与版本匹配（运行时检测） ────────────────────────

printf '\n[12/12] Git tag 检查\n'

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
