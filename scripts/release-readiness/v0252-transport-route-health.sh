# v0.25.2 Transport Route Health focused gate 的发布门禁接线检查。
# 本文件由 scripts/check-release-readiness.sh source，并复用其 pass/fail helper。

check_v0252_transport_route_health_readiness() {
    local ci_file="$repo_root/.github/workflows/release-candidate.yml"
    local registry_file="$repo_root/scripts/release-gates/registry.toml"
    local smoke_script="$repo_root/scripts/test-v0252-transport-route-health-smoke.sh"
    local context_file="$repo_root/CONTEXT.md"
    local changelog_file="$repo_root/CHANGELOG.md"
    local cli_doc="$repo_root/docs/cli.md"

    if [[ ! -f "$ci_file" ]]; then
        fail "CI 配置不存在，无法检查 v0.25.2 focused gate"
    else
        require_ci_text "CI 包含 v0.25.2 transport route health smoke job" \
            "v0252-transport-route-health-smoke:" "$ci_file"
        require_ci_text "v0.25.2 gate 通过 devtools registry 查询 focused smoke" \
            "release-gate focused-smoke 0.25.2" "$ci_file"
        require_ci_text_count_at_least "package/release evidence 依赖 v0.25.2 focused gate" \
            "- v0252-transport-route-health-smoke" "$ci_file" 2
    fi

    if [[ -f "$registry_file" ]]; then
        if grep -qF 'version = "0.25.2"' "$registry_file" &&
            grep -qF 'script = "scripts/test-v0252-transport-route-health-smoke.sh"' "$registry_file"; then
            pass "release gate registry 包含 v0.25.2 focused smoke"
        else
            fail "release gate registry 缺少 v0.25.2 focused smoke"
        fi
    else
        fail "release gate registry 不存在: $registry_file"
    fi

    if [[ -x "$smoke_script" ]]; then
        pass "v0.25.2 focused smoke 脚本存在且可执行"
        require_file_text "v0.25.2 smoke 支持 dry run" \
            "FLOWRT_V0252_TRANSPORT_ROUTE_HEALTH_SMOKE_DRY_RUN" "$smoke_script"
        require_file_text "v0.25.2 smoke 覆盖 Rust introspection counters" \
            "route_transport_error_updates_policy_counter_and_backend_error" "$smoke_script"
        require_file_text "v0.25.2 smoke 覆盖 transport codegen route facts" \
            "backend_route_health_is_recorded" "$smoke_script"
        require_file_text "v0.25.2 smoke 覆盖 transport dataflow golden" \
            "golden_bounded_channel_iox2 golden_cross_process_feedback" "$smoke_script"
        require_file_text "v0.25.2 smoke 覆盖 C++ introspection counters" \
            "flowrt_runtime_introspection_smoke" "$smoke_script"
    else
        fail "v0.25.2 focused smoke 脚本不存在或不可执行: $smoke_script"
    fi

    require_file_text "CONTEXT 记录 v0.25.2 当前版本背景" \
        "v0.25.2 Transport Route Health" "$context_file"
    require_file_text "CHANGELOG 记录 v0.25.2 release 段" \
        "## v0.25.2 - 2026-06-21" "$changelog_file"
    require_file_text "CONTEXT 记录 transport publish failure counter 投影" \
        "route overflow policy 投影到统一 route counters" "$context_file"
    require_file_text "CONTEXT 记录 transport publish backend health 诊断" \
        "backend health / last error 诊断" "$context_file"
    require_file_text "CLI 文档记录 route counter 投影口径" \
        'transport publish 失败会同时保留 `last_error`' "$cli_doc"
}
