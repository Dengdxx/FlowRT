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
language = "rust"
output = ["scan:Scan"]

[component.controller]
language = "rust"
concurrency = "parallel"
input = ["scan:Scan"]
output = ["cmd:Cmd"]
service_client = ["plan:PlanRequest->PlanResponse"]

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
    assert!(text.contains("profile default mode=strict"));
    assert!(
        text.contains("component controller language=rust kind=native user_file=app/rust/mod.rs")
    );
    assert!(text.contains("task reactive trigger=on_message period=none readiness=all_ready lane=control_lane concurrency=parallel"));
    assert!(text.contains("task heartbeat trigger=periodic period=50ms readiness=any_ready lane=control_lane concurrency=parallel"));
    assert!(text.contains("on_tick: fn on_tick(&mut self, plan: &ServiceClient_controller_plan, navigate: &OperationClient_controller_navigate, scan: flowrt::Latest<'_, Scan>, params: &ControllerParams, cmd: &mut flowrt::Output<Cmd>) -> flowrt::Status"));
    assert!(text.contains("on_params_update: fn on_params_update(&mut self, old_params: &ControllerParams, new_params: &ControllerParams, context: &mut flowrt::Context) -> flowrt::Status"));
    assert!(text.contains("inputs: scan:Scan source=sensor.scan"));
    assert!(text.contains("outputs: cmd:Cmd"));
    assert!(text.contains("params: gain:f32 update=on_tick default=1.0"));
    assert!(text.contains("service clients: plan:PlanRequest->PlanResponse handle=ServiceClient_controller_plan backend=inproc server=plan_svc.plan"));
    assert!(text.contains("operation clients: navigate:PlanGoal->PlanFeedback->PlanResult handle=OperationClient_controller_navigate backend=inproc server=navigator.navigate"));

    assert_eq!(json["package"]["name"], "explain_demo");
    assert_eq!(json["graphs"][0]["profiles"][0]["mode"], "strict");
    let controller = json["graphs"][0]["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|component| component["name"] == "controller")
        .unwrap();
    assert_eq!(controller["user_file_path"], "app/rust/mod.rs");
    assert_eq!(controller["tasks"][0]["lane"], "control_lane");
}

#[test]
fn explain_report_text_handles_c_components_without_generated_handler() {
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

    assert!(text.contains("component driver language=c kind=native user_file=app/c/**"));
    assert!(text.contains("on_tick: no generated C on_tick handler yet"));
}
