#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
flowrt="${FLOWRT_BIN:-flowrt}"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v081-smoke.XXXXXX")"
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

demo="$work_dir/frame_descriptor_demo"
cp -a "$repo_root/examples/frame_descriptor_demo" "$demo"
rm -rf "$demo/flowrt"
sed -i "s/platform = \"linux-amd64\"/platform = \"$flowrt_platform\"/" "$demo/rsdl/robot.rsdl"

"$flowrt" check "$demo/rsdl/robot.rsdl"
"$flowrt" build --launcher "$demo/rsdl/robot.rsdl"
test -x "$demo/flowrt/build/bin/release/frame-descriptor-demo-flowrt-app"
test -x "$demo/flowrt/build/bin/release/frame-descriptor-demo-flowrt-supervisor"
grep -q '"kind": "io_boundary"' "$demo/flowrt/selfdesc/selfdesc.json"
grep -q '"descriptor"' "$demo/flowrt/selfdesc/selfdesc.json"
grep -q '"port": "frame"' "$demo/flowrt/selfdesc/selfdesc.json"
grep -q '"record_payload": false' "$demo/flowrt/selfdesc/selfdesc.json"

FLOWRT_TICK_SLEEP_MS=10 "$flowrt" run "$demo/rsdl/robot.rsdl" --process main &
runtime_pid="$!"

socket=""
for _ in {1..120}; do
    status="$("$flowrt" status --live-only 2>/dev/null || true)"
    socket="$(
        awk '
            /package=frame_descriptor_demo/ {
                for (i = 1; i <= NF; i++) {
                    if ($i ~ /^socket=/) {
                        sub(/^socket=/, "", $i);
                        print $i;
                        exit;
                    }
                }
            }
        ' <<<"$status"
    )"
    if [[ -n "$socket" ]] &&
        grep -q 'io_boundary=camera component=camera ready=true healthy=true' <<<"$status" &&
        grep -q 'io_boundary_resource=camera.frames kind=shm ready=true' <<<"$status"; then
        break
    fi
    sleep 0.05
done

if [[ -z "$socket" ]]; then
    printf 'failed to discover frame_descriptor_demo runtime socket\n' >&2
    "$flowrt" status || true
    exit 1
fi

"$flowrt" echo camera.frame --socket "$socket" --image "$demo/flowrt/selfdesc/selfdesc.json" |
    tee "$work_dir/echo.out"
grep -q 'descriptor=frame' "$work_dir/echo.out"
grep -q 'frame_descriptor=' "$work_dir/echo.out"
grep -q 'size_bytes=921600' "$work_dir/echo.out"
grep -q 'width=640 height=480 stride_bytes=1920' "$work_dir/echo.out"

record_output="$work_dir/frame.mcap"
"$flowrt" record --output "$record_output" --socket "$socket" --duration 250ms --all --force |
    tee "$work_dir/record.out"
grep -q 'recorded output=' "$work_dir/record.out"
grep -q 'descriptor_payload=descriptor_only' "$work_dir/record.out"
test -s "$record_output"

kill "$runtime_pid" 2>/dev/null || true
wait "$runtime_pid" 2>/dev/null || true
runtime_pid=""

FLOWRT_BENCH_ITERS="${FLOWRT_BENCH_ITERS:-8}" \
FLOWRT_BENCH_PAYLOAD_BYTES="${FLOWRT_BENCH_PAYLOAD_BYTES:-1024}" \
    "$repo_root/scripts/bench-frame-descriptor.sh" |
    tee "$work_dir/bench.out"
grep -q 'frame_descriptor_microbench' "$work_dir/bench.out"
grep -q 'descriptor_roundtrip' "$work_dir/bench.out"
grep -q 'payload_memcpy' "$work_dir/bench.out"

printf 'v0.8.1 installed smoke passed\n'
