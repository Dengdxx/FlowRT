#!/usr/bin/env bash
# v0.15.1 CI release evidence smoke。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ "${FLOWRT_V0151_CI_RELEASE_EVIDENCE_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.15.1 CI release evidence smoke dry run"
    exit 0
fi

need_text() {
    local label="$1"
    local needle="$2"
    local file="$3"

    if ! grep -qF -- "$needle" "$file"; then
        printf '错误: %s 缺少 %s\n' "$label" "$needle" >&2
        exit 1
    fi
    printf '  ✓ %s\n' "$label"
}

forbid_text() {
    local label="$1"
    local needle="$2"
    local file="$3"

    if grep -qF -- "$needle" "$file"; then
        printf '错误: %s 不应包含 %s\n' "$label" "$needle" >&2
        exit 1
    fi
    printf '  ✓ %s\n' "$label"
}

ci_file=".github/workflows/ci.yml"
release_file=".github/workflows/release.yml"

echo "v0.15.1 CI release evidence smoke: script syntax"
bash -n \
    scripts/check-release-candidate.sh \
    scripts/check-release-readiness.sh \
    scripts/release-readiness/v0151-ci-release-evidence.sh \
    scripts/test-v0151-ci-release-evidence-smoke.sh

echo "v0.15.1 CI release evidence smoke: CI contract"
need_text "CI 包含 v0.15.1 focused job" "v0151-ci-release-evidence-smoke:" "$ci_file"
need_text "CI 通过 registry 运行 v0.15.1 focused smoke" "release-gate focused-smoke 0.15.1" "$ci_file"
need_text "CI 限制 push branches" "branches:" "$ci_file"
need_text "CI 自动产出 Release Evidence Gate" "Release Evidence Gate" "$ci_file"
need_text "CI 上传 release evidence artifact" "flowrt-release-evidence" "$ci_file"
forbid_text "CI 不再暴露 workflow_dispatch RC" "workflow_dispatch:" "$ci_file"
forbid_text "CI 不再内联 GitHub Release job" "name: GitHub Release" "$ci_file"

echo "v0.15.1 CI release evidence smoke: release workflow contract"
need_text "release workflow 存在" "name: Release" "$release_file"
need_text "release workflow 监听 tag" "tags:" "$release_file"
need_text "release workflow 查询 push CI" "event=push&head_sha=\${target_sha}&status=success" "$release_file"
need_text "release workflow 要求 Release Evidence Gate" "Release Evidence Gate\" and .conclusion == \"success\"" "$release_file"
need_text "release workflow 下载 release evidence artifact" "flowrt-release-evidence" "$release_file"

echo "v0.15.1 CI release evidence smoke: release evidence helper"
need_text "helper 查询 push CI" "--event push" "scripts/check-release-candidate.sh"
need_text "helper 等待远端 CI" "gh run watch" "scripts/check-release-candidate.sh"
forbid_text "helper 不再手工触发 CI" "gh workflow run ci.yml" "scripts/check-release-candidate.sh"
forbid_text "helper 不再接受手工触发参数" "--dispatch" "scripts/check-release-candidate.sh"

echo "v0.15.1 CI release evidence smoke passed"
