# v0.23.3 focused gate 的发布门禁接线检查。
# 本文件由 scripts/check-release-readiness.sh source，并复用其 pass/fail helper。

check_v0233_scope_closure_readiness() {
    local ci_file="$repo_root/.github/workflows/release-candidate.yml"
    local registry_file="$repo_root/scripts/release-gates/registry.toml"
    local smoke_script="$repo_root/scripts/test-v0233-scope-closure-smoke.sh"

    if [[ ! -f "$ci_file" ]]; then
        fail "CI 配置不存在，无法检查 v0.23.3 focused gate"
    else
        require_ci_text "CI 包含 v0.23.3 focused smoke job" \
            "v0233-scope-closure-smoke:" "$ci_file"
        require_ci_text "v0.23.3 gate 通过 devtools registry 查询 focused smoke" \
            "release-gate focused-smoke 0.23.3" "$ci_file"
        require_ci_text_count_at_least "package/release evidence 依赖 v0.23.3 focused gate" \
            "- v0233-scope-closure-smoke" "$ci_file" 2
    fi

    if [[ -f "$registry_file" ]]; then
        if grep -qF 'version = "0.23.3"' "$registry_file" &&
            grep -qF 'script = "scripts/test-v0233-scope-closure-smoke.sh"' "$registry_file"; then
            pass "release gate registry 包含 v0.23.3 focused smoke"
        else
            fail "release gate registry 缺少 v0.23.3 focused smoke"
        fi
    else
        fail "release gate registry 不存在: $registry_file"
    fi

    if [[ -x "$smoke_script" ]]; then
        pass "v0.23.3 focused smoke 脚本存在且可执行"
        require_file_text "v0.23.3 smoke 支持 dry run" \
            "FLOWRT_V0233_SMOKE_DRY_RUN" "$smoke_script"
        require_file_text "v0.23.3 smoke 串联 global tick smoke" \
            "scripts/test-v0233-global-tick-determinism-smoke.sh" "$smoke_script"
        require_file_text "v0.23.3 smoke 覆盖 standby failover codegen" \
            "cargo test -p flowrt-codegen standby_failover" "$smoke_script"
        require_file_text "v0.23.3 smoke 覆盖 Operation zenoh codegen" \
            "cargo test -p flowrt-codegen zenoh_operation" "$smoke_script"
        require_file_text "v0.23.3 smoke 覆盖 tracing exporter" \
            "cargo test -p flowrt tracing_exporter" "$smoke_script"
        require_file_text "v0.23.3 smoke 覆盖 C v0 params" \
            "cargo test -p flowrt-codegen c_params" "$smoke_script"
    else
        fail "v0.23.3 focused smoke 脚本不存在或不可执行: $smoke_script"
    fi

    require_file_text "CHANGELOG 记录 v0.23.3" \
        "## v0.23.3 - 2026-06-19" "$repo_root/CHANGELOG.md"
}
