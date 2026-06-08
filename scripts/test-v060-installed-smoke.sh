#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
flowrt="${FLOWRT_BIN:-flowrt}"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v060-smoke.XXXXXX")"
runtime_pid=""

cleanup() {
    if [[ -n "$runtime_pid" ]] && kill -0 "$runtime_pid" 2>/dev/null; then
        kill "$runtime_pid" 2>/dev/null || true
        wait "$runtime_pid" 2>/dev/null || true
    fi
    rm -rf "$work_dir"
}
trap cleanup EXIT

command -v "$flowrt" >/dev/null
"$flowrt" --version

runtime_dir="$work_dir/runtime"
mkdir -p "$runtime_dir"
chmod 700 "$runtime_dir"
export XDG_RUNTIME_DIR="$runtime_dir"
export FLOWRT_CACHE_DIR="$work_dir/flowrt-cache"

operation_demo="$work_dir/operation_demo"
counter_demo="$work_dir/cpp_counter_demo"
cp -a "$repo_root/examples/operation_demo" "$operation_demo"
cp -a "$repo_root/examples/cpp_counter_demo" "$counter_demo"
rm -rf "$operation_demo/flowrt" "$counter_demo/flowrt"

"$flowrt" deps --backend all --build-mode release
"$flowrt" check "$operation_demo/rsdl/robot.rsdl"
"$flowrt" build --launcher "$operation_demo/rsdl/robot.rsdl"
test -x "$operation_demo/flowrt/build/bin/release/operation-demo-flowrt-app"
test -x "$operation_demo/flowrt/build/bin/release/operation-demo-flowrt-supervisor"
"$flowrt" op list --image "$operation_demo/flowrt/selfdesc/selfdesc.json" |
    grep -q 'operation=controller.plan'
FLOWRT_TICK_SLEEP_MS=5 "$flowrt" run --run-steps 5 "$operation_demo/rsdl/robot.rsdl" --process main

"$flowrt" build --launcher "$counter_demo/rsdl/robot.rsdl"
test -x "$counter_demo/flowrt/build/bin/release/cpp_counter_demo_cpp_app"
test -x "$counter_demo/flowrt/build/bin/release/cpp-counter-demo-flowrt-supervisor"
FLOWRT_TICK_SLEEP_MS=10 "$flowrt" run "$counter_demo/rsdl/robot.rsdl" --process control &
runtime_pid="$!"

socket=""
for _ in {1..100}; do
    status="$("$flowrt" status 2>/dev/null || true)"
    socket="$(
        awk '
            /package=cpp_counter_demo/ {
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
    if [[ -n "$socket" ]]; then
        break
    fi
    sleep 0.05
done

if [[ -z "$socket" ]]; then
    printf 'failed to discover cpp_counter_demo runtime socket\n' >&2
    "$flowrt" status || true
    exit 1
fi

record_output="$work_dir/counter.mcap"
"$flowrt" record --output "$record_output" --socket "$socket" --duration 250ms --all --force |
    tee "$work_dir/record.out"
grep -q 'recorded output=' "$work_dir/record.out"
test -s "$record_output"

kill "$runtime_pid" 2>/dev/null || true
wait "$runtime_pid" 2>/dev/null || true
runtime_pid=""

printf 'v0.6.0 installed smoke passed\n'
