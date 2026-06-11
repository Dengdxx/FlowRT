#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v090-island.XXXXXX")"

cleanup() {
    if [[ -n "${runtime_pid:-}" ]] && kill -0 "$runtime_pid" 2>/dev/null; then
        kill "$runtime_pid" 2>/dev/null || true
        wait "$runtime_pid" 2>/dev/null || true
    fi
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.9.0 island smoke work dir: %s\n' "$work_dir" >&2
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

demo_src="$repo_root/examples/island_demo"
demo_dir="$work_dir/island_demo"
cp -a "$demo_src" "$demo_dir"
rm -rf "$demo_dir/flowrt"

export FLOWRT_CACHE_DIR="${FLOWRT_CACHE_DIR:-$work_dir/flowrt-cache}"
export FLOWRT_TICK_SLEEP_MS="${FLOWRT_TICK_SLEEP_MS:-10}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
if [[ "$repo_cli" == "1" ]]; then
    export FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=1
fi

run_flowrt deps "$demo_dir/rsdl/robot.rsdl" --backend inproc > "$work_dir/deps.out"
run_flowrt build --launcher "$demo_dir/rsdl/robot.rsdl" > "$work_dir/build.out"
grep -q 'build summary: target=' "$work_dir/build.out"

run_flowrt list "$demo_dir/flowrt/selfdesc/selfdesc.json" > "$work_dir/list.out"
grep -q 'package=island_demo' "$work_dir/list.out"
grep -q 'mode=island' "$work_dir/list.out"
grep -q 'boundary input sample_in' "$work_dir/list.out"
grep -q 'boundary output result_out' "$work_dir/list.out"

run_flowrt run "$demo_dir/rsdl/robot.rsdl" --process main > "$work_dir/run.out" 2>&1 &
runtime_pid=$!

socket=""
for _ in $(seq 1 80); do
    if run_flowrt status --live-only > "$work_dir/status.out" 2>/dev/null && \
        grep -q 'package=island_demo' "$work_dir/status.out"; then
        socket="$(awk '/package=island_demo/ && /process=main/ && /runtime=rust/ {
            for (i = 1; i <= NF; i++) {
                if ($i ~ /^socket=/) {
                    sub(/^socket=/, "", $i)
                    print $i
                    exit
                }
            }
        }' "$work_dir/status.out")"
        if [[ -n "$socket" ]]; then
            break
        fi
    fi
    sleep 0.05
done
if [[ -z "$socket" ]]; then
    printf 'failed to discover island_demo runtime socket\n' >&2
    sed -n '1,220p' "$work_dir/status.out" >&2 || true
    sed -n '1,220p' "$work_dir/run.out" >&2 || true
    exit 1
fi

run_flowrt pub sample_in \
    --json '{"seq": 7, "value": 21}' \
    --image "$demo_dir/flowrt/selfdesc/selfdesc.json" \
    --socket "$socket" \
    --published-at-ms 1000 > "$work_dir/pub.out"
grep -q 'boundary=sample_in type=Sample revision=' "$work_dir/pub.out"

for _ in $(seq 1 80); do
    if run_flowrt echo result_out --image "$demo_dir/flowrt/selfdesc/selfdesc.json" \
        --socket "$socket" \
        > "$work_dir/echo.out" 2>/dev/null && grep -q 'fields={' "$work_dir/echo.out"; then
        break
    fi
    sleep 0.05
done

grep -q 'seq=7' "$work_dir/echo.out"
grep -q 'doubled=42' "$work_dir/echo.out"

kill "$runtime_pid" 2>/dev/null || true
wait "$runtime_pid" 2>/dev/null || true
runtime_pid=""
printf 'v0.9.0 island demo smoke passed\n'
