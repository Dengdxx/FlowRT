# v0.25.0 iox2 Service/Operation focused gate 的发布门禁接线检查。
# 本文件由 scripts/check-release-readiness.sh source，并复用其 pass/fail helper。

check_v0250_iox2_service_operation_readiness() {
    local ci_file="$repo_root/.github/workflows/release-candidate.yml"
    local registry_file="$repo_root/scripts/release-gates/registry.toml"
    local smoke_script="$repo_root/scripts/test-v0250-iox2-service-operation-smoke.sh"

    if [[ ! -f "$ci_file" ]]; then
        fail "CI 配置不存在，无法检查 v0.25.0 focused gate"
    else
        require_ci_text "CI 包含 v0.25.0 iox2 service/operation smoke job" \
            "v0250-iox2-service-operation-smoke:" "$ci_file"
        require_ci_text "v0.25.0 gate 通过 devtools registry 查询 focused smoke" \
            "release-gate focused-smoke 0.25.0" "$ci_file"
        require_ci_text "v0.25.0 CI 启用真实 iox2 SDK 子步" \
            "FLOWRT_V0250_REQUIRE_IOX2_SDK: \"1\"" "$ci_file"
        require_ci_text "v0.25.0 CI 覆盖 amd64 runner" \
            "runner: ubuntu-latest" "$ci_file"
        require_ci_text "v0.25.0 CI 覆盖 arm64 runner" \
            "runner: ubuntu-24.04-arm" "$ci_file"
        require_ci_text_count_at_least "package/release evidence 依赖 v0.25.0 focused gate" \
            "- v0250-iox2-service-operation-smoke" "$ci_file" 2
    fi

    if [[ -f "$registry_file" ]]; then
        if grep -qF 'version = "0.25.0"' "$registry_file" &&
            grep -qF 'script = "scripts/test-v0250-iox2-service-operation-smoke.sh"' "$registry_file"; then
            pass "release gate registry 包含 v0.25.0 focused smoke"
        else
            fail "release gate registry 缺少 v0.25.0 focused smoke"
        fi
    else
        fail "release gate registry 不存在: $registry_file"
    fi

    if [[ -x "$smoke_script" ]]; then
        pass "v0.25.0 focused smoke 脚本存在且可执行"
        require_file_text "v0.25.0 smoke 支持 dry run" \
            "FLOWRT_V0250_IOX2_SERVICE_OPERATION_SMOKE_DRY_RUN" "$smoke_script"
        require_file_text "v0.25.0 smoke 覆盖 resolver matrix" \
            "control_plane_resolver_matrix" "$smoke_script"
        require_file_text "v0.25.0 smoke 覆盖 runtime iox2 service" \
            "cargo test -p flowrt --features iox2 -j1 -- iox2_service" "$smoke_script"
        require_file_text "v0.25.0 smoke 覆盖 iox2 service demo" \
            "examples/iox2_service_demo/rsdl/robot.rsdl" "$smoke_script"
        require_file_text "v0.25.0 smoke 可强制真实 iox2 SDK build/run" \
            "FLOWRT_V0250_REQUIRE_IOX2_SDK" "$smoke_script"
    else
        fail "v0.25.0 focused smoke 脚本不存在或不可执行: $smoke_script"
    fi
}
