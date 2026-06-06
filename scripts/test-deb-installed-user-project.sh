#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-deb-install-test.XXXXXX")"
ros2_router_pid=""
ros2_launch_pid=""

cleanup() {
    if [[ -n "$ros2_launch_pid" ]]; then
        kill "$ros2_launch_pid" 2>/dev/null || true
        wait "$ros2_launch_pid" 2>/dev/null || true
    fi
    if [[ -n "$ros2_router_pid" ]]; then
        kill "$ros2_router_pid" 2>/dev/null || true
        wait "$ros2_router_pid" 2>/dev/null || true
    fi
    rm -rf "$work_dir"
}
trap cleanup EXIT

out_dir="$work_dir/dist"
"$repo_root/scripts/package-deb.sh" --output-dir "$out_dir"
package="$(find "$out_dir" -maxdepth 1 -type f -name 'flowrt_*_*.deb' | sort | head -n 1)"
if [[ -z "$package" ]]; then
    printf 'package-deb.sh did not produce a flowrt deb package\n' >&2
    exit 1
fi

root="$work_dir/root"
dpkg-deb -x "$package" "$root"
version="$(dpkg-deb -f "$package" Version)"

flowrt="$root/opt/flowrt/${version}/bin/flowrt"
if [[ ! -x "$flowrt" ]]; then
    printf 'installed flowrt binary is missing: %s\n' "$flowrt" >&2
    exit 1
fi
private_prefix="$root/opt/flowrt/${version}"

user_root="$work_dir/user-projects"
mkdir -p "$user_root"
cp -a "$repo_root/examples/import_demo" "$user_root/import_demo"
cp -a "$repo_root/examples/cpp_counter_demo" "$user_root/cpp_counter_demo"
cp -a "$repo_root/examples/mixed_iox2_demo" "$user_root/mixed_iox2_demo"
cp -a "$repo_root/examples/mixed_zenoh_demo" "$user_root/mixed_zenoh_demo"
cp -a "$repo_root/examples/ros2_bridge_demo" "$user_root/ros2_bridge_demo"
rm -rf "$user_root/import_demo/flowrt" "$user_root/cpp_counter_demo/flowrt" \
    "$user_root/mixed_iox2_demo/flowrt" "$user_root/mixed_zenoh_demo/flowrt" \
    "$user_root/ros2_bridge_demo/flowrt"

export CARGO_HOME="$work_dir/cargo-home"
export CARGO_NET_OFFLINE=true

"$flowrt" build --launcher "$user_root/import_demo/rsdl/robot.rsdl"
test -x "$user_root/import_demo/flowrt/build/target/debug/import-demo-flowrt-app"

"$flowrt" build --launcher "$user_root/cpp_counter_demo/rsdl/robot.rsdl"
test -x "$user_root/cpp_counter_demo/flowrt/build/cmake/cpp_counter_demo_cpp_app"

"$flowrt" prepare "$user_root/mixed_iox2_demo/rsdl/robot.rsdl"
cmake -S "$user_root/mixed_iox2_demo/flowrt/build" \
    -B "$user_root/mixed_iox2_demo/flowrt/build/cmake" \
    -DFLOWRT_CPP_RUNTIME_DIR="$private_prefix"

"$flowrt" prepare "$user_root/mixed_zenoh_demo/rsdl/robot.rsdl"
cmake -S "$user_root/mixed_zenoh_demo/flowrt/build" \
    -B "$user_root/mixed_zenoh_demo/flowrt/build/cmake" \
    -DFLOWRT_CPP_RUNTIME_DIR="$private_prefix"

if grep -R "$repo_root/runtime/rust" "$user_root/import_demo/flowrt/build/Cargo.toml" \
    "$user_root/cpp_counter_demo/flowrt/build/Cargo.toml"; then
    printf 'generated Cargo manifests unexpectedly reference the FlowRT source repository\n' >&2
    exit 1
fi

if [[ ! -f "$user_root/import_demo/flowrt/build/.cargo/config.toml" ]]; then
    printf 'generated Rust app is missing offline Cargo config\n' >&2
    exit 1
fi
grep -q 'offline = true' "$user_root/import_demo/flowrt/build/.cargo/config.toml"

if grep -R "FLOWRT_CPP_RUNTIME_DIR=${repo_root}/runtime/cpp" "$user_root/cpp_counter_demo/flowrt/build/cmake" 2>/dev/null; then
    printf 'generated CMake cache unexpectedly references the FlowRT source repository\n' >&2
    exit 1
fi

if [[ -f /opt/ros/jazzy/setup.bash ]]; then
    set +u
    # ROS setup scripts read optional unset variables under normal shell settings.
    # shellcheck disable=SC1091
    source /opt/ros/jazzy/setup.bash
    set -u
    if ros2 pkg prefix rmw_zenoh_cpp >/dev/null 2>&1; then
        export RMW_IMPLEMENTATION=rmw_zenoh_cpp
        "$flowrt" build --launcher "$user_root/ros2_bridge_demo/rsdl/robot.rsdl"
        ros2_bridge_binary="$user_root/ros2_bridge_demo/flowrt/build/cmake/ros2_bridge_demo_ros2_bridge"
        test -x "$ros2_bridge_binary"
        ldd "$ros2_bridge_binary" | grep -Fq '/opt/ros/jazzy/opt/zenoh_cpp_vendor/lib/libzenohc.so'

        ros2 run rmw_zenoh_cpp rmw_zenohd >"$work_dir/rmw_zenohd.log" 2>&1 &
        ros2_router_pid="$!"
        sleep 1
        if ! kill -0 "$ros2_router_pid" 2>/dev/null; then
            cat "$work_dir/rmw_zenohd.log" >&2
            printf 'failed to start rmw_zenoh_cpp router for ROS2 bridge smoke\n' >&2
            exit 1
        fi

        FLOWRT_TICK_SLEEP_MS=5 "$flowrt" launch \
            "$user_root/ros2_bridge_demo/rsdl/robot.rsdl" \
            >"$work_dir/ros2_bridge_launch.log" 2>&1 &
        ros2_launch_pid="$!"

        ros2 daemon stop >/dev/null 2>&1 || true
        if ! timeout 20s ros2 topic echo /flowrt/text --once >"$work_dir/ros2_echo.txt"; then
            cat "$work_dir/ros2_bridge_launch.log" >&2 || true
            cat "$work_dir/rmw_zenohd.log" >&2 || true
            printf 'ROS2 bridge echo smoke did not receive /flowrt/text\n' >&2
            exit 1
        fi
        grep -q 'data: flowrt-' "$work_dir/ros2_echo.txt"
    else
        printf 'skipping ROS2 bridge installed smoke: rmw_zenoh_cpp is not installed\n'
    fi
else
    printf 'skipping ROS2 bridge installed smoke: /opt/ros/jazzy/setup.bash not found\n'
fi

printf 'installed deb user-project smoke passed: %s\n' "$package"
