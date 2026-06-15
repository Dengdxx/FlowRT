# v0.15.1 CI release evidence 的发布门禁接线检查。
# 本文件由 scripts/check-release-readiness.sh source，并复用其 pass/fail helper。

check_v0151_ci_release_evidence_readiness() {
    local registry_file="$repo_root/scripts/release-gates/registry.toml"
    local release_candidate_script="$repo_root/scripts/check-release-candidate.sh"
    local smoke_script="$repo_root/scripts/test-v0151-ci-release-evidence-smoke.sh"
    local release_workflow="$repo_root/.github/workflows/release.yml"

    if [[ ! -f "$ci_file" ]]; then
        fail "CI 配置不存在，无法检查 v0.15.1 focused gate"
    else
        require_ci_text "CI 包含 v0.15.1 CI release evidence smoke job" \
            "v0151-ci-release-evidence-smoke:" "$ci_file"
        require_ci_text "v0.15.1 gate 通过 devtools registry 查询 focused smoke" \
            "release-gate focused-smoke 0.15.1" "$ci_file"
        require_ci_text_count_at_least "package/release evidence 依赖 v0.15.1 CI release evidence gate" \
            "- v0151-ci-release-evidence-smoke" "$ci_file" 2
        require_ci_text "CI 包含 Release Evidence Gate job" \
            "release-evidence:" "$ci_file"
        require_ci_text "Release Evidence Gate 在 push CI 自动运行" \
            "github.event_name == 'push' && startsWith(github.ref, 'refs/heads/dev/v')" "$ci_file"
        require_ci_text "Release Evidence Gate 上传 release evidence artifact" \
            "flowrt-release-evidence" "$ci_file"
        forbid_file_text "CI 不再暴露 workflow_dispatch RC 入口" \
            "workflow_dispatch:" "$ci_file"
        forbid_file_text "CI 不再内联 GitHub Release job" \
            "name: GitHub Release" "$ci_file"
    fi

    if [[ -f "$release_workflow" ]]; then
        pass "release workflow 存在"
        require_file_text "release workflow 只监听 v* tag" \
            "tags:" "$release_workflow"
        require_file_text "release workflow 查询同 SHA push CI" \
            "event=push&head_sha=\${target_sha}&status=success" "$release_workflow"
        require_file_text "release workflow 校验 Release Evidence Gate" \
            "Release Evidence Gate\" and .conclusion == \"success\"" "$release_workflow"
        require_file_text "release workflow 下载 release evidence artifact" \
            "flowrt-release-evidence" "$release_workflow"
    else
        fail "release workflow 不存在: $release_workflow"
    fi

    if [[ -x "$release_candidate_script" ]]; then
        pass "release evidence helper 存在且可执行"
        forbid_file_text "release evidence helper 不再手工触发远端 CI" \
            "gh workflow run ci.yml" "$release_candidate_script"
        forbid_file_text "release evidence helper 不再接受 --dispatch 参数" \
            "--dispatch" "$release_candidate_script"
        require_file_text "release evidence helper 只查询 push CI" \
            "--event push" "$release_candidate_script"
        require_file_text "release evidence helper 支持等待远端 CI" \
            "gh run watch" "$release_candidate_script"
        require_file_text "release evidence helper 校验远端分支 SHA" \
            "远端分支尚未指向当前提交" "$release_candidate_script"
        require_file_text "release evidence helper 要求 Release Evidence Gate 成功" \
            "Release Evidence Gate" "$release_candidate_script"
    else
        fail "release evidence helper 不存在或不可执行: $release_candidate_script"
    fi

    if [[ -f "$registry_file" ]]; then
        if grep -qF 'version = "0.15.1"' "$registry_file"; then
            pass "release gate registry 包含 v0.15.1 focused smoke"
        else
            fail "release gate registry 缺少 v0.15.1 focused smoke"
        fi
    else
        fail "release gate registry 不存在: $registry_file"
    fi

    if [[ -x "$smoke_script" ]]; then
        pass "v0.15.1 CI release evidence smoke 脚本存在且可执行"
        require_file_text "v0.15.1 smoke 支持 dry run" \
            "FLOWRT_V0151_CI_RELEASE_EVIDENCE_SMOKE_DRY_RUN" "$smoke_script"
        require_file_text "v0.15.1 smoke 检查 release workflow" \
            ".github/workflows/release.yml" "$smoke_script"
        require_file_text "v0.15.1 smoke 检查 release evidence" \
            "Release Evidence Gate" "$smoke_script"
        require_file_text "v0.15.1 smoke 禁止 helper --dispatch 参数" \
            "helper 不再接受手工触发参数" "$smoke_script"
    else
        fail "v0.15.1 CI release evidence smoke 脚本不存在或不可执行: $smoke_script"
    fi
}
