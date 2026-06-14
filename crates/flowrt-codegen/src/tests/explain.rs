use super::*;

#[test]
fn explain_report_text_exposes_user_api_and_runtime_context() {
    let ir = contract_from_source(
        r#"
[package]
name = "explain_demo"
rsdl_version = "0.1"

[type.Scan]
range = "f32"

[type.Cmd]
speed = "f32"

[type.PlanRequest]
goal = "u32"

[type.PlanResponse]
accepted = "bool"

[type.PlanGoal]
target = "u32"

[type.PlanFeedback]
progress = "f32"

[type.PlanResult]
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
gain = { type = "f32", default = 1.0, update = "on_tick" }

[component.controller.operation_client.navigate]
goal = "PlanGoal"
feedback = "PlanFeedback"
result = "PlanResult"

[component.plan_service]
language = "rust"
service_server = ["plan:PlanRequest->PlanResponse"]

[component.navigator]
language = "rust"

[component.navigator.operation_server.navigate]
goal = "PlanGoal"
feedback = "PlanFeedback"
result = "PlanResult"

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
input = ["scan"]
output = ["cmd"]

[[instance.controller.task]]
name = "heartbeat"
trigger = "periodic"
period_ms = 50
lane = "control_lane"
concurrency = "parallel"
output = ["cmd"]

[instance.plan_svc]
component = "plan_service"

[instance.plan_svc.task]
trigger = "periodic"
period_ms = 1000

[instance.navigator]
component = "navigator"

[instance.navigator.task]
trigger = "periodic"
period_ms = 1000

[[bind.dataflow]]
from = "sensor.scan"
to = "controller.scan"
channel = "latest"

[[bind.service]]
client = "controller.plan"
server = "plan_svc.plan"
backend = "inproc"

[[bind.operation]]
client = "controller.navigate"
server = "navigator.navigate"
backend = "inproc"
"#,
    );

    let report = explain_report(&ir);
    let text = format_explain_report_text(&report);
    let json = serde_json::to_value(&report).unwrap();

    assert!(text.contains("package explain_demo"));
    assert!(text.contains("graph default"));
    assert!(text.contains("profile=default mode=strict"));
    assert!(text.contains(
        "component sensor language=cpp kind=native concurrency=exclusive user_file=app/cpp/components.cpp"
    ));
    assert!(text.contains(
        "component controller language=rust kind=native concurrency=parallel user_file=app/rust/mod.rs"
    ));
    assert!(text.contains("task reactive instance=controller trigger=on_message period=none readiness=all_ready lane=control_lane concurrency=parallel inputs=scan outputs=cmd"));
    assert!(text.contains("task heartbeat instance=controller trigger=periodic period=50ms readiness=any_ready lane=control_lane concurrency=parallel outputs=cmd"));
    assert!(text.contains("on_tick source=rust_trait required=true: fn on_tick(&self, plan: &ServiceClient_controller_plan, navigate: &OperationClient_controller_navigate, scan: flowrt::Latest<'_, Scan>, params: &ControllerParams, cmd: &mut flowrt::Output<Cmd>) -> flowrt::Status"));
    assert!(text.contains("on_params_update source=rust_trait required=false: fn on_params_update(&self, old_params: &ControllerParams, new_params: &ControllerParams, context: &mut flowrt::Context) -> flowrt::Status"));
    assert!(text.contains("scan:Scan arg=scan: flowrt::Latest<'_, Scan> source=sensor.scan"));
    assert!(text.contains("cmd:Cmd arg=cmd: &mut flowrt::Output<Cmd>"));
    assert!(text.contains("resource accel_budget capability=compute.acceleration.inference access=read required=false readiness=lazy health=optional on_failure=degrade"));
    assert!(text.contains("gain:f32 update=on_tick default=1.0"));
    assert!(text.contains("plan:PlanRequest->PlanResponse handle=ServiceClient_controller_plan arg=plan: &ServiceClient_controller_plan backend=inproc server=plan_svc.plan"));
    assert!(text.contains("navigate:PlanGoal->PlanFeedback->PlanResult handle=OperationClient_controller_navigate arg=navigate: &OperationClient_controller_navigate backend=inproc server=navigator.navigate"));

    assert_eq!(json["package"]["name"], "explain_demo");
    assert_eq!(json["graph"]["mode"], "strict");
    let controller = json["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|component| component["name"] == "controller")
        .unwrap();
    assert_eq!(controller["user_file_path"], "app/rust/mod.rs");
    assert_eq!(
        controller["resources"][0]["capability"],
        "compute.acceleration.inference"
    );
    assert_eq!(controller["tasks"][0]["lane"], "control_lane");
    assert_eq!(
        controller["handlers"][4]["signature"],
        "fn on_tick(&self, plan: &ServiceClient_controller_plan, navigate: &OperationClient_controller_navigate, scan: flowrt::Latest<'_, Scan>, params: &ControllerParams, cmd: &mut flowrt::Output<Cmd>) -> flowrt::Status"
    );
}

#[test]
fn explain_json_reuses_app_api_manifest_core_fields() {
    let ir = contract_from_source(
        r#"
[package]
name = "explain_app_api_demo"
rsdl_version = "0.1"

[type.Scan]
range = "f32"

[type.Cmd]
speed = "f32"

[component.sensor]
language = "cpp"
output = ["scan:Scan"]

[component.controller]
language = "rust"
input = ["scan:Scan"]
output = ["cmd:Cmd"]

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

[instance.controller.task]
trigger = "on_message"
input = ["scan"]
output = ["cmd"]

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
"#,
    );

    let explain_json = serde_json::to_value(explain_report(&ir)).unwrap();
    let bundle = emit_artifacts(&ir).unwrap();
    let app_api_json: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "app/app_api.json")).unwrap();

    assert_eq!(
        explain_json["app_api_version"],
        app_api_json["app_api_version"]
    );
    assert_eq!(explain_json["contract"], app_api_json["contract"]);
    assert_eq!(explain_json["package"], app_api_json["package"]);
    assert_eq!(explain_json["graph"], app_api_json["graph"]);
    assert_eq!(explain_json["components"], app_api_json["components"]);
    assert_eq!(explain_json["stubs"], app_api_json["stubs"]);
}

#[test]
fn explain_report_text_handles_c_component_callback_adapter() {
    let ir = contract_from_source(
        r#"
[package]
name = "c_explain_demo"
rsdl_version = "0.1"

[component.driver]
language = "c"

[instance.driver]
component = "driver"

[instance.driver.task]
trigger = "periodic"
period_ms = 10
"#,
    );

    let report = explain_report(&ir);
    let text = format_explain_report_text(&report);

    assert!(text.contains(
        "component driver language=c kind=native concurrency=exclusive user_file=app/c/driver.c"
    ));
    assert!(
        text.contains(
            "on_tick source=c_callback_table required=false: C task callback table entry"
        )
    );
    assert!(text.contains(
        "C callback table: header=flowrt_app/c_components.h factory=flowrt_app_driver_callbacks"
    ));
    assert!(text.contains("trigger=periodic field=run_periodic required=true"));
}
