#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

cd "$repo_root"

if [[ "${FLOWRT_V0141_ARCHITECTURE_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    printf 'v0.14.1 architecture smoke dry run: repo=%s\n' "$repo_root"
    run bash -n scripts/check-architecture-size.sh scripts/test-v0141-architecture-smoke.sh
    exit 0
fi

printf 'v0.14.1 architecture smoke: script syntax\n'
run bash -n \
    scripts/check-architecture-size.sh \
    scripts/check-release-candidate.sh \
    scripts/check-release-readiness.sh \
    scripts/test-v0141-architecture-smoke.sh

printf 'v0.14.1 architecture smoke: source size guard\n'
run scripts/check-architecture-size.sh

printf 'v0.14.1 architecture smoke passed\n'
