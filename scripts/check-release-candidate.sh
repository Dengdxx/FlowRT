#!/usr/bin/env bash
# 发布证据门禁脚本。
#
# 用法：
#   scripts/check-release-candidate.sh VERSION [--wait] [--ref REF]
#
# 默认只运行本地发布就绪检查和 focused smoke。加 --wait 后等待同一提交的
# push CI，并要求 Release Evidence Gate 成功。该脚本不触发远端 CI；远端
# evidence 只能由 push 自动产生。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

usage() {
    sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'
}

fail() {
    printf '错误: %s\n' "$1" >&2
    exit 1
}

info() {
    printf '→ %s\n' "$1"
}

run_focused_smoke() {
    local version="$1"
    local focused_smoke_script

    focused_smoke_script="$(
        cargo run -p flowrt-devtools -- release-gate focused-smoke "$version"
    )"
    if [[ -z "$focused_smoke_script" ]]; then
        fail "release gate registry 未返回 v${version} focused smoke 脚本"
    fi
    if [[ ! -x "$focused_smoke_script" ]]; then
        fail "v${version} focused smoke 脚本不存在或不可执行: $focused_smoke_script"
    fi

    info "运行 v${version} focused smoke: $focused_smoke_script"
    "$focused_smoke_script"
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
fi

version="${1:-}"
if [[ -z "$version" ]]; then
    usage >&2
    exit 2
fi
shift

version="${version#v}"
if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    fail "VERSION 必须是 X.Y.Z 或 vX.Y.Z，实际为: $version"
fi

wait_for_run=false
ref=""

while [[ "$#" -gt 0 ]]; do
    case "$1" in
        --wait)
            wait_for_run=true
            ;;
        --ref)
            shift
            ref="${1:-}"
            if [[ -z "$ref" ]]; then
                fail "--ref 需要分支名"
            fi
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            fail "未知参数: $1"
            ;;
    esac
    shift
done

cd "$repo_root"

tag="v${version}"
head_sha="$(git rev-parse HEAD)"
current_branch="$(git branch --show-current || true)"
if [[ -z "$ref" ]]; then
    ref="$current_branch"
fi

tracked_status="$(git status --short --untracked-files=no)"
if [[ -n "$tracked_status" ]]; then
    printf '%s\n' "$tracked_status" >&2
    fail "存在未提交的 tracked 改动；发布证据检查必须基于一个确定提交"
fi

info "本地发布就绪检查: $tag"
scripts/check-release-readiness.sh "$version"
scripts/extract-release-notes.sh "$tag" CHANGELOG.md >/tmp/flowrt-release-notes-check.md
if [[ ! -s /tmp/flowrt-release-notes-check.md ]]; then
    fail "$tag 的 release notes 为空"
fi

run_focused_smoke "$version"

if [[ "$wait_for_run" != true ]]; then
    info "本地预检完成；未使用 --wait，未检查远端 release evidence"
    exit 0
fi

command -v gh >/dev/null 2>&1 || fail "需要安装并登录 gh CLI"

repo="$(gh repo view --json nameWithOwner --jq .nameWithOwner)"

if [[ -z "$ref" ]]; then
    fail "当前不是具名分支；请用 --ref 指定要等待的远端分支"
fi

remote_sha="$(git ls-remote origin "refs/heads/${ref}" | awk '{print $1}')"
if [[ -z "$remote_sha" ]]; then
    fail "origin 上不存在分支 refs/heads/${ref}"
fi
if [[ "$remote_sha" != "$head_sha" ]]; then
    printf '本地 HEAD:  %s\n' "$head_sha" >&2
    printf 'origin/%s: %s\n' "$ref" "$remote_sha" >&2
    fail "远端分支尚未指向当前提交；请先 push 再等待 release evidence"
fi

find_run_id() {
    gh run list \
        --workflow ci.yml \
        --event push \
        --branch "$ref" \
        --limit 20 \
        --json databaseId,headSha \
        --jq ".[] | select(.headSha == \"${head_sha}\") | .databaseId" |
        sed -n '1p'
}

run_id=""
for _ in $(seq 1 30); do
    run_id="$(find_run_id)"
    if [[ -n "$run_id" ]]; then
        break
    fi
    sleep 2
done

if [[ -z "$run_id" ]]; then
    fail "未找到 sha=$head_sha 的 push CI run"
fi

run_url="$(gh run view "$run_id" --json url --jq .url)"
info "release evidence source run: $run_url"

gh run watch "$run_id" --exit-status

evidence_job_url="$(
    gh api \
        "/repos/${repo}/actions/runs/${run_id}/jobs?per_page=100" \
        --paginate \
        --jq '.jobs[] | select(.name == "Release Evidence Gate" and .conclusion == "success") | .html_url' |
        sed -n '1p'
)"
if [[ -z "$evidence_job_url" ]]; then
    fail "push CI 已结束，但未找到成功的 Release Evidence Gate job"
fi
info "release evidence gate 通过: $evidence_job_url"
