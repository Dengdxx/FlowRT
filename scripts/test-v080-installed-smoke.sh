#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
flowrt="${FLOWRT_BIN:-flowrt}"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v080-smoke.XXXXXX")"
runtime_pid=""

cleanup() {
    if [[ -n "$runtime_pid" ]] && kill -0 "$runtime_pid" 2>/dev/null; then
        kill "$runtime_pid" 2>/dev/null || true
        wait "$runtime_pid" 2>/dev/null || true
    fi
    rm -rf "$work_dir"
}
trap cleanup EXIT

command -v "$flowrt" >/dev/null || {
    printf 'installed flowrt command is required\n' >&2
    exit 1
}
"$flowrt" --version

case "$(uname -m)" in
    x86_64) flowrt_platform="linux-amd64" ;;
    aarch64 | arm64) flowrt_platform="linux-arm64" ;;
    *)
        printf 'unsupported smoke test architecture: %s\n' "$(uname -m)" >&2
        exit 1
        ;;
esac

runtime_dir="$work_dir/runtime"
mkdir -p "$runtime_dir"
chmod 700 "$runtime_dir"
export XDG_RUNTIME_DIR="$runtime_dir"
export FLOWRT_CACHE_DIR="$work_dir/flowrt-cache"

"$flowrt" deps --backend all --build-mode release

mixed_zenoh="$work_dir/mixed_zenoh_demo"
cp -a "$repo_root/examples/mixed_zenoh_demo" "$mixed_zenoh"
rm -rf "$mixed_zenoh/flowrt"
"$flowrt" check "$mixed_zenoh/rsdl/robot.rsdl"
"$flowrt" build --launcher "$mixed_zenoh/rsdl/robot.rsdl"
grep -q '"type_name": "CrossHostFrame"' "$mixed_zenoh/flowrt/selfdesc/selfdesc.json"
grep -q '"abi_kind": "variable_frame"' "$mixed_zenoh/flowrt/selfdesc/selfdesc.json"
FLOWRT_TICK_SLEEP_MS=5 "$flowrt" launch --run-steps 20 "$mixed_zenoh/rsdl/robot.rsdl"

io_demo="$work_dir/io_boundary_demo"
mkdir -p "$io_demo/rsdl" "$io_demo/src/rust"
cat > "$io_demo/rsdl/robot.rsdl" <<EOF
[package]
name = "io_boundary_demo"
version = "0.1.0"
rsdl_version = "0.1"

[type.FrameHandle]
resource_id_hash = "u64"
slot = "u32"
generation = "u64"
size_bytes = "u64"
timestamp_unix_ns = "u64"
width = "u32"
height = "u32"
stride_bytes = "u32"
format_id = "u32"
encoding_id = "u32"
flags = "u32"

[component.camera]
language = "rust"
kind = "io_boundary"
io_side_effect = ["device", "read"]
io_readiness = "resource_ready"
io_health = "runtime_reported"
io_shutdown = "cooperative"
output = ["frame:FrameHandle"]

[component.camera.resource.frames]
kind = "shm"
required = true

[component.camera.resource.frames.descriptor]
kind = "frame"
port = "frame"
format = "rgb8"
encoding = "row_major"
metadata = { width = "640", height = "480", stride_bytes = "1920" }

[instance.camera]
component = "camera"
process = "main"
target = "linux"

[instance.camera.task]
trigger = "periodic"
period_ms = 20
output = ["frame"]

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[target.linux]
platform = "$flowrt_platform"
runtime = ["rust"]
backends = ["inproc"]
EOF

cat > "$io_demo/src/rust/mod.rs" <<'EOF'
use crate::components::Camera;
use crate::messages::FrameHandle;
use flowrt::FrameDescriptorFields;

#[derive(Debug, Default)]
struct CameraBoundary {
    generation: u64,
}

impl Camera for CameraBoundary {
    fn on_start(&mut self, context: &mut flowrt::Context) -> flowrt::Status {
        if let Some(boundary) = context.boundary() {
            boundary.mark_resource_ready("frames");
            boundary.mark_ready();
            boundary.report_healthy();
        }
        flowrt::Status::ok()
    }

    fn on_tick(&mut self, frame: &mut flowrt::Output<FrameHandle>) -> flowrt::Status {
        self.generation += 1;
        frame.write(FrameHandle::from_frame_descriptor_fields(FrameDescriptorFields {
            resource_id_hash: 0xF080,
            slot: 7,
            generation: self.generation,
            size_bytes: 640 * 480 * 3,
            timestamp_unix_ns: self.generation * 20_000_000,
            width: 640,
            height: 480,
            stride_bytes: 1_920,
            format_id: 1,
            encoding_id: 1,
            flags: 0,
        }));
        flowrt::Status::ok()
    }
}

pub fn build_app() -> crate::App {
    crate::App::new(Box::new(CameraBoundary::default()))
}
EOF

"$flowrt" check "$io_demo/rsdl/robot.rsdl"
"$flowrt" build --launcher "$io_demo/rsdl/robot.rsdl"
test -x "$io_demo/flowrt/build/bin/release/io-boundary-demo-flowrt-app"
test -x "$io_demo/flowrt/build/bin/release/io-boundary-demo-flowrt-supervisor"
grep -q '"kind": "io_boundary"' "$io_demo/flowrt/selfdesc/selfdesc.json"
grep -q '"descriptor"' "$io_demo/flowrt/selfdesc/selfdesc.json"
grep -q '"record_payload": false' "$io_demo/flowrt/selfdesc/selfdesc.json"

FLOWRT_TICK_SLEEP_MS=10 "$flowrt" run "$io_demo/rsdl/robot.rsdl" --process main &
runtime_pid="$!"

for _ in {1..100}; do
    status="$("$flowrt" status --live-only 2>/dev/null || true)"
    if grep -q 'package=io_boundary_demo' <<<"$status" &&
        grep -q 'io_boundary=camera component=camera ready=true healthy=true' <<<"$status" &&
        grep -q 'io_boundary_resource=camera.frames kind=shm ready=true' <<<"$status"; then
        break
    fi
    sleep 0.05
done

status="$("$flowrt" status --live-only)"
grep -q 'package=io_boundary_demo' <<<"$status"
grep -q 'io_boundary=camera component=camera ready=true healthy=true' <<<"$status"
grep -q 'io_boundary_resource=camera.frames kind=shm ready=true' <<<"$status"

kill "$runtime_pid" 2>/dev/null || true
wait "$runtime_pid" 2>/dev/null || true
runtime_pid=""

"$flowrt" bundle "$io_demo/rsdl/robot.rsdl" --output "$io_demo/dist/bundle"
test -f "$io_demo/dist/bundle/bundle.toml"
grep -q 'schema_version = 2' "$io_demo/dist/bundle/bundle.toml"
grep -q 'platform = "'"$flowrt_platform"'"' "$io_demo/dist/bundle/bundle.toml"
"$flowrt" deploy "$io_demo/dist/bundle" \
    --host dry-run@example.invalid \
    --target linux \
    --remote-dir /tmp/flowrt-v080 \
    --dry-run |
    tee "$work_dir/deploy.out"
grep -q "artifacts=" "$work_dir/deploy.out"
grep -q "platforms=\\[$flowrt_platform\\]" "$work_dir/deploy.out"

printf 'v0.8.0 installed smoke passed\n'
