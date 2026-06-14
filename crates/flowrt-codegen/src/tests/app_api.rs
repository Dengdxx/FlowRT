use super::*;

const APP_API_RSDL: &str = r#"
[package]
name = "app_api_demo"
version = "0.12.0"
rsdl_version = "0.1"

[type.Scan]
range = "f32"

[type.Cmd]
speed = "f32"

[type.PlanRequest]
goal = "u32"

[type.PlanResponse]
accepted = "bool"

[type.NavGoal]
target = "u32"

[type.NavFeedback]
progress = "f32"

[type.NavResult]
done = "bool"

[component.sensor]
language = "cpp"
output = ["scan:Scan"]

[component.controller]
language = "rust"
concurrency = "parallel"
input = ["scan:Scan"]
output = ["cmd:Cmd"]
service_client = ["plan:PlanRequest->PlanResponse"]

[component.controller.resource.accel_budget]
capability = "compute.acceleration.inference"
access = "read"
required = false
readiness = "lazy"
health = "optional"
on_failure = "degrade"

[component.controller.params]
gain = { type = "f32", default = 1.0, min = 0.0, max = 10.0, enum = [1.0, 2.0], update = "on_tick" }

[component.controller.operation_client.navigate]
goal = "NavGoal"
feedback = "NavFeedback"
result = "NavResult"

[component.planner]
language = "cpp"
service_server = ["plan:PlanRequest->PlanResponse"]

[component.navigator]
language = "cpp"

[component.navigator.operation_server.navigate]
goal = "NavGoal"
feedback = "NavFeedback"
result = "NavResult"

[component.c_filter]
language = "c"
input = ["scan:Scan"]
output = ["cmd:Cmd"]

[instance.sensor]
component = "sensor"

[instance.sensor.task]
trigger = "periodic"
period_ms = 10
output = ["scan"]

[instance.controller]
component = "controller"

[[instance.controller.task]]
name = "reactive"
trigger = "on_message"
readiness = "all_ready"
lane = "control_lane"
concurrency = "parallel"
deadline_ms = 25
input = ["scan"]
output = ["cmd"]

[[instance.controller.task]]
name = "heartbeat"
trigger = "periodic"
period_ms = 50
lane = "control_lane"
concurrency = "parallel"
output = ["cmd"]

[instance.planner]
component = "planner"

[instance.planner.task]
trigger = "periodic"
period_ms = 1000

[instance.navigator]
component = "navigator"

[instance.navigator.task]
trigger = "periodic"
period_ms = 1000

[instance.c_filter]
component = "c_filter"

[instance.c_filter.task]
trigger = "on_message"
input = ["scan"]
output = ["cmd"]

[[bind.dataflow]]
from = "sensor.scan"
to = "controller.scan"
channel = "latest"

[[bind.dataflow]]
from = "sensor.scan"
to = "c_filter.scan"
channel = "latest"

[[bind.service]]
client = "controller.plan"
server = "planner.plan"
backend = "inproc"

[[bind.operation]]
client = "controller.navigate"
server = "navigator.navigate"
backend = "inproc"

[profile.default]
backend = "inproc"
worker_threads = 4
"#;

#[test]
fn app_api_manifest_exposes_generated_user_contract() {
    let contract = contract_from_source(APP_API_RSDL);
    let bundle = emit_artifacts(&contract).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "app/app_api.json")).unwrap();

    assert_eq!(manifest["app_api_version"], "0.1");
    assert_eq!(manifest["contract"]["ir_version"], "0.1");
    assert_eq!(manifest["contract"]["schema_version"], "0.1");
    assert_eq!(manifest["contract"]["source_hash"], contract.source_hash);
    assert_eq!(manifest["package"]["name"], "app_api_demo");
    assert_eq!(manifest["package"]["version"], "0.12.0");
    assert_eq!(manifest["package"]["rsdl_version"], "0.1");
    assert_eq!(manifest["graph"]["name"], "default");
    assert_eq!(manifest["graph"]["mode"], "strict");
    assert_eq!(manifest["graph"]["profile"], "default");
    assert_eq!(manifest["graph"]["backend"], "inproc");
    assert_eq!(manifest["graph"]["worker_threads"], 4);

    let components = manifest["components"].as_array().unwrap();
    let controller = components
        .iter()
        .find(|component| component["name"] == "controller")
        .unwrap();
    assert_eq!(controller["language"], "rust");
    assert_eq!(controller["kind"], "native");
    assert_eq!(controller["concurrency"], "parallel");
    assert_eq!(controller["user_file_path"], "app/rust/mod.rs");
    assert_eq!(controller["handlers"][0]["name"], "on_init");
    assert_eq!(
        controller["handlers"][4]["signature"],
        "fn on_tick(&self, plan: &ServiceClient_controller_plan, navigate: &OperationClient_controller_navigate, scan: flowrt::Latest<'_, Scan>, params: &ControllerParams, cmd: &mut flowrt::Output<Cmd>) -> flowrt::Status"
    );
    assert_eq!(
        controller["handlers"][5]["signature"],
        "fn on_params_update(&self, old_params: &ControllerParams, new_params: &ControllerParams, context: &mut flowrt::Context) -> flowrt::Status"
    );
    let reactive = controller["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["name"] == "reactive")
        .unwrap();
    assert_eq!(reactive["trigger"], "on_message");
    assert_eq!(reactive["readiness"], "all_ready");
    assert_eq!(reactive["lane"], "control_lane");
    assert_eq!(reactive["concurrency"], "parallel");
    assert_eq!(reactive["deadline_ms"], 25);
    assert_eq!(controller["inputs"][0]["name"], "scan");
    assert_eq!(controller["inputs"][0]["type"], "Scan");
    assert_eq!(controller["inputs"][0]["source"], "sensor.scan");
    assert_eq!(
        controller["inputs"][0]["handler_argument"],
        "scan: flowrt::Latest<'_, Scan>"
    );
    assert_eq!(controller["outputs"][0]["name"], "cmd");
    assert_eq!(controller["outputs"][0]["type"], "Cmd");
    assert_eq!(
        controller["outputs"][0]["handler_argument"],
        "cmd: &mut flowrt::Output<Cmd>"
    );
    assert_eq!(
        controller["resources"][0],
        serde_json::json!({
            "name": "accel_budget",
            "capability": "compute.acceleration.inference",
            "access": "read",
            "required": false,
            "readiness": "lazy",
            "health": "optional",
            "on_failure": "degrade"
        })
    );
    assert_eq!(controller["params"][0]["name"], "gain");
    assert_eq!(controller["params"][0]["type"], "f32");
    assert_eq!(controller["params"][0]["update"], "on_tick");
    assert_eq!(controller["params"][0]["default"], 1.0);
    assert_eq!(controller["params"][0]["min"], 0.0);
    assert_eq!(controller["params"][0]["max"], 10.0);
    assert_eq!(
        controller["params"][0]["choices"],
        serde_json::json!([1.0, 2.0])
    );
    assert_eq!(controller["params_update_hook"], "on_params_update");
    assert_eq!(controller["service_clients"][0]["name"], "plan");
    assert_eq!(
        controller["service_clients"][0]["handle_type"],
        "ServiceClient_controller_plan"
    );
    assert_eq!(controller["service_clients"][0]["server"], "planner.plan");
    assert_eq!(controller["operation_clients"][0]["name"], "navigate");
    assert_eq!(
        controller["operation_clients"][0]["handle_type"],
        "OperationClient_controller_navigate"
    );
    assert_eq!(
        controller["operation_clients"][0]["server"],
        "navigator.navigate"
    );

    let planner = components
        .iter()
        .find(|component| component["name"] == "planner")
        .unwrap();
    assert_eq!(planner["language"], "cpp");
    assert_eq!(planner["user_file_path"], "app/cpp/components.cpp");
    assert_eq!(
        planner["service_servers"][0]["handler_signature"],
        "flowrt::ServiceResult<PlanResponse> on_plan_request(const PlanRequest& request)"
    );

    let navigator = components
        .iter()
        .find(|component| component["name"] == "navigator")
        .unwrap();
    assert_eq!(
        navigator["operation_servers"][0]["handler_signature"],
        "flowrt::OperationHandlerResult<NavResult> on_navigate_operation(const NavGoal& goal, flowrt::OperationCancelToken cancel, flowrt::OperationProgressPublisher<NavFeedback>& progress)"
    );

    let c_filter = components
        .iter()
        .find(|component| component["name"] == "c_filter")
        .unwrap();
    assert_eq!(c_filter["language"], "c");
    assert_eq!(c_filter["user_file_path"], "app/c/c_filter.c");
    assert_eq!(
        c_filter["c_callback_table"]["factory_symbol"],
        "flowrt_app_c_filter_callbacks"
    );
    assert_eq!(
        c_filter["c_callback_table"]["generated_header"],
        "flowrt_app/c_components.h"
    );
    assert_eq!(
        c_filter["c_callback_table"]["task_callbacks"][0]["field"],
        "run_on_message"
    );
    assert_eq!(
        c_filter["inputs"][0]["handler_argument"],
        "flowrt_c_input_view_t scan"
    );
    assert_eq!(
        c_filter["outputs"][0]["handler_argument"],
        "flowrt_c_output_slot_t cmd"
    );
}

#[test]
fn app_api_artifacts_include_implementation_notes_and_reference_stubs() {
    let contract = contract_from_source(APP_API_RSDL);
    let bundle = emit_artifacts(&contract).unwrap();
    let paths = bundle
        .artifacts
        .iter()
        .map(|artifact| artifact.relative_path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    assert!(paths.contains(&"app/app_api.json".to_string()));
    assert!(paths.contains(&"app/implementation.md".to_string()));
    assert!(paths.contains(&"app/stubs/rust/controller.rs".to_string()));
    assert!(paths.contains(&"app/stubs/cpp/planner.cpp".to_string()));
    assert!(paths.contains(&"app/stubs/c/c_filter.c".to_string()));
    assert!(!paths.iter().any(|path| path.starts_with("app/rust/")));
    assert!(!paths.iter().any(|path| path.starts_with("app/cpp/")));
    assert!(!paths.iter().any(|path| path == "app/c/c_filter.c"));

    let implementation = artifact_content(&bundle, "app/implementation.md");
    assert!(implementation.contains("# FlowRT App API 实现清单"));
    assert!(implementation.contains("FlowRT 管理产物，可删除后由 `flowrt prepare` 重建"));
    assert!(implementation.contains("app/rust/mod.rs"));
    assert!(implementation.contains("app/cpp/components.cpp"));
    assert!(implementation.contains("app/c/c_filter.c"));
    assert!(implementation.contains("app/stubs/rust/controller.rs"));
    assert!(implementation.contains("flowrt_app/c_components.h"));
    assert!(implementation.contains("resource accel_budget capability=compute.acceleration.inference access=read required=false readiness=lazy health=optional on_failure=degrade"));

    let rust_stub = artifact_content(&bundle, "app/stubs/rust/controller.rs");
    assert!(rust_stub.contains("impl flowrt_app::components::Controller for Controller"));
    assert!(rust_stub.contains("pub fn build_app() -> flowrt_app::runtime_shell::App"));
    assert!(rust_stub.contains("fn on_tick("));
    assert!(rust_stub.contains("plan: &flowrt_app::components::ServiceClient_controller_plan"));
    assert!(
        rust_stub
            .contains("navigate: &flowrt_app::components::OperationClient_controller_navigate")
    );
    assert!(rust_stub.contains("scan: flowrt::Latest<'_, flowrt_app::messages::Scan>"));
    assert!(rust_stub.contains("params: &flowrt_app::components::ControllerParams"));
    assert!(rust_stub.contains("cmd: &mut flowrt::Output<flowrt_app::messages::Cmd>"));

    let cpp_stub = artifact_content(&bundle, "app/stubs/cpp/planner.cpp");
    assert!(cpp_stub.contains("class Planner final : public flowrt_app::PlannerInterface"));
    assert!(cpp_stub.contains("flowrt::ServiceResult<flowrt_app::PlanResponse> on_plan_request("));
    assert!(cpp_stub.contains("flowrt_app::App build_app()"));

    let c_stub = artifact_content(&bundle, "app/stubs/c/c_filter.c");
    assert!(c_stub.contains("#include \"flowrt_app/c_components.h\""));
    assert!(c_stub.contains("static flowrt_status_t c_filter_run_on_message("));
    assert!(c_stub.contains(".run_on_message = c_filter_run_on_message"));
    assert!(c_stub.contains("flowrt_app_c_filter_callbacks(void)"));
}
