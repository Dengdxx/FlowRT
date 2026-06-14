#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v092-island.XXXXXX")"
workspace_version="$(
    awk '
        $0 == "[workspace.package]" { inside = 1; next }
        inside && /^\[/ { exit }
        inside && /^version = "/ {
            gsub(/"/, "", $3)
            print $3
            exit
        }
    ' "$repo_root/Cargo.toml"
)"
if [[ -z "$workspace_version" ]]; then
    printf 'failed to read workspace version from Cargo.toml\n' >&2
    exit 1
fi

cleanup() {
    if [[ -n "${runtime_pid:-}" ]] && kill -0 "$runtime_pid" 2>/dev/null; then
        kill "$runtime_pid" 2>/dev/null || true
        wait "$runtime_pid" 2>/dev/null || true
    fi
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.9.2 island replay smoke work dir: %s\n' "$work_dir" >&2
        return
    fi
    rm -rf "$work_dir"
}
trap cleanup EXIT

if [[ -n "${FLOWRT_BIN:-}" ]]; then
    flowrt_cmd=("$FLOWRT_BIN")
    repo_cli=0
elif [[ -f "$repo_root/Cargo.toml" ]]; then
    flowrt_cmd=(cargo run -p flowrt-cli --)
    repo_cli=1
elif command -v flowrt >/dev/null; then
    flowrt_cmd=(flowrt)
    repo_cli=0
else
    printf 'flowrt command is required; set FLOWRT_BIN or run from the FlowRT repository\n' >&2
    exit 1
fi

run_flowrt() {
    (cd "$repo_root" && "${flowrt_cmd[@]}" "$@")
}

smoke_target_platform="${FLOWRT_SMOKE_TARGET_PLATFORM:-linux-amd64}"
case "$smoke_target_platform" in
    linux-amd64|linux-arm64) ;;
    *)
        printf 'unsupported FLOWRT_SMOKE_TARGET_PLATFORM: %s\n' "$smoke_target_platform" >&2
        exit 1
        ;;
esac

export FLOWRT_CACHE_DIR="${FLOWRT_CACHE_DIR:-$work_dir/flowrt-cache}"
export FLOWRT_TICK_SLEEP_MS="${FLOWRT_TICK_SLEEP_MS:-10}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
if [[ "$repo_cli" == "1" ]]; then
    export FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=1
fi

demo_dir="$work_dir/replay_demo"
mkdir -p "$demo_dir/rsdl" "$demo_dir/app/rust" "$demo_dir/dist"

cat > "$demo_dir/rsdl/check.rsdl" <<EOF_RSDL
[package]
name = "v092_check_signature_demo"
version = "0.1.0"
rsdl_version = "0.1"

[type.ScanFrame]
seq = "u32"
ranges = "sequence<f32>"

[type.MotionFrame]
seq = "u32"
speed = "f32"

[type.ValidationSummary]
seq = "u32"
count = "u32"
mean_milli = "i32"

[component.source]
language = "rust"
output = ["scan:ScanFrame"]

[component.motion_source]
language = "rust"
output = ["motion:MotionFrame"]

[component.validator]
language = "rust"
input = ["scan:ScanFrame", "motion:MotionFrame"]
output = ["observed:ScanFrame", "summary:ValidationSummary"]

[component.validator.params]
gain = { type = "f32", default = 2.0, min = 0.1, max = 10.0, update = "on_tick" }

[component.sink]
language = "rust"
input = ["summary:ValidationSummary"]

[instance.source]
component = "source"
process = "main"
target = "linux"

[instance.motion_source]
component = "motion_source"
process = "main"
target = "linux"

[instance.validator]
component = "validator"
process = "main"
target = "linux"

[instance.sink]
component = "sink"
process = "main"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 1000
output = ["scan"]

[instance.motion_source.task]
trigger = "periodic"
period_ms = 1000
output = ["motion"]

[instance.validator.task]
trigger = "on_message"
input = ["scan", "motion"]
output = ["observed", "summary"]

[instance.sink.task]
trigger = "on_message"
input = ["summary"]

[[bind.dataflow]]
from = "source.scan"
to = "validator.scan"
channel = "latest"

[[bind.dataflow]]
from = "motion_source.motion"
to = "validator.motion"
channel = "latest"

[[bind.dataflow]]
from = "validator.summary"
to = "sink.summary"
channel = "latest"

[profile.default]
mode = "strict"
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
max_age_ms = 1000

[target.linux]
platform = "$smoke_target_platform"
runtime = ["rust"]
backends = ["inproc"]
EOF_RSDL

cat > "$demo_dir/rsdl/robot.rsdl" <<EOF_RSDL
[package]
name = "v092_replay_demo"
version = "0.1.0"
rsdl_version = "0.1"

[type.ScanFrame]
seq = "u32"
ranges = "sequence<f32>"

[type.MotionFrame]
seq = "u32"
speed = "f32"

[type.ValidationSummary]
seq = "u32"
count = "u32"
mean_milli = "i32"

[component.validator]
language = "rust"
input = ["scan:ScanFrame", "motion:MotionFrame"]
output = ["observed:ScanFrame", "summary:ValidationSummary"]

[component.validator.params]
gain = { type = "f32", default = 2.0, min = 0.1, max = 10.0, update = "on_tick" }

[instance.validator]
component = "validator"
process = "main"
target = "linux"

[instance.validator.task]
trigger = "on_message"
input = ["scan", "motion"]
output = ["observed", "summary"]

[profile.default]
mode = "strict"
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
max_age_ms = 1000

[target.linux]
platform = "$smoke_target_platform"
runtime = ["rust"]
backends = ["inproc"]
EOF_RSDL

cat > "$demo_dir/app/rust/mod.rs" <<'EOF_RUST'
use crate::components::{Validator, ValidatorParams};
use crate::messages::{MotionFrame, ScanFrame, ValidationSummary};

/// v0.9.2 offline validation smoke 组件。
///
/// 该组件用于脚本内的 temporary island projection 验证：两个 boundary input
/// 由同一份 replay fixture 驱动，输出仍是普通 FlowRT typed message，用户代码
/// 不接触 replay、boundary 或 backend API。
#[derive(Default)]
pub struct ValidatorImpl;

impl Validator for ValidatorImpl {
    fn on_tick(
        &mut self,
        scan: flowrt::Latest<'_, ScanFrame>,
        motion: flowrt::Latest<'_, MotionFrame>,
        params: &ValidatorParams,
        observed: &mut flowrt::Output<ScanFrame>,
        summary: &mut flowrt::Output<ValidationSummary>,
    ) -> flowrt::Status {
        let Some(scan) = scan.as_ref() else {
            return flowrt::Status::Retry;
        };
        if motion.as_ref().is_none() {
            return flowrt::Status::Retry;
        }
        if scan.ranges.is_empty() {
            observed.write(scan.clone());
            summary.write(ValidationSummary {
                seq: scan.seq,
                count: 0,
                mean_milli: 0,
            });
            return flowrt::Status::ok();
        }

        let sum = scan.ranges.iter().copied().sum::<f32>();
        let count = scan.ranges.len() as u32;
        let mean = sum / count as f32 * params.gain;
        observed.write(scan.clone());
        summary.write(ValidationSummary {
            seq: scan.seq,
            count,
            mean_milli: (mean * 1000.0).round() as i32,
        });
        flowrt::Status::ok()
    }
}

/// 组装应用：smoke 只验证 FlowRT 生成 shell 和 CLI 工具链，不引入私有依赖。
pub fn build_app() -> crate::App {
    crate::App::new(Box::new(ValidatorImpl))
}
EOF_RUST

cat > "$demo_dir/fixture.jsonl" <<'EOF_JSONL'
{"boundary":"motion_in","at_ms":0,"payload":{"seq":1,"speed":1.5}}
{"boundary":"scan_in","at_ms":1,"payload":{"seq":1,"ranges":[0.5,1.0,1.5,2.0]}}
{"boundary":"motion_in","dt_ms":1,"payload":{"seq":2,"speed":2.5}}
{"boundary":"scan_in","dt_ms":1,"payload":{"seq":2,"ranges":[1.0,2.0,3.0,4.0,5.0,6.0,7.0,8.0,9.0,10.0,11.0,12.0,13.0,14.0,15.0,16.0,17.0,18.0]}}
EOF_JSONL

run_flowrt check "$demo_dir/rsdl/check.rsdl" > "$work_dir/check.out"
grep -q 'generated user API summary:' "$work_dir/check.out"
grep -q 'fn on_tick(&mut self, scan: flowrt::Latest' "$work_dir/check.out"
grep -q 'params: &ValidatorParams' "$work_dir/check.out"

run_flowrt deps --backend inproc > "$work_dir/deps.out"
run_flowrt build --launcher "$demo_dir/rsdl/robot.rsdl" \
    --temporary-island \
    --boundary-input scan_in=validator.scan \
    --boundary-input motion_in=validator.motion \
    --boundary-output observed_out=validator.observed \
    --boundary-output summary_out=validator.summary \
    > "$work_dir/build.out"
grep -q 'temporary_island=true test_only=true' "$work_dir/build.out"
grep -q 'build summary: target=' "$work_dir/build.out"

run_flowrt list "$demo_dir/flowrt/selfdesc/selfdesc.json" > "$work_dir/list.out"
grep -q 'package=v092_replay_demo' "$work_dir/list.out"
grep -q 'artifact_mode=island temporary_island=true test_only=true' "$work_dir/list.out"
grep -q 'boundary input motion_in' "$work_dir/list.out"
grep -q 'boundary input scan_in' "$work_dir/list.out"
grep -q 'boundary output observed_out' "$work_dir/list.out"
grep -q 'boundary output summary_out' "$work_dir/list.out"

if run_flowrt bundle "$demo_dir/rsdl/robot.rsdl" \
    --output "$demo_dir/dist/bundle" > "$work_dir/bundle.out" 2>&1; then
    printf 'bundle unexpectedly accepted temporary island artifact\n' >&2
    exit 1
fi
grep -q 'refusing to bundle temporary overlay' "$work_dir/bundle.out"

run_flowrt run "$demo_dir/rsdl/robot.rsdl" \
    --process main \
    --temporary-island \
    --boundary-input scan_in=validator.scan \
    --boundary-input motion_in=validator.motion \
    --boundary-output observed_out=validator.observed \
    --boundary-output summary_out=validator.summary \
    > "$work_dir/run.out" 2>&1 &
runtime_pid=$!

for _ in $(seq 1 100); do
    if run_flowrt echo summary_out --image "$demo_dir/flowrt/selfdesc/selfdesc.json" \
        > "$work_dir/echo-ready.out" 2>/dev/null; then
        break
    fi
    sleep 0.05
done
if ! grep -q 'channel=summary_out' "$work_dir/echo-ready.out" 2>/dev/null; then
    printf 'failed to discover v0.9.2 replay demo runtime socket\n' >&2
    run_flowrt status --live-only > "$work_dir/status.out" 2>/dev/null || true
    sed -n '1,220p' "$work_dir/status.out" >&2 || true
    sed -n '1,220p' "$work_dir/run.out" >&2 || true
    exit 1
fi

run_flowrt replay \
    --file "$demo_dir/fixture.jsonl" \
    --image "$demo_dir/flowrt/selfdesc/selfdesc.json" \
    --as-fast-as-possible \
    > "$work_dir/replay.out"
grep -q 'replay source=' "$work_dir/replay.out"
grep -q 'events=4' "$work_dir/replay.out"
grep -q 'boundaries=2' "$work_dir/replay.out"

for _ in $(seq 1 100); do
    if run_flowrt echo observed_out --image "$demo_dir/flowrt/selfdesc/selfdesc.json" \
        > "$work_dir/echo-scan.out" 2>/dev/null && \
        grep -q 'sequence_summary' "$work_dir/echo-scan.out"; then
        break
    fi
    sleep 0.05
done
grep -q 'ranges=sequence_summary(count=18' "$work_dir/echo-scan.out"

run_flowrt echo observed_out --raw \
    --image "$demo_dir/flowrt/selfdesc/selfdesc.json" \
    > "$work_dir/echo-scan-raw.out"
grep -q 'ranges=\[1.0,2.0,3.0' "$work_dir/echo-scan-raw.out"

for _ in $(seq 1 100); do
    if run_flowrt echo summary_out --image "$demo_dir/flowrt/selfdesc/selfdesc.json" \
        > "$work_dir/echo-summary.out" 2>/dev/null && \
        grep -q 'fields={' "$work_dir/echo-summary.out"; then
        break
    fi
    sleep 0.05
done
grep -q 'seq=2' "$work_dir/echo-summary.out"
grep -q 'count=18' "$work_dir/echo-summary.out"
grep -q 'mean_milli=19000' "$work_dir/echo-summary.out"

kill "$runtime_pid" 2>/dev/null || true
wait "$runtime_pid" 2>/dev/null || true
runtime_pid=""

cat > "$demo_dir/dist/bundle.toml" <<EOF_TOML
schema_version = 2
flowrt_version = "${workspace_version}"
package = "v092_replay_demo"
profile = "default"
artifact_mode = "island"
target = "linux"
platform = "${smoke_target_platform}"
build_mode = "release"
created_unix_ms = 0
entry = "bin/supervisor"
executables = []
external_processes = []
artifacts = []
EOF_TOML

if run_flowrt deploy "$demo_dir/dist" \
    --host robot@192.0.2.10 \
    --target linux \
    --remote-dir /tmp/flowrt-v092 \
    --dry-run > "$work_dir/deploy.out" 2>&1; then
    printf 'deploy unexpectedly accepted island bundle without --allow-island\n' >&2
    exit 1
fi
grep -q 'refusing to deploy island' "$work_dir/deploy.out"

printf 'v0.9.2 island replay smoke passed\n'
