#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

check_optional_structure_guard() {
    local structure_guard_script="scripts/check-architecture-structure.sh"

    if [[ -x "$structure_guard_script" ]]; then
        printf 'v0.15.0 architecture convergence smoke: structure guard\n'
        run "$structure_guard_script"
    elif [[ -e "$structure_guard_script" ]]; then
        printf 'v0.15.0 structure guard exists but is not executable: %s\n' \
            "$structure_guard_script" >&2
        return 1
    else
        printf 'v0.15.0 architecture convergence smoke: structure guard pending\n'
    fi
}

cd "$repo_root"

if [[ "${FLOWRT_V0150_ARCHITECTURE_CONVERGENCE_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    printf 'v0.15.0 architecture convergence smoke dry run: repo=%s\n' "$repo_root"
    run bash -n \
        scripts/check-architecture-size.sh \
        scripts/check-release-candidate.sh \
        scripts/check-release-readiness.sh \
        scripts/release-readiness/v0141-architecture.sh \
        scripts/release-readiness/v0150-architecture-convergence.sh \
        scripts/test-v0141-architecture-smoke.sh \
        scripts/test-v0150-architecture-convergence-smoke.sh
    run cargo run -p flowrt-devtools -- release-gate check-registry 0.15.0
    exit 0
fi

printf 'v0.15.0 architecture convergence smoke: release gate registry\n'
run cargo run -p flowrt-devtools -- release-gate check-registry 0.15.0

printf 'v0.15.0 architecture convergence smoke: script syntax\n'
run bash -n \
    scripts/check-architecture-size.sh \
    scripts/check-release-candidate.sh \
    scripts/check-release-readiness.sh \
    scripts/release-readiness/v0141-architecture.sh \
    scripts/release-readiness/v0150-architecture-convergence.sh \
    scripts/test-v0141-architecture-smoke.sh \
    scripts/test-v0150-architecture-convergence-smoke.sh

printf 'v0.15.0 architecture convergence smoke: source size guard\n'
run scripts/check-architecture-size.sh

check_optional_structure_guard

printf 'v0.15.0 architecture convergence smoke passed\n'
