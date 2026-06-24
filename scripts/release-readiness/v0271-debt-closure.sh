# v0.27.1 Debt Closure Hardening focused gate 的发布门禁接线检查。
# 本文件由 scripts/check-release-readiness.sh source，并复用其 pass/fail helper。

check_v0271_debt_closure_readiness() {
    local ci_file="$repo_root/.github/workflows/release-candidate.yml"
    local registry_file="$repo_root/scripts/release-gates/registry.toml"
    local smoke_script="$repo_root/scripts/test-v0271-debt-closure-smoke.sh"
    local context_file="$repo_root/CONTEXT.md"
    local changelog_file="$repo_root/CHANGELOG.md"
    local development_doc="$repo_root/docs/development.md"

    if [[ ! -f "$ci_file" ]]; then
        fail "CI 配置不存在，无法检查 v0.27.1 focused gate"
    else
        require_ci_text "CI 包含 v0.27.1 debt closure smoke job" \
            "v0271-debt-closure-smoke:" "$ci_file"
        require_ci_text "v0.27.1 gate 通过 devtools registry 查询 focused smoke" \
            "release-gate focused-smoke 0.27.1" "$ci_file"
        require_ci_text "v0.27.1 gate 安装 C++ static quality 依赖" \
            "clang-tidy" "$ci_file"
        require_ci_text_count_at_least "package/release evidence 依赖 v0.27.1 focused gate" \
            "- v0271-debt-closure-smoke" "$ci_file" 2
    fi

    if [[ -f "$registry_file" ]]; then
        if grep -qF 'version = "0.27.1"' "$registry_file" &&
            grep -qF 'script = "scripts/test-v0271-debt-closure-smoke.sh"' "$registry_file"; then
            pass "release gate registry 包含 v0.27.1 focused smoke"
        else
            fail "release gate registry 缺少 v0.27.1 focused smoke"
        fi
    else
        fail "release gate registry 不存在: $registry_file"
    fi

    if [[ -x "$smoke_script" ]]; then
        pass "v0.27.1 focused smoke 脚本存在且可执行"
        require_file_text "v0.27.1 smoke 支持 dry run" \
            "FLOWRT_V0271_DEBT_CLOSURE_SMOKE_DRY_RUN" "$smoke_script"
        require_file_text "v0.27.1 smoke 覆盖 evidence matrix" \
            "scripts/check-evidence-matrix.sh" "$smoke_script"
        require_file_text "v0.27.1 smoke 覆盖 generated compile net" \
            "scripts/test-codegen-compile.sh" "$smoke_script"
        require_file_text "v0.27.1 smoke 覆盖 C++ static quality" \
            "scripts/test-cpp-static-quality.sh" "$smoke_script"
        require_file_text "v0.27.1 smoke 覆盖 feedback typed literal" \
            "feedback_init" "$smoke_script"
        require_file_text "v0.27.1 smoke 覆盖 route typed error" \
            "live_status_summary_displays_channel_input_and_route_diagnostics" "$smoke_script"
        require_file_text "v0.27.1 smoke 覆盖 Operation observation verification" \
            "operation_observation" "$smoke_script"
        require_file_text "v0.27.1 smoke 覆盖 C ABI string params" \
            "generated_c_params_callback_receives_readonly_snapshot" "$smoke_script"
    else
        fail "v0.27.1 focused smoke 脚本不存在或不可执行: $smoke_script"
    fi

    require_file_text "CONTEXT 记录 v0.27.1 当前版本背景" \
        "v0.27.1 Debt Closure Hardening" "$context_file"
    require_file_text "CHANGELOG 记录 v0.27.1 release 段" \
        "## v0.27.1 - 2026-06-24" "$changelog_file"
    require_file_text "开发文档记录 debt closure smoke" \
        "debt closure smoke" "$development_doc"
}
