# v0.28.0 Module-aware App Layout focused gate 的发布门禁接线检查。
# 本文件由 scripts/check-release-readiness.sh source，并复用其 pass/fail helper。

check_v0280_module_app_layout_readiness() {
    local ci_file="$repo_root/.github/workflows/release-candidate.yml"
    local registry_file="$repo_root/scripts/release-gates/registry.toml"
    local smoke_script="$repo_root/scripts/test-v0280-module-app-layout-smoke.sh"
    local context_file="$repo_root/CONTEXT.md"
    local changelog_file="$repo_root/CHANGELOG.md"
    local development_doc="$repo_root/docs/development.md"

    if [[ ! -f "$ci_file" ]]; then
        fail "CI 配置不存在，无法检查 v0.28.0 focused gate"
    else
        require_ci_text "CI 包含 v0.28.0 module app layout smoke job" \
            "v0280-module-app-layout-smoke:" "$ci_file"
        require_ci_text "v0.28.0 gate 通过 devtools registry 查询 focused smoke" \
            "release-gate focused-smoke 0.28.0" "$ci_file"
        require_ci_text_count_at_least "package/release evidence 依赖 v0.28.0 focused gate" \
            "- v0280-module-app-layout-smoke" "$ci_file" 2
    fi

    if [[ -f "$registry_file" ]]; then
        if grep -qF 'version = "0.28.0"' "$registry_file" &&
            grep -qF 'script = "scripts/test-v0280-module-app-layout-smoke.sh"' "$registry_file"; then
            pass "release gate registry 包含 v0.28.0 focused smoke"
        else
            fail "release gate registry 缺少 v0.28.0 focused smoke"
        fi
    else
        fail "release gate registry 不存在: $registry_file"
    fi

    if [[ -x "$smoke_script" ]]; then
        pass "v0.28.0 focused smoke 脚本存在且可执行"
        require_file_text "v0.28.0 smoke 支持 dry run" \
            "FLOWRT_V0280_MODULE_APP_LAYOUT_SMOKE_DRY_RUN" "$smoke_script"
        require_file_text "v0.28.0 smoke 覆盖 module-aware App API" \
            "app/mygo_lidar/rust/calibrator.rs" "$smoke_script"
        require_file_text "v0.28.0 smoke 覆盖 module reference stub" \
            "app/stubs/mygo_lidar/cpp/driver.cpp" "$smoke_script"
        require_file_text "v0.28.0 smoke 覆盖 prepare 不写用户 app" \
            "test ! -e \"\$project/app/mygo_lidar\"" "$smoke_script"
        require_file_text "v0.28.0 smoke 覆盖 generated CMake module C/C++ discovery" \
            "emits_cpp_managed_app_targets" "$smoke_script"
    else
        fail "v0.28.0 focused smoke 脚本不存在或不可执行: $smoke_script"
    fi

    require_file_text "CONTEXT 记录 v0.28.0 当前版本背景" \
        "v0.28.0 Module-aware App Layout" "$context_file"
    require_file_text "CHANGELOG 记录 v0.28.0 release 段" \
        "## v0.28.0 - 2026-06-24" "$changelog_file"
    require_file_text "开发文档记录 module app layout smoke" \
        "module app layout smoke" "$development_doc"
}
