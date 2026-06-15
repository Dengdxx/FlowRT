# v0.15.2 scheduler clock 的发布门禁接线检查。
# 本文件由 scripts/check-release-readiness.sh source，并复用其 pass/fail helper。

check_v0152_scheduler_clock_readiness() {
    local ci_file="$repo_root/.github/workflows/ci.yml"
    local registry_file="$repo_root/scripts/release-gates/registry.toml"
    local smoke_script="$repo_root/scripts/test-v0152-scheduler-clock-smoke.sh"

    if [[ ! -f "$ci_file" ]]; then
        fail "CI 配置不存在，无法检查 v0.15.2 focused gate"
    else
        require_ci_text "CI 包含 v0.15.2 scheduler clock smoke job" \
            "v0152-scheduler-clock-smoke:" "$ci_file"
        require_ci_text "v0.15.2 gate 通过 devtools registry 查询 focused smoke" \
            "release-gate focused-smoke 0.15.2" "$ci_file"
        require_ci_text_count_at_least "package/release evidence 依赖 v0.15.2 scheduler clock gate" \
            "- v0152-scheduler-clock-smoke" "$ci_file" 2
    fi

    if [[ -f "$registry_file" ]]; then
        if grep -qF 'version = "0.15.2"' "$registry_file"; then
            pass "release gate registry 包含 v0.15.2 focused smoke"
        else
            fail "release gate registry 缺少 v0.15.2 focused smoke"
        fi
    else
        fail "release gate registry 不存在: $registry_file"
    fi

    if [[ -x "$smoke_script" ]]; then
        pass "v0.15.2 scheduler clock smoke 脚本存在且可执行"
        require_file_text "v0.15.2 smoke 支持 dry run" \
            "FLOWRT_V0152_SCHEDULER_CLOCK_SMOKE_DRY_RUN" "$smoke_script"
        require_file_text "v0.15.2 smoke 覆盖 realtime Rust scheduler clock" \
            "rust_shell_builds_scheduler_v2_task_plan_and_wakes_on_input_revision" "$smoke_script"
        require_file_text "v0.15.2 smoke 覆盖 realtime C++ scheduler clock" \
            "cpp_shell_builds_scheduler_v2_task_plan_and_wakes_on_input_revision" "$smoke_script"
        require_file_text "v0.15.2 smoke 覆盖 temporary island replay clock" \
            "launch_manifest_and_selfdesc_expose_temporary_island_artifact_metadata" "$smoke_script"
    else
        fail "v0.15.2 scheduler clock smoke 脚本不存在或不可执行: $smoke_script"
    fi
}
