#!/usr/bin/env bash
set -euo pipefail

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v070-smoke.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
flowrt="${FLOWRT_BIN:-flowrt}"
command -v "$flowrt" >/dev/null || {
    printf 'installed flowrt command is required\n' >&2
    exit 1
}

case "$(uname -m)" in
    x86_64) flowrt_platform="linux-amd64" ;;
    aarch64 | arm64) flowrt_platform="linux-arm64" ;;
    *)
        printf 'unsupported smoke test architecture: %s\n' "$(uname -m)" >&2
        exit 1
        ;;
esac

cp -R "$repo_root/examples/external_driver_demo" "$work_dir/"
project="$work_dir/external_driver_demo"
find "$project/rsdl" -type f -name '*.rsdl' -print0 |
    xargs -0 sed -i -E \
        "s/platform = \"linux-(amd64|arm64)\"/platform = \"$flowrt_platform\"/g"

"$flowrt" external check "$project/external/fake_sensor_driver"
"$flowrt" check "$project/rsdl/robot.rsdl"
"$flowrt" deps "$project/rsdl/robot.rsdl" --backend all --build-mode release
"$flowrt" build --launcher "$project/rsdl/robot.rsdl"
test -x "$project/flowrt/build/bin/release/external-driver-demo-flowrt-supervisor"
"$flowrt" launch --run-steps 2 "$project/rsdl/robot.rsdl"
"$flowrt" bundle "$project/rsdl/robot.rsdl" --output "$project/dist/bundle"
test -f "$project/dist/bundle/bundle.toml"
test -x "$project/dist/bundle/bin/$flowrt_platform/external-driver-demo-flowrt-supervisor"
test -x "$project/dist/bundle/external/fake_sensor_driver/bin/driver"
"$flowrt" deploy "$project/dist/bundle" --host dry-run@example.invalid --target edge --remote-dir /tmp/flowrt-v070 --dry-run

printf 'v0.7.0 installed smoke passed\n'
