#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0120-authoring.XXXXXX")"

cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.12.0 authoring smoke work dir: %s\n' "$work_dir" >&2
        return
    fi
    rm -rf "$work_dir"
}
trap cleanup EXIT

if [[ -n "${FLOWRT_BIN:-}" ]]; then
    flowrt_cmd=("$FLOWRT_BIN")
    repo_cli=0
elif [[ -f "$repo_root/Cargo.toml" ]]; then
    flowrt_cmd=(cargo run --manifest-path "$repo_root/Cargo.toml" -p flowrt-cli --)
    repo_cli=1
elif command -v flowrt >/dev/null; then
    flowrt_cmd=(flowrt)
    repo_cli=0
else
    printf 'flowrt command is required; set FLOWRT_BIN or run from the FlowRT repository\n' >&2
    exit 1
fi

run_flowrt_at() {
    local cwd="$1"
    shift
    (cd "$cwd" && "${flowrt_cmd[@]}" "$@")
}

run_flowrt() {
    run_flowrt_at "$repo_root" "$@"
}

smoke_target_platform="${FLOWRT_SMOKE_TARGET_PLATFORM:-linux-amd64}"
case "$smoke_target_platform" in
    linux-amd64|linux-arm64) ;;
    *)
        printf 'unsupported FLOWRT_SMOKE_TARGET_PLATFORM: %s\n' "$smoke_target_platform" >&2
        exit 1
        ;;
esac

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
export FLOWRT_BUILD_JOBS="${FLOWRT_BUILD_JOBS:-1}"
export FLOWRT_CACHE_DIR="${FLOWRT_CACHE_DIR:-$work_dir/flowrt-cache}"
export FLOWRT_TICK_SLEEP_MS="${FLOWRT_TICK_SLEEP_MS:-5}"
if [[ "$repo_cli" == "1" ]]; then
    export FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=1
fi

rewrite_rsdl_platforms() {
    local root="$1"
    find "$root/rsdl" -type f -name '*.rsdl' -print0 |
        xargs -0 sed -i -E \
            "s/platform = \"linux-(amd64|arm64)\"/platform = \"$smoke_target_platform\"/g"
}

assert_no_user_app() {
    local root="$1"
    if [[ -e "$root/app" ]]; then
        printf 'unexpected user app directory created: %s/app\n' "$root" >&2
        find "$root/app" -maxdepth 3 -print >&2
        exit 1
    fi
}

assert_prepare_outputs() {
    local root="$1"
    local lang="$2"
    test -s "$root/flowrt/app/app_api.json"
    test -s "$root/flowrt/app/implementation.md"
    test -d "$root/flowrt/app/stubs/$lang"
    find "$root/flowrt/app/stubs/$lang" -type f -print -quit | grep -q .
}

exercise_authoring_project() {
    local lang="$1"
    local project="$work_dir/${lang}_app"
    local component="Source"
    local component_name="source"
    local component_output="$work_dir/add-component-${lang}.out"
    local explain_text="$work_dir/explain-${lang}.txt"
    local explain_json="$work_dir/explain-${lang}.json"

    run_flowrt init "$project" --lang "$lang" > "$work_dir/init-${lang}.out"
    grep -q "language=$lang" "$work_dir/init-${lang}.out"
    test -f "$project/flowrt.toml"
    test -f "$project/rsdl/robot.rsdl"
    assert_no_user_app "$project"

    rewrite_rsdl_platforms "$project"

    run_flowrt_at "$project" add message Sample value:u32 > "$work_dir/add-message-${lang}.out"
    run_flowrt_at "$project" add component "$component" --lang "$lang" --output sample:Sample \
        > "$component_output"
    grep -q 'added message `Sample`' "$work_dir/add-message-${lang}.out"
    grep -q "added component \`$component_name\` language=$lang" "$component_output"
    grep -q 'next run `flowrt prepare` or `flowrt explain`' "$component_output"
    assert_no_user_app "$project"

    run_flowrt_at "$project" check > "$work_dir/check-${lang}.out"
    run_flowrt_at "$project" prepare > "$work_dir/prepare-${lang}.out"
    assert_no_user_app "$project"
    assert_prepare_outputs "$project" "$lang"

    run_flowrt_at "$project" explain --format text > "$explain_text"
    run_flowrt_at "$project" explain --format json > "$explain_json"
    grep -q "component $component_name language=$lang kind=native" "$explain_text"
    grep -q "reference_stub=app/stubs/$lang/" "$explain_text"
    grep -q '"app_api_version"' "$project/flowrt/app/app_api.json"
    grep -q "\"language\": \"$lang\"" "$explain_json"
    grep -q "\"path\": \"app/stubs/$lang/" "$explain_json"
}

exercise_demo() {
    local name="$1"
    local demo="$work_dir/$name"
    cp -a "$repo_root/examples/$name" "$demo"
    rm -rf "$demo/flowrt"
    rewrite_rsdl_platforms "$demo"

    run_flowrt deps "$demo/rsdl/robot.rsdl" \
        --backend inproc \
        --target "$smoke_target_platform" \
        --build-mode release > "$work_dir/${name}-deps.out"
    run_flowrt build "$demo/rsdl/robot.rsdl" > "$work_dir/${name}-build.out"
    grep -q 'build summary: target=' "$work_dir/${name}-build.out"
    test -s "$demo/flowrt/app/app_api.json"
    test -s "$demo/flowrt/app/implementation.md"
    test -d "$demo/flowrt/app/stubs"
    run_flowrt run --run-steps 3 "$demo/rsdl/robot.rsdl" > "$work_dir/${name}-run.out"
}

if [[ "${FLOWRT_V0120_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    printf 'v0.12.0 authoring smoke dry run: target=%s repo_cli=%s\n' \
        "$smoke_target_platform" "$repo_cli"
    exit 0
fi

printf 'v0.12.0 authoring smoke: init/add/check/prepare/explain\n'
exercise_authoring_project rust
exercise_authoring_project cpp
exercise_authoring_project c

printf 'v0.12.0 authoring smoke: rust/cpp/c build/run demos\n'
exercise_demo import_demo
exercise_demo cpp_counter_demo
exercise_demo c_counter_demo

printf 'v0.12.0 authoring smoke passed\n'
