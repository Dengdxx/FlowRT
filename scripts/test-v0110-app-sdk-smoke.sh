#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0110-app-sdk.XXXXXX")"

cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.11.0 App SDK smoke work dir: %s\n' "$work_dir" >&2
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

if [[ "${FLOWRT_V0110_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    printf 'v0.11.0 App SDK smoke dry run: target=%s repo_cli=%s\n' \
        "$smoke_target_platform" "$repo_cli"
    exit 0
fi

printf 'v0.11.0 App SDK smoke: flowrt init rust/cpp/c\n'
rust_app="$work_dir/rust_app"
cpp_app="$work_dir/cpp_app"
c_app="$work_dir/c_app"
run_flowrt init "$rust_app" --lang rust > "$work_dir/init-rust.out"
run_flowrt init "$cpp_app" --lang cpp > "$work_dir/init-cpp.out"
run_flowrt init "$c_app" --lang c > "$work_dir/init-c.out"
grep -q 'language=rust' "$work_dir/init-rust.out"
grep -q 'language=cpp' "$work_dir/init-cpp.out"
grep -q 'language=c' "$work_dir/init-c.out"
test -f "$rust_app/flowrt.toml"
test -f "$rust_app/app/rust/mod.rs"
test -f "$cpp_app/app/cpp/components.cpp"
test -f "$c_app/app/c/controller.c"
grep -q 'main = "rsdl/robot.rsdl"' "$rust_app/flowrt.toml"
grep -q 'language = "c"' "$c_app/rsdl/robot.rsdl"

rewrite_rsdl_platforms "$rust_app"
rewrite_rsdl_platforms "$cpp_app"
rewrite_rsdl_platforms "$c_app"

printf 'v0.11.0 App SDK smoke: flowrt add message/component\n'
run_flowrt_at "$rust_app" add message Sample value:u32 > "$work_dir/add-message-rust.out"
run_flowrt_at "$rust_app" add component Source --lang rust --output sample:Sample \
    > "$work_dir/add-component-rust.out"
grep -q 'added message `Sample`' "$work_dir/add-message-rust.out"
grep -q 'added component `source` language=rust' "$work_dir/add-component-rust.out"
grep -q '\[type.Sample\]' "$rust_app/rsdl/robot.rsdl"
grep -q '\[component.source\]' "$rust_app/rsdl/robot.rsdl"
grep -q 'pub struct SourceImpl' "$rust_app/app/rust/mod.rs"

run_flowrt_at "$c_app" add message Sample value:u32 > "$work_dir/add-message-c.out"
run_flowrt_at "$c_app" add component Source --lang c --output sample:Sample \
    > "$work_dir/add-component-c.out"
grep -q 'added component `source` language=c' "$work_dir/add-component-c.out"
test -f "$c_app/app/c/source.c"
grep -q 'flowrt_app_source_callbacks' "$c_app/app/c/source.c"
grep -q 'FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0' "$c_app/app/c/source.c"

printf 'v0.11.0 App SDK smoke: flowrt explain text/json\n'
run_flowrt_at "$rust_app" explain --format text > "$work_dir/explain.txt"
run_flowrt_at "$rust_app" explain --format json > "$work_dir/explain.json"
grep -q 'flowrt explain:' "$work_dir/explain.txt"
grep -q 'component source language=rust kind=native user_file=app/rust/mod.rs' \
    "$work_dir/explain.txt"
grep -q '"name": "source"' "$work_dir/explain.json"
grep -q '"language": "rust"' "$work_dir/explain.json"
grep -q '"user_file_path": "app/rust/mod.rs"' "$work_dir/explain.json"

printf 'v0.11.0 App SDK smoke: examples/c_counter_demo build/run/launch\n'
c_demo="$work_dir/c_counter_demo"
cp -a "$repo_root/examples/c_counter_demo" "$c_demo"
rm -rf "$c_demo/flowrt"
rewrite_rsdl_platforms "$c_demo"

run_flowrt deps "$c_demo/rsdl/robot.rsdl" \
    --backend inproc \
    --target "$smoke_target_platform" \
    --build-mode release > "$work_dir/c-counter-deps.out"
run_flowrt build "$c_demo/rsdl/robot.rsdl" > "$work_dir/c-counter-build.out"
grep -q 'build summary: target=' "$work_dir/c-counter-build.out"
run_flowrt run --run-steps 3 "$c_demo/rsdl/robot.rsdl" > "$work_dir/c-counter-run.out"
run_flowrt build --launcher "$c_demo/rsdl/robot.rsdl" > "$work_dir/c-counter-launcher-build.out"
grep -q 'final_binaries=' "$work_dir/c-counter-launcher-build.out"
grep -q 'c-counter-demo-flowrt-supervisor' "$work_dir/c-counter-launcher-build.out"
run_flowrt launch --run-steps 3 "$c_demo/rsdl/robot.rsdl" > "$work_dir/c-counter-launch.out"

printf 'v0.11.0 App SDK smoke passed\n'
