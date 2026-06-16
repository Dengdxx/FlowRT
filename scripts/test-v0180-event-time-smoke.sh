#!/usr/bin/env bash
# v0.18.0 Sensor Event-Time focused smoke。
# 范围：sensor sample-time 源声明（RSDL `[type.X.timestamp]` → IR → validator）、record→replay
# 按 sample-time（effective time）步进，以及生成 Rust/C++ shell 对声明 timestamp 源的 boundary
# 改走 register_boundary_input_with_sample_time（typed 提取器）。
#
# 关键：除单元/字符串断言外，本 smoke 真编译生成工程的 event-time 分支——C++ shell 经 g++
# 语法校验，Rust island 经 flowrt build 出终产物。字符串断言不编译生成代码，曾漏掉真实编译错
# （0.18.0 开发期发射的 Rust turbofish 泛型 arity 错即由此类真编译捕获）。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0180-event-time.XXXXXX")"
cleanup() {
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.18.0 event-time smoke work dir: %s\n' "$work_dir" >&2
        return
    fi
    rm -rf "$work_dir"
}
trap cleanup EXIT

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

if [[ "${FLOWRT_V0180_EVENT_TIME_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.18.0 event-time smoke dry run"
    exit 0
fi

if [[ -n "${FLOWRT_BIN:-}" ]]; then
    flowrt_cmd=("$FLOWRT_BIN")
else
    flowrt_cmd=(cargo run -q -p flowrt-cli --)
    export FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=1
fi
run_flowrt() {
    "${flowrt_cmd[@]}" "$@"
}

smoke_target_platform="${FLOWRT_SMOKE_TARGET_PLATFORM:-linux-amd64}"
export FLOWRT_CACHE_DIR="${FLOWRT_CACHE_DIR:-$work_dir/flowrt-cache}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"

echo "v0.18.0 event-time smoke: script syntax"
run bash -n scripts/test-v0180-event-time-smoke.sh

echo "v0.18.0 event-time smoke: sample-time 源声明 IR/validator"
run cargo test -p flowrt-validate timestamp_source -j1

echo "v0.18.0 event-time smoke: 回放时间线按 sample-time 排序 + effective time"
run cargo test -p flowrt-record sample_time -j1

echo "v0.18.0 event-time smoke: ReplayDriver 按 sample-time 步进 + 录制 sample_time_ns"
run cargo test -p flowrt --lib replay_driver_steps_by_sample_time_when_present -j1
run cargo test -p flowrt --lib publish_boundary_input_records_sensor_sample_time_ns -j1

echo "v0.18.0 event-time smoke: codegen 双语言发射 with_sample_time 提取器"
run cargo test -p flowrt-codegen boundary_input_sample_time -j1

echo "v0.18.0 event-time smoke: C++ 录制 sample_time_ns + 回放 effective time ctest"
build_dir="build/cpp-v0180-event-time-smoke"
run cmake -S runtime/cpp -B "$build_dir"
run cmake --build "$build_dir" --target flowrt_runtime_introspection_smoke flowrt_replay_smoke
run ctest --test-dir "$build_dir" \
    -R 'flowrt_runtime_introspection_smoke|flowrt_replay_smoke' --output-on-failure

# 共享 event-time island 契约：sensor 消息声明 timestamp 源，boundary 注入按 sample-time 录制。
write_sensor_rsdl() {
    local path="$1" language="$2" name="$3"
    cat > "$path" <<EOF_RSDL
[package]
name = "$name"
version = "0.1.0"
rsdl_version = "0.1"

[type.ImuSample]
stamp_us = "u32"
ax = "f32"

[type.ImuSample.timestamp]
field = "stamp_us"
unit = "us"
clock_domain = "imu"

[component.sink]
language = "$language"
input = ["imu:ImuSample"]

[instance.sink]
component = "sink"
process = "main"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[profile.default]
mode = "strict"
backend = "inproc"

[target.linux]
platform = "$smoke_target_platform"
runtime = ["$language"]
backends = ["inproc"]
EOF_RSDL
}

echo "v0.18.0 event-time smoke: 生成 C++ event-time island 并 g++ 语法编译"
cpp_dir="$work_dir/cpp_demo"
mkdir -p "$cpp_dir/rsdl"
write_sensor_rsdl "$cpp_dir/rsdl/sensor.rsdl" cpp v0180_event_time_cpp
run run_flowrt prepare "$cpp_dir/rsdl/sensor.rsdl" \
    --temporary-island \
    --boundary-input imu_in=sink.imu \
    --out-dir "$cpp_dir/flowrt"
cpp_shell="$cpp_dir/flowrt/cpp/src/runtime_shell.cpp"
grep -q 'register_boundary_input_with_sample_time<ImuSample>' "$cpp_shell"
grep -q 'flowrt::detail::decode_frame<ImuSample>(payload).stamp_us) \* 1000U' "$cpp_shell"
run g++ -std=c++20 -fsyntax-only \
    -I "$cpp_dir/flowrt/cpp/include" \
    -I runtime/cpp/include \
    "$cpp_shell"

echo "v0.18.0 event-time smoke: 生成 Rust event-time island 并 flowrt build 出终产物"
rust_dir="$work_dir/rust_demo"
mkdir -p "$rust_dir/rsdl" "$rust_dir/app/rust"
write_sensor_rsdl "$rust_dir/rsdl/sensor.rsdl" rust v0180_event_time_rust
# 用户组件实现（crate:: 路径；生成 stub 是面向独立 crate 的模板，临时 island #[path] 内联需 crate::）。
cat > "$rust_dir/app/rust/mod.rs" <<'EOF_RUST'
use crate::components::Sink;
use crate::messages::ImuSample;

#[derive(Default)]
pub struct SinkImpl;

impl Sink for SinkImpl {
    fn on_tick(&mut self, imu: flowrt::Latest<'_, ImuSample>) -> flowrt::Status {
        let _ = imu;
        flowrt::Status::Ok
    }
}

pub fn build_app() -> crate::App {
    crate::App::new(Box::new(SinkImpl::default()))
}
EOF_RUST
run run_flowrt deps --backend inproc --build-mode release --target "$smoke_target_platform"
run run_flowrt build "$rust_dir/rsdl/sensor.rsdl" \
    --temporary-island \
    --boundary-input imu_in=sink.imu \
    --out-dir "$rust_dir/flowrt"
grep -q 'register_boundary_input_with_sample_time::<ImuSample, _>' \
    "$rust_dir/flowrt/rust/src/runtime_shell.rs"
test -x "$rust_dir/flowrt/build/bin/release/v0180-event-time-rust-flowrt-app"

echo "v0.18.0 event-time smoke passed"
