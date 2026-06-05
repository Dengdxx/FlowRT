#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/extract-release-notes.sh <tag> [CHANGELOG.md]

Print the changelog section for a release tag such as v0.1.0.
EOF
}

if [[ "$#" -lt 1 || "$#" -gt 2 ]]; then
    usage >&2
    exit 2
fi

tag="$1"
changelog="${2:-CHANGELOG.md}"
version="${tag#v}"

if [[ ! -f "$changelog" ]]; then
    printf 'changelog not found: %s\n' "$changelog" >&2
    exit 1
fi

awk -v tag="$tag" -v version="$version" '
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
    lines[++line_count] = $0;
}

END {
    if (!found) {
        printf("release section for `%s` not found in changelog\n", tag) > "/dev/stderr";
        exit 3;
    }
    while (line_count > 0 && lines[line_count] ~ /^[[:space:]]*$/) {
        line_count--;
    }
    if (line_count == 0) {
        printf("release section for `%s` is empty\n", tag) > "/dev/stderr";
        exit 4;
    }
    for (i = 1; i <= line_count; i++) {
        print lines[i];
    }
}
' "$changelog"
