use super::*;

#[test]
fn prepare_workspace_projects_default_profile_when_selection_is_omitted() {
    let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
process = "main"
target = "linux"

[instance.worker.task]
trigger = "periodic"
period_ms = 1

[profile.default]
backend = "inproc"

[profile.iox2]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let rsdl_dir = temp_test_dir("prepare-default-profile");
    let rsdl_path = rsdl_dir.join("robot.rsdl");
    std::fs::create_dir_all(&rsdl_dir).unwrap();
    std::fs::write(&rsdl_path, source).unwrap();
    let out_dir = rsdl_dir.join("flowrt");

    assert!(load_contract_from_rsdl(&rsdl_path).is_err());
    let prepared =
        prepare_workspace(&rsdl_path, &out_dir, None).expect("default profile should prepare");
    let prepared_ir =
        ContractIr::from_json_str(&std::fs::read_to_string(&prepared.contract_path).unwrap())
            .unwrap();

    assert_eq!(prepared_ir.profiles.len(), 1);
    assert_eq!(prepared_ir.profiles[0].name, "default");
    assert_eq!(prepared_ir.deployments.len(), 1);
    assert_eq!(prepared_ir.deployments[0].profile.name, "default");

    let _ = std::fs::remove_dir_all(&rsdl_dir);
}

#[test]
fn prepare_workspace_writes_app_api_artifacts_without_touching_user_app() {
    let source = r#"
[package]
name = "prepare_app_api_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"

[instance.worker.task]
trigger = "periodic"
period_ms = 10
"#;
    let root = temp_test_dir("prepare-app-api");
    let rsdl_dir = root.join("rsdl");
    let user_app = root.join("app/rust/mod.rs");
    let rsdl_path = rsdl_dir.join("robot.rsdl");
    let out_dir = root.join("flowrt");
    std::fs::create_dir_all(&rsdl_dir).unwrap();
    std::fs::create_dir_all(user_app.parent().unwrap()).unwrap();
    std::fs::write(&rsdl_path, source).unwrap();
    std::fs::write(&user_app, "pub struct Existing;\n").unwrap();

    let prepared = prepare_workspace(&rsdl_path, &out_dir, None)
        .expect("prepare should write managed app API artifacts");
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out_dir.join("app/app_api.json")).unwrap())
            .unwrap();

    assert!(prepared.artifact_count > 0);
    assert_eq!(
        manifest["components"][0]["user_file_path"],
        "app/rust/mod.rs"
    );
    assert!(out_dir.join("app/implementation.md").exists());
    assert!(out_dir.join("app/stubs/rust/worker.rs").exists());
    assert_eq!(
        std::fs::read_to_string(&user_app).unwrap(),
        "pub struct Existing;\n"
    );
    assert!(!root.join("app/stubs").exists());
    assert!(!root.join("app/cpp").exists());
    assert!(!root.join("app/c").exists());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn prepare_workspace_applies_temporary_island_overlay_without_rewriting_source() {
    let source = r#"
[package]
name = "temporary_island_demo"
rsdl_version = "0.1"

[type.Scan]
range = "f32"

[type.Command]
speed = "f32"

[component.planner]
language = "rust"
input = ["scan:Scan"]
output = ["cmd:Command"]

[instance.planner]
component = "planner"

[instance.planner.task]
trigger = "on_message"
input = ["scan"]
output = ["cmd"]

[profile.default]
backend = "inproc"
"#;
    let rsdl_dir = temp_test_dir("prepare-temporary-island");
    let rsdl_path = rsdl_dir.join("robot.rsdl");
    std::fs::create_dir_all(&rsdl_dir).unwrap();
    std::fs::write(&rsdl_path, source).unwrap();
    let out_dir = rsdl_dir.join("flowrt");

    assert!(load_contract_from_rsdl(&rsdl_path).is_err());
    let overlay = TemporaryIslandCliOptions {
        enabled: true,
        boundary_inputs: vec!["scan_in=planner.scan".to_string()],
        boundary_outputs: vec!["cmd_out=planner.cmd".to_string()],
    };
    let prepared = prepare_workspace_with_options(&rsdl_path, &out_dir, None, &overlay, None)
        .expect("temporary island overlay should prepare a strict source");
    let prepared_ir =
        ContractIr::from_json_str(&std::fs::read_to_string(&prepared.contract_path).unwrap())
            .unwrap();
    let selfdesc: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(out_dir.join("selfdesc/selfdesc.json")).unwrap(),
    )
    .unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out_dir.join("launch/launch.json")).unwrap())
            .unwrap();

    assert_eq!(std::fs::read_to_string(&rsdl_path).unwrap(), source);
    assert_eq!(prepared_ir.profiles[0].mode, GraphMode::Island);
    assert_eq!(prepared_ir.artifact.mode, GraphMode::Island);
    assert!(prepared_ir.artifact.temporary_island);
    assert!(prepared_ir.artifact.test_only);
    let overlay_metadata = prepared_ir
        .artifact
        .temporary_overlay
        .as_ref()
        .expect("prepare must persist temporary overlay metadata");
    assert_eq!(overlay_metadata.original_profile_mode, GraphMode::Strict);
    assert_eq!(overlay_metadata.generated_by.command, "flowrt prepare");
    assert_eq!(
        overlay_metadata.generated_by.source,
        rsdl_path.display().to_string()
    );
    assert_eq!(overlay_metadata.boundary_mappings.len(), 2);
    assert_eq!(
        overlay_metadata.boundary_mappings[0].source,
        "--boundary-input"
    );
    assert_eq!(
        overlay_metadata.boundary_mappings[1].source,
        "--boundary-output"
    );
    assert_eq!(prepared_ir.graphs[0].boundary_endpoints.len(), 2);
    assert_eq!(selfdesc["artifact"]["temporary_island"], true);
    assert_eq!(selfdesc["artifact"]["test_only"], true);
    assert_eq!(
        selfdesc["artifact"]["temporary_overlay"]["generated_by"]["source"],
        rsdl_path.display().to_string()
    );
    assert_eq!(selfdesc["artifact"]["clock"]["source"], "simulated_replay");
    assert_eq!(launch["artifact"]["temporary_island"], true);
    assert_eq!(launch["artifact"]["test_only"], true);
    assert_eq!(
        launch["artifact"]["temporary_overlay"]["generated_by"]["source"],
        rsdl_path.display().to_string()
    );
    assert_eq!(launch["artifact"]["clock"]["source"], "simulated_replay");

    let _ = std::fs::remove_dir_all(&rsdl_dir);
}

#[test]
fn prepare_workspace_rejects_boundary_flags_without_temporary_island() {
    let source = r#"
[package]
name = "temporary_flag_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
input = ["sample:u32"]

[instance.worker]
component = "worker"

[profile.default]
backend = "inproc"
"#;
    let rsdl_dir = temp_test_dir("prepare-boundary-without-temporary");
    let rsdl_path = rsdl_dir.join("robot.rsdl");
    std::fs::create_dir_all(&rsdl_dir).unwrap();
    std::fs::write(&rsdl_path, source).unwrap();
    let out_dir = rsdl_dir.join("flowrt");
    let overlay = TemporaryIslandCliOptions {
        enabled: false,
        boundary_inputs: vec!["sample_in=worker.sample".to_string()],
        boundary_outputs: vec![],
    };

    let error = prepare_workspace_with_options(&rsdl_path, &out_dir, None, &overlay, None)
        .expect_err("boundary flag without temporary island should fail");

    assert!(error.to_string().contains("require `--temporary-island`"));
    assert_eq!(std::fs::read_to_string(&rsdl_path).unwrap(), source);

    let _ = std::fs::remove_dir_all(&rsdl_dir);
}

#[test]
fn prepare_workspace_applies_fault_injection_overlay_from_scenario_file() {
    let source = r#"
[package]
name = "fault_injection_cli_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.flaky]
language = "rust"
input = ["sample:Sample"]
output = ["echo:Sample"]

[instance.flaky]
component = "flaky"

[instance.flaky.fault]
policy = "restart"
max_restarts = 2
initial_delay_ms = 10
max_delay_ms = 40

[instance.flaky.task]
trigger = "on_message"
input = ["sample"]
output = ["echo"]

[profile.dev]
mode = "island"
backend = "inproc"

[[boundary.input]]
name = "feed"
port = "flaky.sample"
type = "Sample"
"#;
    let rsdl_dir = temp_test_dir("prepare-fault-injection");
    let rsdl_path = rsdl_dir.join("robot.rsdl");
    std::fs::create_dir_all(&rsdl_dir).unwrap();
    std::fs::write(&rsdl_path, source).unwrap();
    let scenario_path = rsdl_dir.join("inject.toml");
    std::fs::write(
        &scenario_path,
        "[[inject]]\nkind = \"panic\"\ninstance = \"flaky\"\ntask = \"main\"\nfrom_invocation = 1\nreason = \"drive restart to terminal\"\n",
    )
    .unwrap();
    let out_dir = rsdl_dir.join("flowrt");

    let overlay = TemporaryIslandCliOptions::default();
    let prepared = prepare_workspace_with_options(
        &rsdl_path,
        &out_dir,
        None,
        &overlay,
        Some(scenario_path.as_path()),
    )
    .expect("fault injection scenario should prepare a test-only contract");

    // 源 RSDL 不被改写。
    assert_eq!(std::fs::read_to_string(&rsdl_path).unwrap(), source);
    let prepared_ir =
        ContractIr::from_json_str(&std::fs::read_to_string(&prepared.contract_path).unwrap())
            .unwrap();
    assert!(prepared_ir.artifact.test_only);
    assert_eq!(
        prepared_ir.artifact.clock_source,
        flowrt_ir::ClockSourceKind::SimulatedReplay
    );
    let fault = prepared_ir
        .artifact
        .fault_injection
        .expect("prepared contract must record fault injection scenario");
    assert_eq!(fault.points.len(), 1);
    assert_eq!(fault.points[0].kind, flowrt_ir::FaultInjectionKind::Panic);
    assert_eq!(fault.points[0].instance.name, "flaky");
    assert_eq!(fault.points[0].from_invocation, Some(1));

    let _ = std::fs::remove_dir_all(&rsdl_dir);
}

#[test]
fn bundle_workspace_rejects_temporary_overlay_even_if_mode_is_tampered_to_strict() {
    let mut contract = contract_from_source(
        r#"
[package]
name = "temporary_overlay_bundle_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "inproc"
"#,
    );
    contract.artifact.test_only = true;
    contract.artifact.temporary_overlay = Some(flowrt_ir::TemporaryOverlayIr {
        kind: "temporary_island".to_string(),
        original_profile_mode: GraphMode::Strict,
        generated_by: flowrt_ir::TemporaryOverlayGenerationIr {
            command: "flowrt prepare".to_string(),
            source: "robot.rsdl".to_string(),
        },
        boundary_mappings: vec![],
    });
    let root = temp_test_dir("bundle-temporary-overlay-reject");
    let rsdl = root.join("robot.rsdl");
    let out_dir = root.join("flowrt");
    let bundle = root.join("dist/temporary");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&rsdl, "").unwrap();

    let error = bundle_workspace(&rsdl, &contract, &out_dir, &bundle, None, false).unwrap_err();

    assert!(error.to_string().contains("temporary overlay"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_rejects_temporary_overlay_even_if_mode_is_tampered_to_strict() {
    let root = temp_test_dir("deploy-temporary-overlay-reject");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "temporary_overlay_demo".into(),
        profile: Some("default".into()),
        artifact_mode: "strict".into(),
        temporary_overlay: true,
        test_only: true,
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![],
        artifacts: vec![],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let error = deploy_bundle(
        &bundle,
        "robot@192.0.2.10",
        "pi",
        "/tmp/flowrt-demo",
        true,
        false,
    )
    .unwrap_err();

    assert!(error.to_string().contains("temporary overlay"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn prepare_workspace_writes_projected_channel_policy_to_managed_artifacts() {
    let source = r#"
[package]
name = "profile_policy_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["defaulted:Sample", "selected:Sample"]

[component.consumer]
language = "rust"
input = ["defaulted:Sample", "selected:Sample"]

[instance.producer]
component = "producer"
process = "main"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 1
output = ["defaulted", "selected"]

[instance.consumer]
component = "consumer"
process = "main"
target = "linux"

[instance.consumer.task]
trigger = "on_message"
input = ["defaulted", "selected"]

[[bind.dataflow]]
from = "producer.defaulted"
to = "consumer.defaulted"
channel = "fifo"
depth = 2

[[bind.dataflow]]
from = "producer.selected"
to = "consumer.selected"
channel = "latest"
overflow = "drop_newest"
stale_policy = "hold_last"
max_age_ms = 7

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[profile.safety]
backend = "inproc"
default_overflow = "error"
default_stale_policy = "drop"
max_age_ms = 25

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let rsdl_dir = temp_test_dir("prepare-profile-policy");
    let rsdl_path = rsdl_dir.join("robot.rsdl");
    std::fs::create_dir_all(&rsdl_dir).unwrap();
    std::fs::write(&rsdl_path, source).unwrap();
    let out_dir = rsdl_dir.join("flowrt");

    let prepared = prepare_workspace(&rsdl_path, &out_dir, Some("safety"))
        .expect("selected profile policy should prepare");
    let prepared_ir =
        ContractIr::from_json_str(&std::fs::read_to_string(&prepared.contract_path).unwrap())
            .unwrap();
    let defaulted_ir = prepared_ir.graphs[0]
        .binds
        .iter()
        .find(|bind| bind.to.port == "defaulted")
        .unwrap();
    let selected_ir = prepared_ir.graphs[0]
        .binds
        .iter()
        .find(|bind| bind.to.port == "selected")
        .unwrap();

    assert_eq!(defaulted_ir.overflow, flowrt_ir::OverflowPolicy::Error);
    assert_eq!(defaulted_ir.stale, flowrt_ir::StalePolicy::Drop);
    assert_eq!(defaulted_ir.max_age_ms, Some(25));
    assert_eq!(selected_ir.overflow, flowrt_ir::OverflowPolicy::DropNewest);
    assert_eq!(selected_ir.stale, flowrt_ir::StalePolicy::HoldLast);
    assert_eq!(selected_ir.max_age_ms, Some(7));

    let launch: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out_dir.join("launch/launch.json")).unwrap())
            .unwrap();
    let channels = launch["graphs"][0]["channels"].as_array().unwrap();
    let defaulted_launch = channels
        .iter()
        .find(|channel| channel["to"] == "consumer.defaulted")
        .unwrap();
    let selected_launch = channels
        .iter()
        .find(|channel| channel["to"] == "consumer.selected")
        .unwrap();

    assert_eq!(defaulted_launch["overflow"], "error");
    assert_eq!(defaulted_launch["stale_policy"], "drop");
    assert_eq!(defaulted_launch["max_age_ms"], 25);
    assert_eq!(selected_launch["overflow"], "drop_newest");
    assert_eq!(selected_launch["stale_policy"], "hold_last");
    assert_eq!(selected_launch["max_age_ms"], 7);

    let _ = std::fs::remove_dir_all(&rsdl_dir);
}
