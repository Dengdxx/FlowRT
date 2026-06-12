#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v091-varframe.XXXXXX")"

cleanup() {
    if [[ -n "${runtime_pid:-}" ]] && kill -0 "$runtime_pid" 2>/dev/null; then
        kill "$runtime_pid" 2>/dev/null || true
        wait "$runtime_pid" 2>/dev/null || true
    fi
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.9.1 variable frame smoke work dir: %s\n' "$work_dir" >&2
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

demo_src="$repo_root/examples/variable_frame_island_demo"
demo_dir="$work_dir/variable_frame_island_demo"
cp -a "$demo_src" "$demo_dir"
rm -rf "$demo_dir/flowrt"

smoke_target_platform="${FLOWRT_SMOKE_TARGET_PLATFORM:-}"
if [[ -n "$smoke_target_platform" ]]; then
    case "$smoke_target_platform" in
        linux-amd64|linux-arm64) ;;
        *)
            printf 'unsupported FLOWRT_SMOKE_TARGET_PLATFORM: %s\n' "$smoke_target_platform" >&2
            exit 1
            ;;
    esac

    demo_rsdl="$demo_dir/rsdl/robot.rsdl"
    if ! grep -qE '^platform = "linux-(amd64|arm64)"$' "$demo_rsdl"; then
        printf 'variable frame island demo target platform line is missing: %s\n' "$demo_rsdl" >&2
        exit 1
    fi
    # CI 在不同架构 runner 上运行同一 demo；只改临时副本，示例源码保持通用。
    sed -i -E "s/^platform = \"linux-(amd64|arm64)\"$/platform = \"$smoke_target_platform\"/" "$demo_rsdl"
fi

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
grep -q 'package=variable_frame_island_demo' "$work_dir/list.out"
grep -q 'mode=island' "$work_dir/list.out"
grep -q 'boundary input scan_in' "$work_dir/list.out"
grep -q 'boundary output summary_out' "$work_dir/list.out"

run_flowrt run "$demo_dir/rsdl/robot.rsdl" --process main > "$work_dir/run.out" 2>&1 &
runtime_pid=$!

socket=""
for _ in $(seq 1 80); do
    if run_flowrt status --live-only > "$work_dir/status.out" 2>/dev/null && \
        grep -q 'package=variable_frame_island_demo' "$work_dir/status.out"; then
        socket="$(awk '/package=variable_frame_island_demo/ && /process=main/ && /runtime=rust/ {
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
    printf 'failed to discover variable_frame_island_demo runtime socket\n' >&2
    sed -n '1,220p' "$work_dir/status.out" >&2 || true
    sed -n '1,220p' "$work_dir/run.out" >&2 || true
    exit 1
fi

run_flowrt pub scan_in \
    --file "$demo_dir/samples/scan.jsonl" \
    --freq 1000 \
    --image "$demo_dir/flowrt/selfdesc/selfdesc.json" \
    --socket "$socket" \
    --published-at-ms 2000 > "$work_dir/pub.out"
grep -q 'summary: endpoint=scan_in sent=2' "$work_dir/pub.out"

for _ in $(seq 1 80); do
    if run_flowrt echo summary_out --image "$demo_dir/flowrt/selfdesc/selfdesc.json" \
        --socket "$socket" \
        > "$work_dir/echo.out" 2>/dev/null && grep -q 'fields={' "$work_dir/echo.out"; then
        break
    fi
    sleep 0.05
done

grep -q 'seq=2' "$work_dir/echo.out"
grep -q 'count=4' "$work_dir/echo.out"
grep -q 'mean_milli=1250' "$work_dir/echo.out"

kill "$runtime_pid" 2>/dev/null || true
wait "$runtime_pid" 2>/dev/null || true
runtime_pid=""
printf 'v0.9.1 variable frame island demo smoke passed\n'
