#!/usr/bin/env bash
# v0.25.0 iox2 Service/Operation focused smoke。
# 覆盖 control-plane backend resolver、validator gates、runtime iox2 service、
# codegen golden/selfdesc/manifest、示例 prepare，以及可选真实 iox2 SDK build/run。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-v0250-iox2-service-operation.XXXXXX")"
cleanup() {
    if declare -F stop_generated_supervisor >/dev/null; then
        stop_generated_supervisor
    fi
    if [[ "${FLOWRT_KEEP_SMOKE_WORKDIR:-0}" == "1" ]]; then
        printf 'preserved v0.25.0 iox2 service/operation smoke work dir: %s\n' "$work_dir" >&2
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

if [[ "${FLOWRT_V0250_IOX2_SERVICE_OPERATION_SMOKE_DRY_RUN:-0}" == "1" ]]; then
    echo "v0.25.0 iox2 service/operation smoke dry run"
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

find_supervisor_binary() {
    local build_out="$1"
    local supervisor
    if [[ ! -d "$build_out/build/bin/debug" ]]; then
        echo "missing generated binary directory under $build_out/build/bin/debug" >&2
        return 1
    fi
    supervisor="$(find "$build_out/build/bin/debug" -maxdepth 1 -type f -name '*supervisor' -print -quit)"
    if [[ -z "$supervisor" ]]; then
        echo "missing generated supervisor binary under $build_out/build/bin/debug" >&2
        return 1
    fi
    printf '%s\n' "$supervisor"
}

start_generated_supervisor() {
    local build_out="$1"
    local log_path="$2"
    local supervisor
    supervisor="$(find_supervisor_binary "$build_out")"
    printf '+ %q\n' "$supervisor"
    "$supervisor" >"$log_path" 2>&1 &
    FLOWRT_SMOKE_SUPERVISOR_PID="$!"
}

stop_generated_supervisor() {
    local pid="${FLOWRT_SMOKE_SUPERVISOR_PID:-}"
    if [[ -z "$pid" ]]; then
        return
    fi
    if kill -0 "$pid" 2>/dev/null; then
        kill "$pid" 2>/dev/null || true
    fi
    wait "$pid" 2>/dev/null || true
    FLOWRT_SMOKE_SUPERVISOR_PID=""
}

wait_for_live_operation_counter() {
    local package="$1"
    local operation="$2"
    local status_path="$3"
    for _ in $(seq 1 80); do
        if run_flowrt status --live-only --format json >"$status_path" 2>/dev/null &&
            python3 - "$status_path" "$package" "$operation" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as status_file:
    entries = json.load(status_file)

package = sys.argv[2]
operation_name = sys.argv[3]
for entry in entries:
    handshake = entry.get("handshake") or {}
    if handshake.get("package") != package:
        continue
    status = entry.get("status") or {}
    for operation in status.get("operations", []):
        if operation.get("name") != operation_name:
            continue
        started = int(operation.get("total_started", 0))
        succeeded = int(operation.get("succeeded_count", 0))
        if started > 0 and succeeded > 0:
            raise SystemExit(0)
raise SystemExit(1)
PY
        then
            return 0
        fi
        sleep 0.2
    done
    echo "missing live Operation counter package=$package operation=$operation" >&2
    if [[ -s "$status_path" ]]; then
        sed -n '1,120p' "$status_path" >&2
    fi
    return 1
}

echo "v0.25.0 iox2 service/operation smoke: script syntax"
run bash -n scripts/test-v0250-iox2-service-operation-smoke.sh

echo "v0.25.0 iox2 service/operation smoke: IR resolver matrix"
run cargo test -p flowrt-ir -j1 -- control_plane_resolver_matrix

echo "v0.25.0 iox2 service/operation smoke: validator iox2 gates"
run cargo test -p flowrt-validate -j1 -- iox2

echo "v0.25.0 iox2 service/operation smoke: runtime iox2 service"
run cargo test -p flowrt --features iox2 -j1 -- iox2_service

echo "v0.25.0 iox2 service/operation smoke: codegen iox2 service"
run cargo test -p flowrt-codegen -j1 -- iox2_service

echo "v0.25.0 iox2 service/operation smoke: codegen iox2 operation"
run cargo test -p flowrt-codegen -j1 -- iox2_operation

echo "v0.25.0 iox2 service/operation smoke: bounded variable golden"
run cargo test -p flowrt-codegen -j1 -- golden_bounded

echo "v0.25.0 iox2 service/operation smoke: codegen dynamic fallback"
run cargo test -p flowrt-codegen -j1 -- service_iox2_dynamic_fallback

echo "v0.25.0 iox2 service/operation smoke: CLI display separation"
run cargo test -p flowrt-cli -j1 -- self_description_summary_separates_iox2_service_name_and_zenoh_key_expr
run cargo test -p flowrt-cli -j1 -- operation_topology_summary_separates_iox2_service_name_and_zenoh_key_expr
run cargo test -p flowrt-cli -j1 -- self_description_summary_displays_frame_transport_diagnostics

echo "v0.25.0 iox2 service/operation smoke: variable channel switches to bounded iox2"
route_unbounded="$work_dir/variable_route_unbounded.rsdl"
route_bounded="$work_dir/variable_route_bounded.rsdl"
cat > "$route_unbounded" <<'RSDL'
[package]
name = "variable_route_switch"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes"

[component.source]
language = "rust"
output = ["packet:Packet"]

[component.sink]
language = "rust"
input = ["packet:Packet"]

[instance.source]
component = "source"
process = "source_proc"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["packet"]

[instance.sink]
component = "sink"
process = "sink_proc"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["packet"]

[[bind.dataflow]]
from = "source.packet"
to = "sink.packet"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2", "zenoh"]
RSDL
sed 's/payload = "bytes"/payload = "bytes<max=8>"/' "$route_unbounded" > "$route_bounded"

unbounded_out="$work_dir/variable_route_unbounded/flowrt"
bounded_out="$work_dir/variable_route_bounded/flowrt"
run run_flowrt prepare "$route_unbounded" --out-dir "$unbounded_out"
run grep -qF '"backend": "zenoh"' "$unbounded_out/launch/launch.json"
run grep -qF '加 max=N 可留在 iox2' "$unbounded_out/selfdesc/selfdesc.json"
run run_flowrt prepare "$route_bounded" --out-dir "$bounded_out"
run grep -qF '"backend": "iox2"' "$bounded_out/launch/launch.json"
run grep -qF '"iox2_slot_cap_bytes": 16' "$bounded_out/selfdesc/selfdesc.json"
run grep -qF 'Iox2FramePubSub<Packet, 16>' "$bounded_out/rust/src/runtime_shell.rs"
if grep -qF 'ZenohPubSub<Packet>' "$bounded_out/rust/src/runtime_shell.rs"; then
    echo "bounded variable route must stay on iox2 without zenoh pubsub" >&2
    exit 1
fi

echo "v0.25.0 iox2 service/operation smoke: example check"
run run_flowrt check examples/iox2_service_demo/rsdl/robot.rsdl

echo "v0.25.0 iox2 service/operation smoke: example prepare + service name"
demo_out="$work_dir/iox2_service_demo/flowrt"
run run_flowrt prepare examples/iox2_service_demo/rsdl/robot.rsdl --out-dir "$demo_out"
run grep -qF '"service": "FlowRT/service/plan_client_plan"' \
    "$demo_out/selfdesc/selfdesc.json"
run grep -qF '"request_frame": {' "$demo_out/selfdesc/selfdesc.json"
run grep -qF '"start_request_frame": {' "$demo_out/selfdesc/selfdesc.json"
run grep -qF '"iox2_slot_cap_bytes": 40' "$demo_out/selfdesc/selfdesc.json"
run grep -qF 'Iox2FrameServiceClient<PlanRequest, PlanResponse' "$demo_out/rust/src/components.rs"
run grep -qF 'Iox2FrameServiceClient<flowrt::OperationStartRequest<PlanGoal>' "$demo_out/rust/src/components.rs"
if awk '
    /"services": \[/ { in_services = 1 }
    /"operations": \[/ { in_services = 0 }
    in_services && /"key_expr"/ { found = 1 }
    END { exit found ? 0 : 1 }
' "$demo_out/selfdesc/selfdesc.json"; then
    echo "iox2 service selfdesc must not expose zenoh key_expr" >&2
    exit 1
fi
run grep -qF '"start_service": "FlowRT/service/__flowrt_operation_nav_client_nav_start"' \
    "$demo_out/selfdesc/selfdesc.json"

if [[ "${FLOWRT_V0250_REQUIRE_IOX2_SDK:-0}" == "1" ]]; then
    echo "v0.25.0 iox2 service/operation smoke: real iox2 build/run"
    real_demo="$work_dir/iox2_service_demo_real"
    mkdir -p "$real_demo"
    cp -R examples/iox2_service_demo/app "$real_demo/app"
    cp -R examples/iox2_service_demo/rsdl "$real_demo/rsdl"
    run sed -i 's/name = "iox2_service_demo"/name = "iox2_service_demo_smoke"/' \
        "$real_demo/rsdl/robot.rsdl"
    case "$(uname -m)" in
        aarch64|arm64)
            run sed -i 's/platform = "linux-amd64"/platform = "linux-arm64"/' \
                "$real_demo/rsdl/robot.rsdl"
            ;;
    esac
    build_out="$real_demo/flowrt"
    status_out="$real_demo/live-status.json"
    supervisor_log="$real_demo/supervisor.log"
    run run_flowrt deps "$real_demo/rsdl/robot.rsdl" --backend iox2 --build-mode debug
    run run_flowrt build "$real_demo/rsdl/robot.rsdl" --out-dir "$build_out" --build-mode debug --launcher
    start_generated_supervisor "$build_out" "$supervisor_log"
    if ! wait_for_live_operation_counter "iox2_service_demo_smoke" "nav_client.nav" "$status_out"; then
        sed -n '1,120p' "$supervisor_log" >&2 || true
        stop_generated_supervisor
        exit 1
    fi
    stop_generated_supervisor

    echo "v0.25.0 iox2 service/operation smoke: real C++ bounded operation build/run"
    cpp_demo="$work_dir/cpp_bounded_operation_real"
    mkdir -p "$cpp_demo/app/cpp" "$cpp_demo/rsdl"
    cp crates/flowrt-codegen/tests/golden/bounded_operation_iox2_cpp/input.rsdl \
        "$cpp_demo/rsdl/robot.rsdl"
    run sed -i 's/name = "bounded_operation_iox2_cpp"/name = "bounded_operation_iox2_cpp_smoke"/' \
        "$cpp_demo/rsdl/robot.rsdl"
    cat >> "$cpp_demo/rsdl/robot.rsdl" <<'RSDL'

[[process]]
name = "server_proc"
readiness = "runtime_ready"

[[process]]
name = "client_proc"
depends_on = ["server_proc"]
RSDL
    cat > "$cpp_demo/app/cpp/components.cpp" <<'CPP'
#include "flowrt_app/runtime_shell.hpp"

#include <memory>

namespace {

class Controller final : public flowrt_app::ControllerInterface {
public:
    flowrt::Status on_tick(flowrt_app::OperationClient_controller_plan& plan) override {
        if (started_) {
            return flowrt::Status::Ok;
        }
        auto goal = flowrt_app::PlanGoal{};
        goal.target = "dock";
        const auto result = plan.start(goal, 5000);
        if (result.is_err()) {
            return flowrt::Status::Ok;
        }
        if (!result.value().has_value() || !result.value()->accepted) {
            return flowrt::Status::Error;
        }
        started_ = true;
        return flowrt::Status::Ok;
    }

private:
    bool started_ = false;
};

class Navigator final : public flowrt_app::NavigatorInterface {
public:
    flowrt::OperationHandlerResult<flowrt_app::PlanResult> on_plan_operation(
        const flowrt_app::PlanGoal& goal,
        flowrt::OperationCancelToken cancel,
        flowrt::OperationProgressPublisher<flowrt_app::PlanFeedback>& progress) override {
        (void)cancel;
        if (goal.target != "dock") {
            return flowrt::OperationHandlerResult<flowrt_app::PlanResult>::failed();
        }
        progress.publish(flowrt_app::PlanFeedback{.progress = 0.5F});
        progress.publish(flowrt_app::PlanFeedback{.progress = 1.0F});
        return flowrt::OperationHandlerResult<flowrt_app::PlanResult>::succeeded(
            flowrt_app::PlanResult{.accepted = true});
    }

    flowrt::Status on_tick() override { return flowrt::Status::Ok; }
};

}  // namespace

namespace flowrt_user {

flowrt_app::App build_app() {
    return flowrt_app::App(
        std::make_unique<Controller>(),
        std::make_unique<Navigator>());
}

}  // namespace flowrt_user
CPP
    cpp_build_out="$cpp_demo/flowrt"
    cpp_status_out="$cpp_demo/live-status.json"
    cpp_supervisor_log="$cpp_demo/supervisor.log"
    run run_flowrt deps "$cpp_demo/rsdl/robot.rsdl" --backend iox2 --build-mode debug
    run run_flowrt build "$cpp_demo/rsdl/robot.rsdl" --out-dir "$cpp_build_out" \
        --build-mode debug --launcher
    start_generated_supervisor "$cpp_build_out" "$cpp_supervisor_log"
    if ! wait_for_live_operation_counter \
        "bounded_operation_iox2_cpp_smoke" "controller.plan" "$cpp_status_out"; then
        sed -n '1,120p' "$cpp_supervisor_log" >&2 || true
        stop_generated_supervisor
        exit 1
    fi
    stop_generated_supervisor
else
    echo "v0.25.0 iox2 service/operation smoke: skip real iox2 build/run (set FLOWRT_V0250_REQUIRE_IOX2_SDK=1)"
fi

echo "v0.25.0 iox2 service/operation smoke passed"
