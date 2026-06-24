#!/usr/bin/env bash
# v0.28.0 module-aware App layout focused smoke。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

if [[ "${FLOWRT_V0280_MODULE_APP_LAYOUT_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.28.0 module app layout smoke dry run"
    exit 0
fi

work_dir="${FLOWRT_V0280_MODULE_APP_LAYOUT_SMOKE_WORK_DIR:-}"
cleanup=true
if [[ -z "$work_dir" ]]; then
    work_dir="$(mktemp -d)"
else
    cleanup=false
    rm -rf "$work_dir"
    mkdir -p "$work_dir"
fi

cleanup_work_dir() {
    local status=$?
    if [[ "$status" -eq 0 && "$cleanup" == true ]]; then
        rm -rf "$work_dir"
    elif [[ "$status" -ne 0 ]]; then
        printf 'preserved v0.28.0 module app layout smoke work dir: %s\n' "$work_dir" >&2
    fi
}
trap cleanup_work_dir EXIT

project="$work_dir/module_layout_demo"
mkdir -p "$project/rsdl/modules" "$project/rsdl/composition" "$project/app/sentinel"
printf 'user-owned\n' >"$project/app/sentinel/keep.txt"

cat >"$project/rsdl/robot.rsdl" <<'RSDL'
[package]
name = "module_layout_demo"
rsdl_version = "0.1"

[workspace]
modules = ["modules/*.rsdl"]
compositions = ["composition/default.rsdl"]
RSDL

cat >"$project/rsdl/modules/mygo_lidar.rsdl" <<'RSDL'
[module]
name = "mygo_lidar"

[component.calibrator]
language = "rust"

[component.driver]
language = "cpp"
RSDL

cat >"$project/rsdl/composition/default.rsdl" <<'RSDL'
[instance.calibrator]
component = "mygo_lidar::calibrator"
process = "calibration_proc"

[instance.calibrator.task]
trigger = "periodic"
period_ms = 1000

[instance.driver]
component = "mygo_lidar::driver"
process = "driver_proc"

[instance.driver.task]
trigger = "periodic"
period_ms = 20

[profile.default]
backend = "inproc"
RSDL

echo "v0.28.0 module app layout smoke: script syntax"
run bash -n scripts/test-v0280-module-app-layout-smoke.sh

echo "v0.28.0 module app layout smoke: codegen regression"
run cargo test -p flowrt-codegen -j1 emits_workspace_module_symbols_without_component_name_collisions
run cargo test -p flowrt-codegen -j1 emits_cpp_managed_app_targets

echo "v0.28.0 module app layout smoke: CLI prepare"
run cargo run -p flowrt-cli -- prepare "$project/rsdl/robot.rsdl"

app_api="$project/flowrt/app/app_api.json"
implementation="$project/flowrt/app/implementation.md"
rust_stub="$project/flowrt/app/stubs/mygo_lidar/rust/calibrator.rs"
cpp_stub="$project/flowrt/app/stubs/mygo_lidar/cpp/driver.cpp"

run test -f "$app_api"
run test -f "$implementation"
run test -f "$rust_stub"
run test -f "$cpp_stub"
run grep -F '"module": "mygo_lidar"' "$app_api"
run grep -F '"user_file_path": "app/mygo_lidar/rust/calibrator.rs"' "$app_api"
run grep -F '"user_file_path": "app/mygo_lidar/cpp/driver.cpp"' "$app_api"
run grep -F '"path": "app/stubs/mygo_lidar/rust/calibrator.rs"' "$app_api"
run grep -F '"path": "app/stubs/mygo_lidar/cpp/driver.cpp"' "$app_api"
run grep -F 'user file: `app/mygo_lidar/rust/calibrator.rs`' "$implementation"
run grep -F 'reference stub: `app/stubs/mygo_lidar/rust/calibrator.rs`' "$implementation"
run grep -F 'user file: `app/mygo_lidar/cpp/driver.cpp`' "$implementation"
run grep -F 'reference stub: `app/stubs/mygo_lidar/cpp/driver.cpp`' "$implementation"
run test -f "$project/app/sentinel/keep.txt"
run test ! -e "$project/app/mygo_lidar"

echo "v0.28.0 module app layout smoke passed"
