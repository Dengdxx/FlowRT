#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/test-ros2-bridge-installed.sh --distro DISTRO

Run the installed FlowRT ROS2 bridge smoke test against a ROS2 distribution.
The test is intentionally strict: ROS2, rmw_zenoh_cpp, and an installed
flowrt command must all be present.
EOF
}

ros_distro=""
while [[ "$#" -gt 0 ]]; do
    case "$1" in
        --distro)
            ros_distro="${2:?missing value for --distro}"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            printf 'unknown argument: %s\n' "$1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ -z "$ros_distro" ]]; then
    printf '--distro is required\n' >&2
    usage >&2
    exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
setup_file="/opt/ros/${ros_distro}/setup.bash"
if [[ ! -f "$setup_file" ]]; then
    printf 'ROS2 setup file not found: %s\n' "$setup_file" >&2
    exit 1
fi

command -v flowrt >/dev/null || {
    printf 'installed flowrt command is required\n' >&2
    exit 1
}
flowrt --version

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-ros2-bridge-test.XXXXXX")"
ros2_router_pid=""
flowrt_launch_pid=""

cleanup() {
    if [[ -n "$flowrt_launch_pid" ]]; then
        kill "$flowrt_launch_pid" 2>/dev/null || true
        wait "$flowrt_launch_pid" 2>/dev/null || true
    fi
    if [[ -n "$ros2_router_pid" ]]; then
        kill "$ros2_router_pid" 2>/dev/null || true
        wait "$ros2_router_pid" 2>/dev/null || true
    fi
    rm -rf "$work_dir"
}
trap cleanup EXIT

set +u
# ROS setup scripts read optional unset variables under normal shell settings.
# shellcheck disable=SC1090
source "$setup_file"
set -u

if ! ros2 pkg prefix rmw_zenoh_cpp >/dev/null 2>&1; then
    printf 'rmw_zenoh_cpp is required for ROS2 bridge smoke on %s\n' "$ros_distro" >&2
    exit 1
fi

export RMW_IMPLEMENTATION=rmw_zenoh_cpp
export CARGO_NET_OFFLINE=true
export FLOWRT_CACHE_DIR="${FLOWRT_CACHE_DIR:-$work_dir/flowrt-cache}"

user_root="$work_dir/user-project"
cp -a "$repo_root/examples/ros2_bridge_demo" "$user_root"
rm -rf "$user_root/flowrt"

flowrt deps "$user_root/rsdl/robot.rsdl" --backend zenoh --build-mode release
flowrt build --launcher "$user_root/rsdl/robot.rsdl"
ros2_bridge_binary="$user_root/flowrt/build/bin/release/ros2_bridge_demo_ros2_bridge"
test -x "$ros2_bridge_binary"

ros_prefix="$(ros2 pkg prefix rmw_zenoh_cpp)"
ros_root="${ros_prefix%/share/rmw_zenoh_cpp}"
zenoh_vendor_lib="${ros_root}/opt/zenoh_cpp_vendor/lib/libzenohc.so"
if [[ ! -f "$zenoh_vendor_lib" ]]; then
    printf 'ROS2 zenoh_cpp_vendor libzenohc.so not found: %s\n' "$zenoh_vendor_lib" >&2
    exit 1
fi
ldd "$ros2_bridge_binary" | grep -Fq "$zenoh_vendor_lib"

ros2 run rmw_zenoh_cpp rmw_zenohd >"$work_dir/rmw_zenohd.log" 2>&1 &
ros2_router_pid="$!"
sleep 1
if ! kill -0 "$ros2_router_pid" 2>/dev/null; then
    cat "$work_dir/rmw_zenohd.log" >&2 || true
    printf 'failed to start rmw_zenoh_cpp router for ROS2 bridge smoke\n' >&2
    exit 1
fi

FLOWRT_TICK_SLEEP_MS=5 flowrt launch "$user_root/rsdl/robot.rsdl" \
    >"$work_dir/flowrt-launch.log" 2>&1 &
flowrt_launch_pid="$!"

ros2 daemon stop >/dev/null 2>&1 || true
if ! timeout 25s ros2 topic echo /flowrt/text --once >"$work_dir/ros2-echo.txt"; then
    cat "$work_dir/flowrt-launch.log" >&2 || true
    cat "$work_dir/rmw_zenohd.log" >&2 || true
    printf 'ROS2 bridge echo smoke did not receive /flowrt/text on %s\n' "$ros_distro" >&2
    exit 1
fi

grep -q 'data: flowrt-' "$work_dir/ros2-echo.txt"
printf 'ROS2 bridge installed smoke passed on %s\n' "$ros_distro"
