#!/usr/bin/env bash
# 版本一致性和发布就绪检查脚本
#
# 用法：scripts/check-release-readiness.sh [VERSION]
#
# 不传 VERSION 时从根 Cargo.toml 读取 workspace version。
# 检查项涵盖版本来源、CHANGELOG/release notes、release evidence、历代 focused
# gate、README/CONTEXT、tag release 门禁和 tag 状态。实际分节编号在
# 输出中维护；版本专项检查可拆到 scripts/release-readiness/ 下。
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

require_file_text() {
    local label="$1"
    local needle="$2"
    local file="$3"

    if grep -qF -- "$needle" "$file"; then
        pass "$label"
    else
        fail "$label: $file 缺少 '$needle'"
    fi
}

forbid_file_text() {
    local label="$1"
    local needle="$2"
    local file="$3"

    if grep -qF -- "$needle" "$file"; then
        fail "$label: $file 不应包含 '$needle'"
    else
        pass "$label"
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

printf '\n[release-gate] registry 合同校验\n'
if release_gate_registry_output="$(cargo run -q -p flowrt-devtools -- release-gate check-registry "$expected_version" 2>&1)"; then pass "$release_gate_registry_output"; else fail "release gate registry 校验失败: $release_gate_registry_output"; fi

# ── 1. 版本来源一致性 ────────────────────────────────────────

printf '\n[1/24] 版本来源一致性\n'

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

printf '\n[2/24] CHANGELOG.md 版本段格式\n'

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

printf '\n[3/24] Release notes 抽取\n'

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

# ── 4. Release evidence 版本校验覆盖 ────────────────────────

printf '\n[4/24] Release evidence 版本校验覆盖\n'

ci_file="$repo_root/.github/workflows/ci.yml"
release_workflow_file="$repo_root/.github/workflows/release.yml"
if [[ ! -f "$ci_file" ]]; then
    fail "CI 配置不存在: $ci_file"
else
    # release evidence gate 复用 readiness 脚本做版本来源一致性校验。
    if grep -qF 'scripts/check-release-readiness.sh "$version"' "$ci_file"; then
        pass "release evidence gate 运行 release readiness 版本校验"
    else
        fail "release evidence gate 缺少 release readiness 版本校验"
    fi
    if grep -qF 'deb_version="$(dpkg-deb -f "$deb" Version)"' "$ci_file"; then
        pass "release evidence gate 校验 deb 版本"
    else
        fail "release evidence gate 缺少 deb 版本校验"
    fi

    # 检查 release evidence 是否校验了 release notes 和 artifact。
    if grep -q 'release-notes' "$ci_file" && grep -qF 'flowrt-release-evidence' "$ci_file"; then
        pass "release evidence gate 包含 release notes 和 artifact 校验"
    else
        fail "release evidence gate 缺少 release notes 或 artifact 校验"
    fi
fi

if [[ ! -f "$release_workflow_file" ]]; then
    fail "release workflow 不存在: $release_workflow_file"
else
    if grep -qF 'release-evidence.json' "$release_workflow_file" &&
        grep -qF "'.version'" "$release_workflow_file" &&
        grep -qF "'.sha'" "$release_workflow_file" &&
        grep -qF "'.run_id'" "$release_workflow_file"; then
        pass "tag release 校验 release evidence 的 version/tag/sha/run_id"
    else
        fail "tag release 缺少 release evidence version/tag/sha/run_id 校验"
    fi
    if grep -qF 'sha256sum -c SHA256SUMS' "$release_workflow_file" &&
        grep -qF 'fail_on_unmatched_files: true' "$release_workflow_file"; then
        pass "tag release 校验校验和并要求发布文件存在"
    else
        fail "tag release 缺少校验和或 artifact 存在性校验"
    fi
fi

# ── 5. v0.5.0 focused CI gate 覆盖 ───────────────────────────

printf '\n[5/24] v0.5.0 focused CI gate 覆盖\n'

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

printf '\n[6/24] v0.6.0 focused CI gate 覆盖\n'

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

printf '\n[7/24] v0.7.0 focused CI gate 覆盖\n'

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

printf '\n[8/24] v0.8.0 focused CI gate 覆盖\n'

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

printf '\n[9/24] v0.8.1 focused CI gate 覆盖\n'

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

printf '\n[10/24] v0.8.3 交叉编译 focused CI gate 覆盖\n'

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

# ── 11. v0.8.6 focused CI gate 覆盖 ──────────────────────────

printf '\n[11/24] v0.8.6 交叉 UX focused CI gate 覆盖\n'

if [[ ! -f "$ci_file" ]]; then
    fail "CI 配置不存在，无法检查 v0.8.6 focused gate"
else
    require_ci_text "CI 包含 v0.8.6 cross UX SDK smoke job" \
        "v086-cross-sdk-smoke:" "$ci_file"
    require_ci_text "v0.8.6 gate 固定 amd64 host runner" \
        "runs-on: ubuntu-latest" "$ci_file"
    require_ci_text "v0.8.6 gate 依赖 package job" \
        "- package" "$ci_file"
    require_ci_text "v0.8.6 gate 安装 Rust arm64 target" \
        "rustup target add aarch64-unknown-linux-gnu" "$ci_file"
    require_ci_text "v0.8.6 gate 安装 C/C++ 交叉编译器" \
        "gcc-aarch64-linux-gnu" "$ci_file"
    require_ci_text "v0.8.6 gate 使用公开 SDK overlay cache" \
        ".flowrt-public-sdk/v086-arm64" "$ci_file"
    require_ci_text "v0.8.6 gate 使用 FlowRT deps cache" \
        "flowrt-deps-v086-cross-sdk" "$ci_file"
    require_ci_text "v0.8.6 gate 运行公开 SDK smoke 脚本" \
        "scripts/test-v086-cross-sdk-demos.sh" "$ci_file"
    require_ci_text "v0.8.6 gate 验证 arm64 ELF" \
        "readelf" "$ci_file"
    require_ci_text "release 依赖 v0.8.6 cross UX gate" \
        "- v086-cross-sdk-smoke" "$ci_file"
fi

installed_v086_smoke="$repo_root/scripts/test-v086-cross-sdk-demos.sh"
if [[ -x "$installed_v086_smoke" ]]; then
    pass "v0.8.6 cross UX smoke 脚本存在且可执行"
    require_file_text "v0.8.6 smoke 运行 toolchain init" \
        "toolchain init" "$installed_v086_smoke"
    require_file_text "v0.8.6 smoke 运行 toolchain show" \
        "toolchain show" "$installed_v086_smoke"
    require_file_text "v0.8.6 smoke 使用带 RSDL 的 doctor" \
        "doctor rsdl/robot.rsdl --target linux-arm64" "$installed_v086_smoke"
    require_file_text "v0.8.6 smoke 检查 pkg-config module found" \
        "status=found" "$installed_v086_smoke"
    require_file_text "v0.8.6 smoke 检查 build summary" \
        "build summary: target=linux-arm64 mode=release" "$installed_v086_smoke"
else
    fail "v0.8.6 cross UX smoke 脚本不存在或不可执行: $installed_v086_smoke"
fi

legacy_v085_smoke="$repo_root/scripts/test-v085-cross-sdk-demos.sh"
if [[ -x "$legacy_v085_smoke" ]]; then
    pass "v0.8.5 兼容 smoke 入口存在且可执行"
else
    fail "v0.8.5 兼容 smoke 入口不存在或不可执行: $legacy_v085_smoke"
fi

# ── 12. v0.9.0 focused CI gate 覆盖 ──────────────────────────

printf '\n[12/24] v0.9.0 Island focused CI gate 覆盖\n'

if [[ ! -f "$ci_file" ]]; then
    fail "CI 配置不存在，无法检查 v0.9.0 focused gate"
else
    require_ci_text "CI 包含 v0.9.0 island smoke job" \
        "v090-island-smoke:" "$ci_file"
    require_ci_text "v0.9.0 gate 覆盖 amd64" \
        "arch: amd64" "$ci_file"
    require_ci_text "v0.9.0 gate 覆盖 arm64" \
        "arch: arm64" "$ci_file"
    require_ci_text "v0.9.0 gate 使用 FlowRT island cache" \
        ".flowrt-cache/v090-island" "$ci_file"
    require_ci_text "v0.9.0 gate 按 runner 架构选择 island target" \
        "FLOWRT_SMOKE_TARGET_PLATFORM=\"linux-\${{ matrix.arch }}\"" "$ci_file"
    require_ci_text "v0.9.0 gate 运行 island smoke 脚本" \
        "scripts/test-v090-island-demo.sh" "$ci_file"
    require_ci_text "package 依赖 v0.9.0 island gate" \
        "- v090-island-smoke" "$ci_file"
fi

installed_v090_smoke="$repo_root/scripts/test-v090-island-demo.sh"
if [[ -x "$installed_v090_smoke" ]]; then
    pass "v0.9.0 island smoke 脚本存在且可执行"
    require_file_text "v0.9.0 smoke 支持 target platform override" \
        "FLOWRT_SMOKE_TARGET_PLATFORM" "$installed_v090_smoke"
    require_file_text "v0.9.0 smoke 运行 island demo build" \
        "build --launcher" "$installed_v090_smoke"
    require_file_text "v0.9.0 smoke 运行 runtime" \
        "run \"\$demo_dir/rsdl/robot.rsdl\" --process main" "$installed_v090_smoke"
    require_file_text "v0.9.0 smoke 使用 flowrt pub 注入" \
        "pub sample_in" "$installed_v090_smoke"
    require_file_text "v0.9.0 smoke 使用 flowrt echo 观察" \
        "echo result_out" "$installed_v090_smoke"
    require_file_text "v0.9.0 smoke 校验输出字段" \
        "doubled=42" "$installed_v090_smoke"
else
    fail "v0.9.0 island smoke 脚本不存在或不可执行: $installed_v090_smoke"
fi

# ── 13. v0.9.1 focused CI gate 覆盖 ──────────────────────────

printf '\n[13/24] v0.9.1 Island tooling focused CI gate 覆盖\n'

if [[ ! -f "$ci_file" ]]; then
    fail "CI 配置不存在，无法检查 v0.9.1 focused gate"
else
    require_ci_text "CI 包含 v0.9.1 island tooling smoke job" \
        "v091-island-tooling-smoke:" "$ci_file"
    require_ci_text "v0.9.1 gate 覆盖 amd64" \
        "arch: amd64" "$ci_file"
    require_ci_text "v0.9.1 gate 覆盖 arm64" \
        "arch: arm64" "$ci_file"
    require_ci_text "v0.9.1 gate 使用 FlowRT island cache" \
        ".flowrt-cache/v091-island" "$ci_file"
    require_ci_text "v0.9.1 gate 运行 params set --file focused tests" \
        "cargo test -p flowrt-cli params_set_file -j1" "$ci_file"
    require_ci_text "v0.9.1 gate 运行 flowrt pub focused tests" \
        "cargo test -p flowrt-cli pub_ -j1" "$ci_file"
    require_ci_text "v0.9.1 gate 运行多 channel echo focused tests" \
        "cargo test -p flowrt-cli echo_multiple_channels -j1" "$ci_file"
    require_ci_text "v0.9.1 gate 运行显式空消息 focused tests" \
        "cargo test -p flowrt-validate empty -j1" "$ci_file"
    require_ci_text "v0.9.1 gate 运行 variable frame island smoke 脚本" \
        "scripts/test-v091-variable-frame-island-demo.sh" "$ci_file"
    require_ci_text_count_at_least "package/release 依赖 v0.9.1 focused gate" \
        "- v091-island-tooling-smoke" "$ci_file" 2
fi

installed_v091_smoke="$repo_root/scripts/test-v091-variable-frame-island-demo.sh"
if [[ -x "$installed_v091_smoke" ]]; then
    pass "v0.9.1 variable frame island smoke 脚本存在且可执行"
    require_file_text "v0.9.1 smoke 支持 target platform override" \
        "FLOWRT_SMOKE_TARGET_PLATFORM" "$installed_v091_smoke"
    require_file_text "v0.9.1 smoke 运行 variable frame demo build" \
        "build --launcher" "$installed_v091_smoke"
    require_file_text "v0.9.1 smoke 运行 runtime" \
        "run \"\$demo_dir/rsdl/robot.rsdl\" --process main" "$installed_v091_smoke"
    require_file_text "v0.9.1 smoke 使用 flowrt pub --file" \
        "--file \"\$demo_dir/samples/scan.jsonl\"" "$installed_v091_smoke"
    require_file_text "v0.9.1 smoke 使用 flowrt pub --freq" \
        "--freq 1000" "$installed_v091_smoke"
    require_file_text "v0.9.1 smoke 使用 flowrt echo 观察" \
        "echo summary_out" "$installed_v091_smoke"
    require_file_text "v0.9.1 smoke 校验输出字段" \
        "mean_milli=1250" "$installed_v091_smoke"
else
    fail "v0.9.1 variable frame island smoke 脚本不存在或不可执行: $installed_v091_smoke"
fi

# ── 14. v0.9.2 focused CI gate 覆盖 ──────────────────────────

printf '\n[14/24] v0.9.2 Island offline validation focused CI gate 覆盖\n'

if [[ ! -f "$ci_file" ]]; then
    fail "CI 配置不存在，无法检查 v0.9.2 focused gate"
else
    require_ci_text "CI 包含 v0.9.2 island offline validation smoke job" \
        "v092-island-offline-validation-smoke:" "$ci_file"
    require_ci_text "v0.9.2 gate 覆盖 amd64" \
        "arch: amd64" "$ci_file"
    require_ci_text "v0.9.2 gate 覆盖 arm64" \
        "arch: arm64" "$ci_file"
    require_ci_text "v0.9.2 gate 使用 FlowRT island cache" \
        ".flowrt-cache/v092-island" "$ci_file"
    require_ci_text "v0.9.2 gate 运行 check signature focused tests" \
        "cargo test -p flowrt-cli check -j1" "$ci_file"
    require_ci_text "v0.9.2 gate 运行 replay focused tests" \
        "cargo test -p flowrt-cli replay -j1" "$ci_file"
    require_ci_text "v0.9.2 gate 运行 echo focused tests" \
        "cargo test -p flowrt-cli echo -j1" "$ci_file"
    require_ci_text "v0.9.2 gate 覆盖 Cargo app hash 隔离回归" \
        "cargo_internal_names_include_contract_hash_without_changing_public_names" "$ci_file"
    require_ci_text "v0.9.2 gate 运行 temporary island/codegen focused tests" \
        "cargo test -p flowrt-codegen island -j1" "$ci_file"
    require_ci_text "v0.9.2 gate 运行 replay smoke 脚本" \
        "scripts/test-v092-island-replay-demo.sh" "$ci_file"
    require_ci_text_count_at_least "package/release 依赖 v0.9.2 focused gate" \
        "- v092-island-offline-validation-smoke" "$ci_file" 2
fi

installed_v092_smoke="$repo_root/scripts/test-v092-island-replay-demo.sh"
if [[ -x "$installed_v092_smoke" ]]; then
    pass "v0.9.2 island replay smoke 脚本存在且可执行"
    require_file_text "v0.9.2 smoke 支持 target platform override" \
        "FLOWRT_SMOKE_TARGET_PLATFORM" "$installed_v092_smoke"
    require_file_text "v0.9.2 smoke 运行 flowrt check" \
        "flowrt check" "$installed_v092_smoke"
    require_file_text "v0.9.2 smoke 使用 temporary island overlay" \
        "--temporary-island" "$installed_v092_smoke"
    require_file_text "v0.9.2 smoke 声明 boundary input" \
        "--boundary-input scan_in=validator.scan" "$installed_v092_smoke"
    require_file_text "v0.9.2 smoke 声明 boundary output" \
        "--boundary-output summary_out=validator.summary" "$installed_v092_smoke"
    require_file_text "v0.9.2 smoke 运行 replay" \
        "replay" "$installed_v092_smoke"
    require_file_text "v0.9.2 smoke 校验多 boundary replay" \
        "boundaries=2" "$installed_v092_smoke"
    require_file_text "v0.9.2 smoke 校验 echo 默认摘要" \
        "sequence_summary(count=18" "$installed_v092_smoke"
    require_file_text "v0.9.2 smoke 校验 echo --raw" \
        "--raw" "$installed_v092_smoke"
    require_file_text "v0.9.2 smoke 校验 bundle gate" \
        "refusing to bundle temporary overlay" "$installed_v092_smoke"
    require_file_text "v0.9.2 smoke 校验 deploy gate" \
        "refusing to deploy island" "$installed_v092_smoke"
else
    fail "v0.9.2 island replay smoke 脚本不存在或不可执行: $installed_v092_smoke"
fi

# ── 15. v0.10.2 focused CI gate 覆盖 ─────────────────────────

printf '\n[15/24] v0.10.2 Concurrency focused CI gate 覆盖\n'

if [[ ! -f "$ci_file" ]]; then
    fail "CI 配置不存在，无法检查 v0.10.2 focused gate"
else
    require_ci_text "CI 包含 v0.10.2 concurrency smoke job" \
        "v0102-concurrency-smoke:" "$ci_file"
    require_ci_text "v0.10.2 gate 覆盖 amd64" \
        "arch: amd64" "$ci_file"
    require_ci_text "v0.10.2 gate 覆盖 arm64" \
        "arch: arm64" "$ci_file"
    require_ci_text "v0.10.2 gate 使用 FlowRT concurrency cache" \
        ".flowrt-cache/v0102-concurrency" "$ci_file"
    require_ci_text "v0.10.2 gate 按 runner 架构选择 smoke target" \
        "FLOWRT_SMOKE_TARGET_PLATFORM=\"linux-\${{ matrix.arch }}\"" "$ci_file"
    require_ci_text "v0.10.2 gate 运行 concurrency smoke 脚本" \
        "scripts/test-v0102-concurrency-smoke.sh" "$ci_file"
    require_ci_text_count_at_least "package/release 依赖 v0.10.2 focused gate" \
        "- v0102-concurrency-smoke" "$ci_file" 2
fi

installed_v0102_smoke="$repo_root/scripts/test-v0102-concurrency-smoke.sh"
if [[ -x "$installed_v0102_smoke" ]]; then
    pass "v0.10.2 concurrency smoke 脚本存在且可执行"
    require_file_text "v0.10.2 smoke 运行 codegen concurrency focused tests" \
        "cargo test -p flowrt-codegen concurrency -j1" "$installed_v0102_smoke"
    require_file_text "v0.10.2 smoke 运行 Rust iox2 generated shell tests" \
        "cargo test -p flowrt-codegen rust_iox2 -j1" "$installed_v0102_smoke"
    require_file_text "v0.10.2 smoke 运行 backend route tests" \
        "cargo test -p flowrt-codegen backend -j1" "$installed_v0102_smoke"
    require_file_text "v0.10.2 smoke 运行 runtime executor tests" \
        "cargo test -p flowrt executor -j1" "$installed_v0102_smoke"
    require_file_text "v0.10.2 smoke 运行 C++ runtime ctest" \
        "ctest --test-dir \"\$work_dir/cpp-runtime\" --output-on-failure" "$installed_v0102_smoke"
    require_file_text "v0.10.2 smoke 检查 generated Rust shell" \
        "cargo check" "$installed_v0102_smoke"
    require_file_text "v0.10.2 smoke 构建 generated C++ shell" \
        "cmake --build \"\$cpp_demo/flowrt/build/cmake-v0102-smoke\" -j1" "$installed_v0102_smoke"
else
    fail "v0.10.2 concurrency smoke 脚本不存在或不可执行: $installed_v0102_smoke"
fi

# ── 16. v0.12.0 focused CI gate 覆盖 ─────────────────────────

printf '\n[16/24] v0.12.0 Authoring focused CI gate 覆盖\n'

if [[ ! -f "$ci_file" ]]; then
    fail "CI 配置不存在，无法检查 v0.12.0 focused gate"
else
    require_ci_text "CI 包含 v0.12.0 authoring smoke job" \
        "v0120-authoring-smoke:" "$ci_file"
    require_ci_text "v0.12.0 gate 覆盖 amd64" \
        "arch: amd64" "$ci_file"
    require_ci_text "v0.12.0 gate 覆盖 arm64" \
        "arch: arm64" "$ci_file"
    require_ci_text "v0.12.0 gate 使用 FlowRT authoring cache" \
        ".flowrt-cache/v0120-authoring" "$ci_file"
    require_ci_text "v0.12.0 gate 按 runner 架构选择 smoke target" \
        "FLOWRT_SMOKE_TARGET_PLATFORM=\"linux-\${{ matrix.arch }}\"" "$ci_file"
    require_ci_text "v0.12.0 gate 运行 authoring smoke 脚本" \
        "scripts/test-v0120-authoring-smoke.sh" "$ci_file"
    require_ci_text_count_at_least "package/release 依赖 v0.12.0 focused gate" \
        "- v0120-authoring-smoke" "$ci_file" 2
fi

installed_v0120_smoke="$repo_root/scripts/test-v0120-authoring-smoke.sh"
if [[ -x "$installed_v0120_smoke" ]]; then
    pass "v0.12.0 authoring smoke 脚本存在且可执行"
    require_file_text "v0.12.0 smoke 覆盖 flowrt init Rust" \
        'exercise_authoring_project rust' "$installed_v0120_smoke"
    require_file_text "v0.12.0 smoke 覆盖 flowrt init C++" \
        'exercise_authoring_project cpp' "$installed_v0120_smoke"
    require_file_text "v0.12.0 smoke 覆盖 flowrt init C" \
        'exercise_authoring_project c' "$installed_v0120_smoke"
    require_file_text "v0.12.0 smoke 断言 init/add 不写用户 app" \
        'assert_no_user_app "$project"' "$installed_v0120_smoke"
    require_file_text "v0.12.0 smoke 覆盖 flowrt prepare" \
        'run_flowrt_at "$project" prepare' "$installed_v0120_smoke"
    require_file_text "v0.12.0 smoke 检查 App API manifest" \
        'flowrt/app/app_api.json' "$installed_v0120_smoke"
    require_file_text "v0.12.0 smoke 检查 implementation 清单" \
        'flowrt/app/implementation.md' "$installed_v0120_smoke"
    require_file_text "v0.12.0 smoke 检查 reference stubs" \
        'flowrt/app/stubs/$lang' "$installed_v0120_smoke"
    require_file_text "v0.12.0 smoke 覆盖 explain text" \
        'explain --format text' "$installed_v0120_smoke"
    require_file_text "v0.12.0 smoke 覆盖 explain json" \
        'explain --format json' "$installed_v0120_smoke"
    require_file_text "v0.12.0 smoke 覆盖 Rust demo build/run" \
        'exercise_demo import_demo' "$installed_v0120_smoke"
    require_file_text "v0.12.0 smoke 覆盖 C++ demo build/run" \
        'exercise_demo cpp_counter_demo' "$installed_v0120_smoke"
    require_file_text "v0.12.0 smoke 覆盖 C demo build/run" \
        'exercise_demo c_counter_demo' "$installed_v0120_smoke"
else
    fail "v0.12.0 authoring smoke 脚本不存在或不可执行: $installed_v0120_smoke"
fi

v0120_release_body="$(
    awk '
        /^## v0\.12\.0 - / {
            inside = 1;
            next;
        }
        inside && /^## / {
            exit;
        }
        inside {
            print;
        }
    ' "$repo_root/CHANGELOG.md"
)"
v0120_notes_source="CHANGELOG v0.12.0 版本段"
v0120_notes_body="$v0120_release_body"
if [[ -z "$v0120_notes_body" ]]; then
    v0120_notes_source="CHANGELOG 未发布段"
    v0120_notes_body="$(
    awk '
        /^## 未发布$/ {
            inside = 1;
            next;
        }
        inside && /^## / {
            exit;
        }
        inside {
            print;
        }
    ' "$repo_root/CHANGELOG.md"
)"
fi
if [[ -z "$v0120_notes_body" ]]; then
    fail "CHANGELOG.md 缺少可作为 v0.12.0 release notes 事实源的版本段或未发布段"
else
    if grep -qF 'Contract-driven App Authoring' <<<"$v0120_notes_body"; then
        pass "$v0120_notes_source 记录 Contract-driven App Authoring"
    else
        fail "$v0120_notes_source 缺少 Contract-driven App Authoring 条目"
    fi
    if grep -qF 'flowrt/app/app_api.json' <<<"$v0120_notes_body"; then
        pass "$v0120_notes_source 记录 App API manifest"
    else
        fail "$v0120_notes_source 缺少 App API manifest 说明"
    fi
    if grep -qF 'v0.12.0 Authoring Smoke' <<<"$v0120_notes_body"; then
        pass "$v0120_notes_source 记录 v0.12.0 authoring focused gate"
    else
        fail "$v0120_notes_source 缺少 v0.12.0 authoring focused gate 条目"
    fi
fi

# ── 17. v0.13.0 focused CI gate 覆盖 ─────────────────────────

printf '\n[17/24] v0.13.0 Robot Runtime Completion focused CI gate 覆盖\n'

if [[ ! -f "$ci_file" ]]; then
    fail "CI 配置不存在，无法检查 v0.13.0 focused gate"
else
    require_ci_text "CI 包含 v0.13.0 runtime completion smoke job" \
        "v0130-runtime-completion-smoke:" "$ci_file"
    require_ci_text "v0.13.0 gate 覆盖 amd64" \
        "arch: amd64" "$ci_file"
    require_ci_text "v0.13.0 gate 覆盖 arm64" \
        "arch: arm64" "$ci_file"
    require_ci_text "v0.13.0 gate 使用 FlowRT runtime cache" \
        ".flowrt-cache/v0130-runtime" "$ci_file"
    require_ci_text "v0.13.0 gate 按 runner 架构选择 smoke target" \
        "FLOWRT_SMOKE_TARGET_PLATFORM=\"linux-\${{ matrix.arch }}\"" "$ci_file"
    require_ci_text "v0.13.0 gate 运行 runtime completion smoke 脚本" \
        "scripts/test-v0130-runtime-completion-smoke.sh" "$ci_file"
    require_ci_text_count_at_least "package/release 依赖 v0.13.0 focused gate" \
        "- v0130-runtime-completion-smoke" "$ci_file" 2
fi

installed_v0130_smoke="$repo_root/scripts/test-v0130-runtime-completion-smoke.sh"
if [[ -x "$installed_v0130_smoke" ]]; then
    pass "v0.13.0 runtime completion smoke 脚本存在且可执行"
    require_file_text "v0.13.0 smoke 支持 target platform override" \
        "FLOWRT_SMOKE_TARGET_PLATFORM" "$installed_v0130_smoke"
    require_file_text "v0.13.0 smoke 覆盖 replay" \
        "cargo_test flowrt-cli replay" "$installed_v0130_smoke"
    require_file_text "v0.13.0 smoke 覆盖 temporary island overlay" \
        "cargo_test flowrt-cli temporary" "$installed_v0130_smoke"
    require_file_text "v0.13.0 smoke 覆盖 resource IR" \
        "cargo_test flowrt-ir resource" "$installed_v0130_smoke"
    require_file_text "v0.13.0 smoke 覆盖 resource validator" \
        "cargo_test flowrt-validate resource" "$installed_v0130_smoke"
    require_file_text "v0.13.0 smoke 覆盖 diagnostics/status" \
        "cargo_test flowrt-cli diagnostics" "$installed_v0130_smoke"
    require_file_text "v0.13.0 smoke 覆盖 deploy hardening" \
        "cargo_test flowrt-cli deploy" "$installed_v0130_smoke"
    require_file_text "v0.13.0 smoke 覆盖 doctor/cross" \
        "cargo_test flowrt-cli doctor" "$installed_v0130_smoke"
    require_file_text "v0.13.0 smoke 覆盖 C ABI" \
        "cargo_test flowrt abi" "$installed_v0130_smoke"
else
    fail "v0.13.0 runtime completion smoke 脚本不存在或不可执行: $installed_v0130_smoke"
fi

v0130_release_body="$(
    awk '
        /^## v0\.13\.0 - / {
            inside = 1;
            next;
        }
        inside && /^## / {
            exit;
        }
        inside {
            print;
        }
    ' "$repo_root/CHANGELOG.md"
)"
v0130_notes_source="CHANGELOG v0.13.0 版本段"
v0130_notes_body="$v0130_release_body"
if [[ -z "$v0130_notes_body" ]]; then
    v0130_notes_source="CHANGELOG 未发布段"
    v0130_notes_body="$(
    awk '
        /^## 未发布$/ {
            inside = 1;
            next;
        }
        inside && /^## / {
            exit;
        }
        inside {
            print;
        }
    ' "$repo_root/CHANGELOG.md"
)"
fi
if [[ -z "$v0130_notes_body" ]]; then
    fail "CHANGELOG.md 缺少可作为 v0.13.0 release notes 事实源的版本段或未发布段"
else
    if grep -qF 'C ABI 基础边界升级到 `0.2`' <<<"$v0130_notes_body"; then
        pass "$v0130_notes_source 记录 C ABI 0.2"
    else
        fail "$v0130_notes_source 缺少 C ABI 0.2 条目"
    fi
    if grep -qF 'resource requirement' <<<"$v0130_notes_body"; then
        pass "$v0130_notes_source 记录 resource contract"
    else
        fail "$v0130_notes_source 缺少 resource contract 条目"
    fi
    if grep -qF 'diagnostics_event' <<<"$v0130_notes_body"; then
        pass "$v0130_notes_source 记录 diagnostics_event"
    else
        fail "$v0130_notes_source 缺少 diagnostics_event 条目"
    fi
    if grep -qF 'Temporary island overlay' <<<"$v0130_notes_body"; then
        pass "$v0130_notes_source 记录 temporary overlay"
    else
        fail "$v0130_notes_source 缺少 temporary overlay 条目"
    fi
    if grep -qF 'v0.13.0 Robot Runtime Completion Smoke' <<<"$v0130_notes_body"; then
        pass "$v0130_notes_source 记录 v0.13.0 runtime completion focused gate"
    else
        fail "$v0130_notes_source 缺少 v0.13.0 runtime completion focused gate 条目"
    fi
fi

# ── 18. v0.14.0 focused CI gate 覆盖 ─────────────────────────

printf '\n[18/24] v0.14.0 Realtime Scheduler focused CI gate 覆盖\n'

if [[ ! -f "$ci_file" ]]; then
    fail "CI 配置不存在，无法检查 v0.14.0 focused gate"
else
    require_ci_text "CI 包含 v0.14.0 realtime scheduler smoke job" \
        "v0140-realtime-scheduler-smoke:" "$ci_file"
    require_ci_text "v0.14.0 gate 覆盖 amd64" \
        "arch: amd64" "$ci_file"
    require_ci_text "v0.14.0 gate 覆盖 arm64" \
        "arch: arm64" "$ci_file"
    require_ci_text "v0.14.0 gate 使用 FlowRT realtime cache" \
        ".flowrt-cache/v0140-realtime" "$ci_file"
    require_ci_text "v0.14.0 gate 按 runner 架构选择 smoke target" \
        "FLOWRT_SMOKE_TARGET_PLATFORM=\"linux-\${{ matrix.arch }}\"" "$ci_file"
    require_ci_text "v0.14.0 gate 运行 realtime scheduler smoke 脚本" \
        "scripts/test-v0140-realtime-scheduler-smoke.sh" "$ci_file"
    require_ci_text_count_at_least "package/release 依赖 v0.14.0 focused gate" \
        "- v0140-realtime-scheduler-smoke" "$ci_file" 2
fi

installed_v0140_smoke="$repo_root/scripts/test-v0140-realtime-scheduler-smoke.sh"
if [[ -x "$installed_v0140_smoke" ]]; then
    pass "v0.14.0 realtime scheduler smoke 脚本存在且可执行"
    require_file_text "v0.14.0 smoke 支持 target platform override" \
        "FLOWRT_SMOKE_TARGET_PLATFORM" "$installed_v0140_smoke"
    require_file_text "v0.14.0 smoke 覆盖 executor admission/completion" \
        "cargo_test flowrt executor" "$installed_v0140_smoke"
    require_file_text "v0.14.0 smoke 覆盖 generated scheduler" \
        "cargo_test flowrt-codegen tasks" "$installed_v0140_smoke"
    require_file_text "v0.14.0 smoke 覆盖 status timing" \
        "cargo_test flowrt-cli selfdesc_status" "$installed_v0140_smoke"
    require_file_text "v0.14.0 smoke 覆盖 introspection timing" \
        "cargo_test flowrt introspection" "$installed_v0140_smoke"
    require_file_text "v0.14.0 smoke 覆盖 C ABI layout" \
        "cargo_test flowrt abi" "$installed_v0140_smoke"
else
    fail "v0.14.0 realtime scheduler smoke 脚本不存在或不可执行: $installed_v0140_smoke"
fi

v0140_release_body="$(
    awk '
        /^## v0\.14\.0 - / {
            inside = 1;
            next;
        }
        inside && /^## / {
            exit;
        }
        inside {
            print;
        }
    ' "$repo_root/CHANGELOG.md"
)"
v0140_notes_source="CHANGELOG v0.14.0 版本段"
v0140_notes_body="$v0140_release_body"
if [[ -z "$v0140_notes_body" ]]; then
    v0140_notes_source="CHANGELOG 未发布段"
    v0140_notes_body="$(
    awk '
        /^## 未发布$/ {
            inside = 1;
            next;
        }
        inside && /^## / {
            exit;
        }
        inside {
            print;
        }
    ' "$repo_root/CHANGELOG.md"
)"
fi
if [[ -z "$v0140_notes_body" ]]; then
    fail "CHANGELOG.md 缺少可作为 v0.14.0 release notes 事实源的版本段或未发布段"
else
    if grep -qF 'task timing context' <<<"$v0140_notes_body"; then
        pass "$v0140_notes_source 记录 task timing context"
    else
        fail "$v0140_notes_source 缺少 task timing context 条目"
    fi
    if grep -qF 'WorkerCompletionQueue' <<<"$v0140_notes_body"; then
        pass "$v0140_notes_source 记录 completion queue"
    else
        fail "$v0140_notes_source 缺少 completion queue 条目"
    fi
    if grep -qF 'v0.14.0 Realtime Scheduler Smoke' <<<"$v0140_notes_body"; then
        pass "$v0140_notes_source 记录 v0.14.0 realtime scheduler focused gate"
    else
        fail "$v0140_notes_source 缺少 v0.14.0 realtime scheduler focused gate 条目"
    fi
fi

# ── 19. v0.14.1 focused CI gate 覆盖 ────────────────────────

printf '\n[19/24] v0.14.1 Architecture focused CI gate 覆盖\n'

source "$repo_root/scripts/release-readiness/v0141-architecture.sh"; check_v0141_architecture_readiness

# ── 20. v0.15.x focused CI gate 覆盖 ────────────────────────
printf '\n[20/24] v0.15.x focused CI gate 覆盖\n'
source "$repo_root/scripts/release-readiness/v0150-architecture-convergence.sh"; check_v0150_architecture_convergence_readiness
source "$repo_root/scripts/release-readiness/v0151-ci-release-evidence.sh"; check_v0151_ci_release_evidence_readiness

# ── 21. README 安装示例版本 ──────────────────────────────────

printf '\n[21/24] README.md 安装示例\n'

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

# ── 22. CONTEXT 当前状态版本 ────────────────────────────────

printf '\n[22/24] CONTEXT.md 当前状态版本\n'

context_file="$repo_root/CONTEXT.md"
if [[ -f "$context_file" ]]; then
    context_version="$(first_match '当前 workspace 版本(仍)?为 `\K[0-9]+\.[0-9]+\.[0-9]+' "$context_file")"
    if [[ "$context_version" == "$expected_version" ]]; then
        pass "CONTEXT.md 当前 workspace 版本 = $context_version"
    elif [[ -z "$context_version" ]]; then
        fail "CONTEXT.md 缺少 '当前 workspace 版本为 X.Y.Z' 状态行"
    else
        fail "CONTEXT.md 当前 workspace 版本 = $context_version，期望 $expected_version"
    fi
else
    fail "CONTEXT.md 不存在"
fi

# ── 23. Release evidence 门禁覆盖 ─────────────────────────

printf '\n[23/24] Release evidence 门禁覆盖\n'

pass "release evidence 门禁由 v0.15.1 专项 adapter 覆盖"

# ── 24. Tag 与版本匹配（运行时检测） ────────────────────────

printf '\n[24/24] Git tag 检查\n'

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
