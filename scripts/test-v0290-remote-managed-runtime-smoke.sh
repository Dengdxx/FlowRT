#!/usr/bin/env bash
set -euo pipefail

# v0.29.0 remote managed runtime smoke。

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0290-managed-runtime.XXXXXX")"
flowrt_version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo_root/Cargo.toml" | head -n 1)"

cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.29.0 remote managed runtime smoke work dir: %s\n' "$work_dir" >&2
    else
        rm -rf "$work_dir"
    fi
}
trap cleanup EXIT

run() {
    local cmd="$1"
    shift
    printf '+ %q' "$cmd"
    for arg in "$@"; do
        printf ' %q' "$arg"
    done
    printf '\n'
    "$cmd" "$@"
}

if [[ "${1:-}" == "--dry-run" ]]; then
    echo "v0.29.0 remote managed runtime smoke dry run"
    exit 0
fi

create_managed_bundle() {
    local bundle="$1"
    local label="$2"
    local entry="bin/linux-amd64/flowrt-supervisor"

    mkdir -p "$bundle/bin/linux-amd64"
    cat >"$bundle/$entry" <<EOF
#!/usr/bin/env sh
echo started-$label
trap 'echo stopped-$label; exit 0' TERM
while true; do sleep 1; done
EOF
    chmod +x "$bundle/$entry"
    local hash
    hash="$(sha256sum "$bundle/$entry" | awk '{print $1}')"
    cat >"$bundle/bundle.toml" <<EOF
schema_version = 2
flowrt_version = "$flowrt_version"
package = "managed_smoke"
profile = "default"
artifact_mode = "strict"
temporary_overlay = false
test_only = false
target = "edge"
platform = "linux-amd64"
build_mode = "release"
created_unix_ms = 0
entry = "$entry"
executables = []
external_processes = []
resource_providers = []
runtime_dependencies = []

[[artifacts]]
kind = "supervisor"
target = "edge"
platform = "linux-amd64"
path = "$entry"
sha256 = "$hash"
EOF
}

wait_for_log() {
    local remote_dir="$1"
    local text="$2"
    local logs=""
    for _ in $(seq 1 20); do
        logs="$(cargo run -p flowrt-cli -- managed logs --remote-dir "$remote_dir" --lines 20)"
        if grep -Fq "$text" <<<"$logs"; then
            return 0
        fi
        sleep 0.05
    done
    printf 'expected log text not found: %s\n%s\n' "$text" "$logs" >&2
    return 1
}

echo "v0.29.0 remote managed runtime smoke: tracked workspace_demo layout"
run test -f "$repo_root/examples/workspace_demo/app/rust/mod.rs"
run test -f "$repo_root/examples/workspace_demo/app/perception/rust/processor.rs"
run test -f "$repo_root/examples/workspace_demo/app/control/cpp/src/processor.cpp"
run test -f "$repo_root/examples/workspace_demo/app/control/cpp/inc/control_gain.hpp"
run test ! -e "$repo_root/examples/workspace_demo/app/cpp"
run test ! -e "$repo_root/examples/workspace_demo/app/control/cpp/processor.cpp"

project="$work_dir/workspace_demo"
cp -a "$repo_root/examples/workspace_demo" "$project"
rm -rf "$project/flowrt"

echo "v0.29.0 remote managed runtime smoke: prepare workspace_demo"
run cargo run -p flowrt-cli -- prepare "$project/rsdl/robot.rsdl"
run grep -F '"user_file_path": "app/perception/rust/processor.rs"' \
    "$project/flowrt/app/app_api.json"
run grep -F '"user_file_path": "app/control/cpp/src/processor.cpp"' \
    "$project/flowrt/app/app_api.json"
run grep -F '"path": "app/stubs/control/cpp/src/processor.cpp"' \
    "$project/flowrt/app/app_api.json"
run grep -F 'user file: `app/control/cpp/src/processor.cpp`' \
    "$project/flowrt/app/implementation.md"
run grep -F 'reference stub: `app/stubs/control/cpp/src/processor.cpp`' \
    "$project/flowrt/app/implementation.md"
run grep -F '"${FLOWRT_USER_APP_ROOT}/*/cpp/src/*.cpp"' \
    "$project/flowrt/build/CMakeLists.txt"
run grep -F '"${FLOWRT_USER_APP_ROOT}/*/cpp/inc"' \
    "$project/flowrt/build/CMakeLists.txt"

echo "v0.29.0 remote managed runtime smoke: build workspace_demo"
run env FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=1 cargo run -p flowrt-cli -- deps \
    "$project/rsdl/robot.rsdl" --backend iox2 --build-mode release
sdk_prefix="$("$repo_root/scripts/prepare-ci-iox2-cpp-sdk.sh")"
run env FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=1 CMAKE_PREFIX_PATH="$sdk_prefix" \
    cargo run -p flowrt-cli -- build --launcher "$project/rsdl/robot.rsdl"
run test -x "$project/flowrt/build/bin/release/workspace-demo-flowrt-app"
run test -x "$project/flowrt/build/bin/release/workspace_demo_cpp_app"
run test -x "$project/flowrt/build/bin/release/workspace-demo-flowrt-supervisor"

echo "v0.29.0 remote managed runtime smoke: release lifecycle"
managed_root="$work_dir/managed-root"
bundle_v1="$work_dir/managed-bundle-v1"
bundle_v2="$work_dir/managed-bundle-v2"
create_managed_bundle "$bundle_v1" "v1"
create_managed_bundle "$bundle_v2" "v2"

run cargo run -p flowrt-cli -- managed install "$bundle_v1" \
    --remote-dir "$managed_root" --target edge --activate
run cargo run -p flowrt-cli -- managed install "$bundle_v2" \
    --remote-dir "$managed_root" --target edge --activate
run cargo run -p flowrt-cli -- managed start --remote-dir "$managed_root"
run cargo run -p flowrt-cli -- managed status --remote-dir "$managed_root" --format json
run bash -c "cargo run -p flowrt-cli -- managed status --remote-dir '$managed_root' --format json | grep -F '\"state\": \"running\"'"
wait_for_log "$managed_root" "started-v2"
run cargo run -p flowrt-cli -- managed rollback --remote-dir "$managed_root" --start
wait_for_log "$managed_root" "started-v1"
run cargo run -p flowrt-cli -- managed stop --remote-dir "$managed_root" --timeout-ms 1000
run bash -c "cargo run -p flowrt-cli -- managed status --remote-dir '$managed_root' | grep -F 'managed_run state=stopped'"

echo "v0.29.0 remote managed runtime smoke passed"
