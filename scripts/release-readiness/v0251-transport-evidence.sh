# v0.25.1 Transport Evidence focused gate 的发布门禁接线检查。
# 本文件由 scripts/check-release-readiness.sh source，并复用其 pass/fail helper。

check_v0251_transport_evidence_readiness() {
    local ci_file="$repo_root/.github/workflows/release-candidate.yml"
    local registry_file="$repo_root/scripts/release-gates/registry.toml"
    local smoke_script="$repo_root/scripts/test-v0251-transport-evidence-smoke.sh"
    local project_layout="$repo_root/docs/project-layout.md"

    if [[ ! -f "$ci_file" ]]; then
        fail "CI 配置不存在，无法检查 v0.25.1 focused gate"
    else
        require_ci_text "CI 包含 v0.25.1 transport evidence smoke job" \
            "v0251-transport-evidence-smoke:" "$ci_file"
        require_ci_text "v0.25.1 gate 通过 devtools registry 查询 focused smoke" \
            "release-gate focused-smoke 0.25.1" "$ci_file"
        require_ci_text "v0.25.1 CI 启用真实 transport SDK build 子步" \
            "FLOWRT_V0251_REQUIRE_TRANSPORT_SDK: \"1\"" "$ci_file"
        require_ci_text "v0.25.1 CI 覆盖 amd64 runner" \
            "runner: ubuntu-latest" "$ci_file"
        require_ci_text "v0.25.1 CI 覆盖 arm64 runner" \
            "runner: ubuntu-24.04-arm" "$ci_file"
        require_ci_text_count_at_least "package/release evidence 依赖 v0.25.1 focused gate" \
            "- v0251-transport-evidence-smoke" "$ci_file" 2
    fi

    if [[ -f "$registry_file" ]]; then
        if grep -qF 'version = "0.25.1"' "$registry_file" &&
            grep -qF 'script = "scripts/test-v0251-transport-evidence-smoke.sh"' "$registry_file"; then
            pass "release gate registry 包含 v0.25.1 focused smoke"
        else
            fail "release gate registry 缺少 v0.25.1 focused smoke"
        fi
    else
        fail "release gate registry 不存在: $registry_file"
    fi

    if [[ -x "$smoke_script" ]]; then
        pass "v0.25.1 focused smoke 脚本存在且可执行"
        require_file_text "v0.25.1 smoke 支持 dry run" \
            "FLOWRT_V0251_TRANSPORT_EVIDENCE_SMOKE_DRY_RUN" "$smoke_script"
        require_file_text "v0.25.1 smoke 可强制真实 transport SDK build" \
            "FLOWRT_V0251_REQUIRE_TRANSPORT_SDK" "$smoke_script"
        require_file_text "v0.25.1 smoke 覆盖 zenoh service demo build" \
            "examples/zenoh_service_demo" "$smoke_script"
        require_file_text "v0.25.1 smoke 覆盖 iox2 service/operation demo build" \
            "examples/iox2_service_demo" "$smoke_script"
        require_file_text "v0.25.1 smoke 在 arm64 runner 使用本机 target" \
            "linux-arm64" "$smoke_script"
    else
        fail "v0.25.1 focused smoke 脚本不存在或不可执行: $smoke_script"
    fi

    if [[ -f "$project_layout" ]]; then
        require_file_text "C callback v0 文档记录 readonly params snapshot" \
            "readonly params snapshot" "$project_layout"
    else
        fail "project layout 文档不存在: $project_layout"
    fi
}
