# v0.15.1 CI release evidence 的发布门禁接线检查。
# 本文件由 scripts/check-release-readiness.sh source，并复用其 pass/fail helper。

check_release_evidence_workflow_readiness() {
    local fast_ci_file="$1"
    local ci_file="$2"

    if [[ ! -f "$fast_ci_file" ]]; then
        fail "日常 CI 配置不存在: $fast_ci_file"
    else
        require_file_text "日常 CI 保留 push/PR 快速验证入口" \
            "pull_request:" "$fast_ci_file"
        forbid_file_text "日常 CI 不产出 release evidence" \
            "Release Evidence Gate" "$fast_ci_file"
    fi

    if [[ ! -f "$ci_file" ]]; then
        fail "release candidate 配置不存在: $ci_file"
    else
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
        if grep -q 'release-notes' "$ci_file" && grep -qF 'flowrt-release-evidence' "$ci_file"; then
            pass "release evidence gate 包含 release notes 和 artifact 校验"
        else
            fail "release evidence gate 缺少 release notes 或 artifact 校验"
        fi
    fi
}

check_v0151_ci_release_evidence_readiness() {
    local registry_file="$repo_root/scripts/release-gates/registry.toml"
    local release_candidate_script="$repo_root/scripts/check-release-candidate.sh"
    local smoke_script="$repo_root/scripts/test-v0151-ci-release-evidence-smoke.sh"
    local release_workflow="$repo_root/.github/workflows/release.yml"
    local release_candidate_workflow="$repo_root/.github/workflows/release-candidate.yml"

    if [[ ! -f "$release_candidate_workflow" ]]; then
        fail "release candidate 配置不存在，无法检查 v0.15.1 focused gate"
    else
        require_ci_text "release candidate 包含 v0.15.1 CI release evidence smoke job" \
            "v0151-ci-release-evidence-smoke:" "$release_candidate_workflow"
        require_ci_text "v0.15.1 gate 通过 devtools registry 查询 focused smoke" \
            "release-gate focused-smoke 0.15.1" "$release_candidate_workflow"
        require_ci_text_count_at_least "package/release evidence 依赖 v0.15.1 CI release evidence gate" \
            "- v0151-ci-release-evidence-smoke" "$release_candidate_workflow" 2
        require_ci_text "release candidate 包含 Release Evidence Gate job" \
            "release-evidence:" "$release_candidate_workflow"
        require_ci_text "Release Evidence Gate 在 release candidate push 自动运行" \
            "github.event_name == 'push' && startsWith(github.ref, 'refs/heads/dev/v')" "$release_candidate_workflow"
        require_ci_text "Release Evidence Gate 上传 release evidence artifact" \
            "flowrt-release-evidence" "$release_candidate_workflow"
        forbid_file_text "release candidate 不再暴露 workflow_dispatch RC 入口" \
            "workflow_dispatch:" "$release_candidate_workflow"
        forbid_file_text "release candidate 不再内联 GitHub Release job" \
            "name: GitHub Release" "$release_candidate_workflow"
    fi

    if [[ -f "$release_workflow" ]]; then
        pass "release workflow 存在"
        require_file_text "release workflow 只监听 v* tag" \
            "tags:" "$release_workflow"
        require_file_text "release workflow 查询同 SHA push run" \
            "event=push&head_sha=\${target_sha}&status=success" "$release_workflow"
        require_file_text "release workflow 查询 release candidate workflow" \
            ".github/workflows/release-candidate.yml" "$release_workflow"
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
        require_file_text "release evidence helper 只查询 release candidate workflow" \
            "--workflow release-candidate.yml" "$release_candidate_script"
        require_file_text "release evidence helper 支持等待远端 workflow" \
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
        require_file_text "v0.15.1 smoke 检查 release candidate workflow" \
            ".github/workflows/release-candidate.yml" "$smoke_script"
        require_file_text "v0.15.1 smoke 检查 release evidence" \
            "Release Evidence Gate" "$smoke_script"
        require_file_text "v0.15.1 smoke 禁止 helper --dispatch 参数" \
            "helper 不再接受手工触发参数" "$smoke_script"
    else
        fail "v0.15.1 CI release evidence smoke 脚本不存在或不可执行: $smoke_script"
    fi
}
