# v0.23.2 C++ clang-tidy gate 的发布门禁接线检查。
# 本文件由 scripts/check-release-readiness.sh source，并复用其 pass/fail helper。

check_v0232_cpp_clang_tidy_readiness() {
    local ci_file="$repo_root/.github/workflows/release-candidate.yml"
    local registry_file="$repo_root/scripts/release-gates/registry.toml"
    local smoke_script="$repo_root/scripts/test-v0232-cpp-clang-tidy-smoke.sh"
    local static_quality_script="$repo_root/scripts/test-cpp-static-quality.sh"
    local clang_tidy_config="$repo_root/.clang-tidy"

    if [[ ! -f "$ci_file" ]]; then
        fail "CI 配置不存在，无法检查 v0.23.2 focused gate"
    else
        require_ci_text "CI 包含 v0.23.2 C++ clang-tidy smoke job" \
            "v0232-cpp-clang-tidy-smoke:" "$ci_file"
        require_ci_text "v0.23.2 gate 安装 clang-tidy" \
            "sudo apt-get install -y clang-tidy cmake ninja-build g++" "$ci_file"
        require_ci_text "v0.23.2 gate 通过 devtools registry 查询 focused smoke" \
            "release-gate focused-smoke 0.23.2" "$ci_file"
        require_ci_text_count_at_least "package/release evidence 依赖 v0.23.2 clang-tidy gate" \
            "- v0232-cpp-clang-tidy-smoke" "$ci_file" 2
    fi

    if [[ -f "$registry_file" ]]; then
        if grep -qF 'version = "0.23.2"' "$registry_file" &&
            grep -qF 'script = "scripts/test-v0232-cpp-clang-tidy-smoke.sh"' "$registry_file"; then
            pass "release gate registry 包含 v0.23.2 focused smoke"
        else
            fail "release gate registry 缺少 v0.23.2 focused smoke"
        fi
    else
        fail "release gate registry 不存在: $registry_file"
    fi

    if [[ -f "$clang_tidy_config" ]]; then
        pass ".clang-tidy 配置存在"
        require_file_text ".clang-tidy 将 warnings 升为错误" \
            'WarningsAsErrors: "*"' "$clang_tidy_config"
        require_file_text ".clang-tidy 启用 use-after-move 检查" \
            "bugprone-use-after-move" "$clang_tidy_config"
        require_file_text ".clang-tidy 启用 generated/runtime header filter" \
            "runtime/cpp/include/flowrt" "$clang_tidy_config"
        require_file_text ".clang-tidy 关闭 clang analyzer 首版噪声" \
            "-clang-analyzer-*" "$clang_tidy_config"
    else
        fail ".clang-tidy 配置不存在: $clang_tidy_config"
    fi

    if [[ -x "$smoke_script" ]]; then
        pass "v0.23.2 C++ clang-tidy smoke 脚本存在且可执行"
        require_file_text "v0.23.2 smoke 支持 dry run" \
            "FLOWRT_V0232_CLANG_TIDY_SMOKE_DRY_RUN" "$smoke_script"
        require_file_text "v0.23.2 smoke 委托长期 C++ static quality gate" \
            "scripts/test-cpp-static-quality.sh" "$smoke_script"
    else
        fail "v0.23.2 C++ clang-tidy smoke 脚本不存在或不可执行: $smoke_script"
    fi

    if [[ -x "$static_quality_script" ]]; then
        pass "C++ static quality 脚本存在且可执行"
        require_file_text "C++ static quality 支持本地 clang-tidy 覆盖" \
            "FLOWRT_CLANG_TIDY" "$static_quality_script"
        require_file_text "C++ static quality 覆盖 runtime profile" \
            "runtime profile" "$static_quality_script"
        require_file_text "C++ static quality 覆盖 generated profile" \
            "generated profile" "$static_quality_script"
        require_file_text "C++ static quality 覆盖 ABI/POD profile" \
            "ABI/POD profile" "$static_quality_script"
        require_file_text "C++ static quality 从 evidence matrix 读取 generated case" \
            "cpp_static_quality" "$static_quality_script"
        require_file_text "C++ static quality 从 compile_commands 读取 runtime TU" \
            "compile_commands.json" "$static_quality_script"
        require_file_text "C++ static quality lint runtime_shell.cpp" \
            "runtime_shell.cpp" "$static_quality_script"
    else
        fail "C++ static quality 脚本不存在或不可执行: $static_quality_script"
    fi
}
