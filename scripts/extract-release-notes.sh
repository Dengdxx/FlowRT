#!/usr/bin/env bash
# 从 CHANGELOG.md 抽取指定 tag 对应的 release notes 段。
#
# 用法：scripts/extract-release-notes.sh <tag> [CHANGELOG.md]
#
# 退出码：
#   0 — 成功，release notes 已输出到 stdout
#   1 — CHANGELOG 文件不存在
#   2 — 参数错误
#   3 — 对应版本段未找到
#   4 — 版本段存在但内容为空
#
# 版本段格式要求：## vX.Y.Z - YYYY-MM-DD
# 匹配逻辑同时接受 tag（vX.Y.Z）和 version（X.Y.Z）作为 heading key。

set -euo pipefail

usage() {
    cat <<'EOF'
用法: scripts/extract-release-notes.sh <tag> [CHANGELOG.md]

从 CHANGELOG.md 抽取指定 tag（如 v0.1.0）对应的 release notes 段。
输出到 stdout；错误信息输出到 stderr。

示例:
  scripts/extract-release-notes.sh v0.3.2 CHANGELOG.md
  scripts/extract-release-notes.sh v0.3.2
EOF
}

if [[ "$#" -lt 1 || "$#" -gt 2 ]]; then
    usage >&2
    exit 2
fi

tag="$1"
changelog="${2:-CHANGELOG.md}"
version="${tag#v}"

if [[ -z "$tag" ]]; then
    printf '错误: tag 参数不能为空\n' >&2
    exit 2
fi

if [[ ! -f "$changelog" ]]; then
    printf '错误: CHANGELOG 文件不存在: %s\n' "$changelog" >&2
    exit 1
fi

result="$(awk -v tag="$tag" -v version="$version" '
function heading_key(line, key) {
    if (line !~ /^##[[:space:]]+/) {
        return "";
    }
    key = line;
    sub(/^##[[:space:]]+/, "", key);
    sub(/^\[/, "", key);
    sub(/\].*$/, "", key);
    sub(/[[:space:]].*$/, "", key);
    return key;
}

heading_key($0) == tag || heading_key($0) == version {
    found = 1;
    capture = 1;
    next;
}

capture && $0 ~ /^##[[:space:]]+/ {
    capture = 0;
    exit;
}

capture {
    if (!started && $0 ~ /^[[:space:]]*$/) {
        next;
    }
    started = 1;
    line_count++;
    lines[line_count] = $0;
}

END {
    if (!found) {
        printf("错误: CHANGELOG 中未找到 `%s` 对应的版本段\n", tag) > "/dev/stderr";
        exit 3;
    }
    while (line_count > 0 && lines[line_count] ~ /^[[:space:]]*$/) {
        line_count--;
    }
    if (line_count == 0) {
        printf("错误: `%s` 版本段存在但内容为空\n", tag) > "/dev/stderr";
        exit 4;
    }
    for (i = 1; i <= line_count; i++) {
        print lines[i];
    }
}
' "$changelog")" || exit $?

printf '%s\n' "$result"
