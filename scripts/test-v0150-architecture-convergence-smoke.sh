#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

check_script_syntax() {
    local script

    for script in "$@"; do
        run bash -n "$script"
    done
}

syntax_scripts=(
    scripts/check-architecture-size.sh
    scripts/check-architecture-contract.sh
    scripts/check-release-candidate.sh
    scripts/check-release-readiness.sh
    scripts/release-readiness/v0141-architecture.sh
    scripts/release-readiness/v0150-architecture-convergence.sh
    scripts/test-v0141-architecture-smoke.sh
    scripts/test-v0150-architecture-convergence-smoke.sh
)

cd "$repo_root"

if [[ "${FLOWRT_V0150_ARCHITECTURE_CONVERGENCE_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    printf 'v0.15.0 architecture convergence smoke dry run: repo=%s\n' "$repo_root"
    check_script_syntax "${syntax_scripts[@]}"
    run cargo run -p flowrt-devtools -- release-gate check-registry 0.15.0
    exit 0
fi

printf 'v0.15.0 architecture convergence smoke: release gate registry\n'
run cargo run -p flowrt-devtools -- release-gate check-registry 0.15.0

printf 'v0.15.0 architecture convergence smoke: script syntax\n'
check_script_syntax "${syntax_scripts[@]}"

printf 'v0.15.0 architecture convergence smoke: source size guard\n'
run scripts/check-architecture-size.sh

printf 'v0.15.0 architecture convergence smoke: architecture contract guard\n'
run scripts/check-architecture-contract.sh

printf 'v0.15.0 architecture convergence smoke passed\n'
