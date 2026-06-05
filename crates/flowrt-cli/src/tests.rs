use clap::CommandFactory;
use flowrt_rsdl::parse_str;
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

fn contract_from_source(source: &str) -> ContractIr {
    let raw = parse_str(source).unwrap();
    let contract = normalize_document(&raw, hash_source(source)).unwrap();
    validate_contract(&contract).unwrap();
    contract
}

fn temp_test_dir(test_name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("flowrt-{test_name}-{}-{nonce}", std::process::id()))
}

#[test]
fn build_plan_selects_cargo_for_rust_contract() {
    let contract = contract_from_source(
        r#"
[package]
name = "rust_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
"#,
    );

    assert_eq!(build_steps(&contract, false), vec![BuildStep::CargoApp]);
    assert_eq!(
        build_steps(&contract, true),
        vec![BuildStep::CargoApp, BuildStep::CargoSupervisor]
    );
}

#[test]
fn build_plan_selects_cmake_for_cpp_contract() {
    let contract = contract_from_source(
        r#"
[package]
name = "cpp_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"
"#,
    );

    assert_eq!(build_steps(&contract, false), vec![BuildStep::CmakeApp]);
    assert_eq!(
        build_steps(&contract, true),
        vec![BuildStep::CmakeApp, BuildStep::CargoSupervisor]
    );
}

#[test]
fn default_build_plan_does_not_build_launcher() {
    let contract = contract_from_source(
        r#"
[package]
name = "cpp_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"
"#,
    );

    assert!(!build_steps(&contract, false).contains(&BuildStep::CargoSupervisor));
    assert!(build_steps(&contract, true).contains(&BuildStep::CargoSupervisor));
}

#[test]
fn build_plan_selects_cargo_and_cmake_for_mixed_contract() {
    let contract = contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[component.cpp_worker]
language = "cpp"

[component.rust_worker]
language = "rust"
"#,
    );

    assert_eq!(
        build_steps(&contract, false),
        vec![BuildStep::CargoApp, BuildStep::CmakeApp]
    );
    assert_eq!(
        build_steps(&contract, true),
        vec![
            BuildStep::CargoApp,
            BuildStep::CmakeApp,
            BuildStep::CargoSupervisor
        ]
    );
}

#[test]
fn run_mode_selects_cmake_app_only_for_cpp_only_contracts() {
    let cpp_contract = contract_from_source(
        r#"
[package]
name = "cpp_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"
"#,
    );
    assert_eq!(run_mode(&cpp_contract), Some(RunMode::CmakeApp));

    let rust_contract = contract_from_source(
        r#"
[package]
name = "rust_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
"#,
    );
    assert_eq!(run_mode(&rust_contract), Some(RunMode::CargoApp));

    let mixed_contract = contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[component.cpp_worker]
language = "cpp"

[component.rust_worker]
language = "rust"
"#,
    );
    assert_eq!(run_mode(&mixed_contract), None);
    assert!(is_mixed_language_contract(&mixed_contract));
    let error = ensure_direct_runtime_supported(&mixed_contract, "run").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("mixed-language `run` requires backend `iox2` or `zenoh`")
    );
}

#[test]
fn run_mode_selects_app_by_process_for_mixed_iox2_contracts() {
    let contract = contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "rust_main"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "cpp_main"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    assert_eq!(
        run_mode_for_process(&contract, Some("rust_main")).unwrap(),
        RunMode::CargoApp
    );
    assert_eq!(
        run_mode_for_process(&contract, Some("cpp_main")).unwrap(),
        RunMode::CmakeApp
    );
    assert!(run_mode_for_process(&contract, None).is_err());
}

#[test]
fn mixed_runtime_readiness_rejects_same_process_mixed_components() {
    let contract = contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "main"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "main"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    let error = ensure_direct_runtime_supported(&contract, "launch").unwrap_err();
    let message = error.to_string();
    assert!(message.contains("process `main`"));
    assert!(message.contains("contains both C++ and Rust components"));
    assert!(message.contains("split them into language-specific RSDL process groups"));
}

#[test]
fn mixed_runtime_readiness_rejects_inproc_cross_process_components() {
    let contract = contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "rust_main"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "cpp_main"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    let error = ensure_direct_runtime_supported(&contract, "launch").unwrap_err();
    let message = error.to_string();
    assert!(message.contains("mixed-language `launch` requires backend `iox2` or `zenoh`"));
    assert!(message.contains("selected backend `inproc`"));
}

#[test]
fn mixed_runtime_readiness_allows_iox2_cross_process_components() {
    let contract = contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "rust_main"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "cpp_main"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    ensure_direct_runtime_supported(&contract, "launch").unwrap();
}

#[test]
fn mixed_runtime_readiness_allows_zenoh_cross_process_components() {
    let contract = contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "rust_main"
target = "dev_host"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "cpp_main"
target = "pi_host"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "zenoh"
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[target.dev_host]
runtime = ["rust"]
backends = ["zenoh"]

[target.pi_host]
runtime = ["cpp"]
backends = ["zenoh"]
"#,
    );

    ensure_direct_runtime_supported(&contract, "launch").unwrap();
    ensure_launch_process_boundaries_supported(&contract).unwrap();
}

#[test]
fn launch_readiness_rejects_inproc_dataflow_across_process_groups() {
    let contract = contract_from_source(
        r#"
[package]
name = "split_rust_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "source_process"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "sink_process"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    let error = ensure_launch_process_boundaries_supported(&contract).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("backend `inproc`"));
    assert!(message.contains("source_process"));
    assert!(message.contains("sink_process"));
}

#[test]
fn run_process_readiness_rejects_inproc_dataflow_across_process_groups() {
    let contract = contract_from_source(
        r#"
[package]
name = "split_rust_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "source_process"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "sink_process"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    let error =
        ensure_run_process_boundaries_supported(&contract, Some("sink_process")).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("backend `inproc`"));
    assert!(message.contains("source_process"));
    assert!(message.contains("sink_process"));
    assert!(message.contains("run --process"));
    ensure_run_process_boundaries_supported(&contract, None).unwrap();
}

#[test]
fn backend_runtime_readiness_allows_cpp_iox2_contracts() {
    let contract = contract_from_source(
        r#"
[package]
name = "cpp_iox2_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    ensure_backend_runtime_supported(&contract, "build").unwrap();
    ensure_backend_runtime_supported(&contract, "run").unwrap();
}

#[test]
fn backend_runtime_readiness_allows_rust_iox2_contracts() {
    let contract = contract_from_source(
        r#"
[package]
name = "rust_iox2_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    ensure_backend_runtime_supported(&contract, "build").unwrap();
}

#[test]
fn cli_exposes_installed_binary_metadata() {
    let command = Cli::command();

    assert_eq!(command.get_name(), "flowrt");
    assert_eq!(command.get_version(), Some(env!("CARGO_PKG_VERSION")));
}

#[test]
fn self_description_sidecar_drives_list_and_nodes_output() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [{
      "name": "source",
      "component": "imu_sim",
      "process": "main",
      "target": null,
      "runtime": "rust"
    }],
    "tasks": [{ "instance": "source", "trigger": "periodic" }],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 8 }]
}
"#;
    let root = temp_test_dir("selfdesc-sidecar");
    let path = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();

    let self_description = load_self_description(&path).unwrap();
    let list = self_description_summary(&self_description);
    let nodes = self_description_nodes(&self_description);

    assert!(list.contains("package=robot_demo"));
    assert!(list.contains("channel source.imu -> sink.imu type=Imu"));
    assert!(list.contains("message Imu size=8"));
    assert!(nodes.contains("source process=main runtime=rust component=imu_sim"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn reads_self_description_from_object_section() {
    let root = temp_test_dir("selfdesc-section");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        r#"[package]
name = "selfdesc-section-test"
version = "0.1.0"
edition = "2024"

[workspace]
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/main.rs"),
        r##"
#[used]
#[unsafe(link_section = ".flowrt.selfdesc")]
static FLOWRT_SELF_DESCRIPTION: [u8; 253] = *br#"{
  "self_description_version": "0.1",
  "source_hash": "feedface",
  "package": { "name": "binary_demo" },
  "graphs": [{ "name": "default", "instances": [], "tasks": [], "channels": [] }],
  "message_abi": [{ "type_name": "Ping", "size_bytes": 4 }]
}
"#;

fn main() {}
"##,
    )
    .unwrap();

    let status = ProcessCommand::new("cargo")
        .arg("build")
        .arg("--quiet")
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(status.success());

    let binary_name = if cfg!(windows) {
        "selfdesc-section-test.exe"
    } else {
        "selfdesc-section-test"
    };
    let binary = root.join("target/debug").join(binary_name);
    let self_description = load_self_description(&binary).unwrap();

    assert_eq!(self_description.package.name, "binary_demo");
    assert_eq!(self_description.message_abi[0].type_name, "Ping");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_summary_reads_runtime_socket_handshake() {
    let root = temp_test_dir("live-status");
    let socket = root.join("main.sock");
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 77,
        started_at_unix_ms: 1234,
        self_description_hash: "feedface".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.register_channel("source.imu_to_sink.imu", "Imu");
    for _ in 0..9 {
        state.record_tick();
    }
    for _ in 0..4 {
        state.record_channel_publish_bytes("source.imu_to_sink.imu", "Imu", vec![0u8; 48], None);
    }
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = live_status_summary_for_sockets(vec![socket]).unwrap();

    assert!(output.contains("pid=77"));
    assert!(output.contains("package=robot_demo"));
    assert!(output.contains("process=main"));
    assert!(output.contains("selfdesc=feedface"));
    assert!(output.contains("ticks=9"));
    assert!(output.contains("channels=1"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_summary_displays_supervisor_process_health() {
    let root = temp_test_dir("live-status-supervisor-health");
    let socket = root.join("supervisor.sock");
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 70,
        started_at_unix_ms: 1234,
        self_description_hash: "feedface".to_string(),
        package: "robot_demo".to_string(),
        process: "flowrt_supervisor".to_string(),
        runtime: "supervisor".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_process_health(flowrt::IntrospectionProcessStatus {
        name: "sensors".to_string(),
        state: "stale".to_string(),
        pid: Some(77),
        restart_count: 2,
        tick_count: Some(10),
        last_seen_unix_ms: Some(2000),
        tick_stale: true,
        exit_code: None,
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = live_status_summary_for_sockets(vec![socket]).unwrap();

    assert!(output.contains("supervisor_process=sensors"));
    assert!(output.contains("state=stale"));
    assert!(output.contains("pid=77"));
    assert!(output.contains("restarts=2"));
    assert!(output.contains("ticks=10"));
    assert!(output.contains("tick_stale=true"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cli_parses_hz_command_with_socket_and_window() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "hz",
        "source.imu_to_sink.imu",
        "--socket",
        "/tmp/flowrt-main.sock",
        "--window-ms",
        "250",
    ])
    .unwrap();

    let Command::Hz {
        channel,
        socket,
        window_ms,
    } = cli.command
    else {
        panic!("hz command should parse into Command::Hz")
    };

    assert_eq!(channel.as_deref(), Some("source.imu_to_sink.imu"));
    assert_eq!(socket, Some(PathBuf::from("/tmp/flowrt-main.sock")));
    assert_eq!(window_ms, 250);
}

#[test]
fn cli_rejects_zero_hz_window() {
    let error = Cli::try_parse_from(["flowrt", "hz", "--window-ms", "0"])
        .expect_err("zero hz window should be rejected");

    assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
}

#[test]
fn live_hz_summary_formats_channel_delta_rate() {
    let first = flowrt::IntrospectionResponse::Status {
        handshake: flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 77,
            started_at_unix_ms: 1234,
            self_description_hash: "feedface".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        },
        status: flowrt::IntrospectionStatus {
            tick_count: 10,
            channels: vec![flowrt::IntrospectionChannelStatus {
                name: "source.imu_to_sink.imu".to_string(),
                message_type: "Imu".to_string(),
                published_count: 100,
                last_payload_len: None,
                active_observers: 0,
                dropped_samples: 0,
            }],
            processes: Vec::new(),
        },
    };
    let second = flowrt::IntrospectionResponse::Status {
        handshake: flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 77,
            started_at_unix_ms: 1234,
            self_description_hash: "feedface".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        },
        status: flowrt::IntrospectionStatus {
            tick_count: 20,
            channels: vec![flowrt::IntrospectionChannelStatus {
                name: "source.imu_to_sink.imu".to_string(),
                message_type: "Imu".to_string(),
                published_count: 150,
                last_payload_len: None,
                active_observers: 0,
                dropped_samples: 0,
            }],
            processes: Vec::new(),
        },
    };

    let output = format_hz_summary_from_status_pair(&first, &second, Duration::from_millis(500))
        .expect("hz summary should format status pair");

    assert!(output.contains("channel=source.imu_to_sink.imu"));
    assert!(output.contains("type=Imu"));
    assert!(output.contains("delta=50"));
    assert!(output.contains("hz=100.00"));
}

#[test]
fn live_hz_summary_reads_status_without_enabling_probe() {
    let root = temp_test_dir("live-hz");
    let socket = root.join("main.sock");
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 77,
        started_at_unix_ms: 1234,
        self_description_hash: "feedface".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.register_channel("source.imu_to_sink.imu", "Imu");
    state.record_channel_publish_bytes("source.imu_to_sink.imu", "Imu", vec![0u8; 48], None);
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state.clone())
        .expect("status server should start");
    let publish_state = state.clone();
    let publisher = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(10));
        for _ in 0..5 {
            publish_state.record_channel_publish_bytes(
                "source.imu_to_sink.imu",
                "Imu",
                vec![0u8; 48],
                None,
            );
        }
    });

    let output = live_hz_summary_for_sockets(
        Some("source.imu_to_sink.imu"),
        vec![socket],
        Duration::from_millis(50),
    )
    .unwrap();
    publisher.join().unwrap();

    assert!(output.contains("channel=source.imu_to_sink.imu"));
    assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(0));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_hz_summary_reports_stale_socket_without_failing_scan() {
    let root = temp_test_dir("live-hz-stale");
    let socket = root.join("missing.sock");
    std::fs::create_dir_all(&root).unwrap();

    let output = live_hz_summary_for_sockets(None, vec![socket.clone()], Duration::from_millis(1))
        .expect("stale socket should be reported as a line");

    assert!(output.contains(&format!("stale socket={}", socket.display())));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cli_parses_echo_command_with_optional_socket() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "echo",
        "flowrt/selfdesc/selfdesc.json",
        "source.imu_to_sink.imu",
        "--socket",
        "/tmp/flowrt-main.sock",
    ])
    .unwrap();

    let Command::Echo {
        target,
        image,
        channel,
        socket,
        follow,
        interval_ms,
    } = cli.command
    else {
        panic!("echo command should parse into Command::Echo")
    };

    assert_eq!(target, "flowrt/selfdesc/selfdesc.json");
    assert_eq!(image, None);
    assert_eq!(channel.as_deref(), Some("source.imu_to_sink.imu"));
    assert_eq!(socket, Some(PathBuf::from("/tmp/flowrt-main.sock")));
    assert!(!follow);
    assert_eq!(interval_ms, 250);
}

#[test]
fn cli_parses_echo_channel_without_image() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "echo",
        "source.imu_to_sink.imu",
        "--socket",
        "/tmp/flowrt-main.sock",
    ])
    .unwrap();

    let Command::Echo {
        target,
        image,
        channel,
        socket,
        ..
    } = cli.command
    else {
        panic!("echo command should parse into Command::Echo")
    };

    assert_eq!(target, "source.imu_to_sink.imu");
    assert_eq!(image, None);
    assert_eq!(channel, None);
    assert_eq!(socket, Some(PathBuf::from("/tmp/flowrt-main.sock")));
}

#[test]
fn cli_parses_echo_image_option() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "echo",
        "source.imu_to_sink.imu",
        "--image",
        "flowrt/selfdesc/selfdesc.json",
    ])
    .unwrap();

    let Command::Echo {
        target,
        image,
        channel,
        ..
    } = cli.command
    else {
        panic!("echo command should parse into Command::Echo")
    };

    assert_eq!(target, "source.imu_to_sink.imu");
    assert_eq!(image, Some(PathBuf::from("flowrt/selfdesc/selfdesc.json")));
    assert_eq!(channel, None);
}

#[test]
fn cli_parses_echo_follow_options() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "echo",
        "flowrt/selfdesc/selfdesc.json",
        "source.imu_to_sink.imu",
        "--follow",
        "--interval-ms",
        "10",
    ]);

    let Command::Echo {
        follow,
        interval_ms,
        ..
    } = cli.unwrap().command
    else {
        panic!("echo --follow should parse into Command::Echo")
    };

    assert!(follow);
    assert_eq!(interval_ms, 10);
}

#[test]
fn cli_parses_params_set_command() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "params",
        "set",
        "flowrt/selfdesc/selfdesc.json",
        "controller.kp",
        "2.5",
        "--socket",
        "/tmp/flowrt-main.sock",
    ])
    .unwrap();

    let Command::Params {
        command:
            ParamsCommand::Set {
                image,
                name,
                value,
                socket,
            },
    } = cli.command
    else {
        panic!("params set command should parse into Command::Params")
    };

    assert_eq!(image, PathBuf::from("flowrt/selfdesc/selfdesc.json"));
    assert_eq!(name, "controller.kp");
    assert_eq!(value, "2.5");
    assert_eq!(socket, Some(PathBuf::from("/tmp/flowrt-main.sock")));
}

#[test]
fn cli_rejects_zero_echo_follow_interval() {
    let error = Cli::try_parse_from([
        "flowrt",
        "echo",
        "flowrt/selfdesc/selfdesc.json",
        "source.imu_to_sink.imu",
        "--follow",
        "--interval-ms",
        "0",
    ])
    .expect_err("zero follow interval should be rejected");

    assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
}

#[test]
fn cli_parses_run_ticks_for_run_and_launch() {
    let run_cli = Cli::try_parse_from([
        "flowrt",
        "run",
        "examples/import_demo/rsdl/robot.rsdl",
        "--process",
        "main",
        "--run-ticks",
        "5",
    ])
    .unwrap();
    let Command::Run {
        process, run_ticks, ..
    } = run_cli.command
    else {
        panic!("run command should parse into Command::Run")
    };
    assert_eq!(process.as_deref(), Some("main"));
    assert_eq!(run_ticks, Some(5));

    let launch_cli = Cli::try_parse_from([
        "flowrt",
        "launch",
        "examples/import_demo/rsdl/robot.rsdl",
        "--run-ticks",
        "7",
    ])
    .unwrap();
    let Command::Launch { run_ticks, .. } = launch_cli.command else {
        panic!("launch command should parse into Command::Launch")
    };
    assert_eq!(run_ticks, Some(7));
}

#[test]
fn cli_parses_build_launcher_flag() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "build",
        "examples/import_demo/rsdl/robot.rsdl",
        "--launcher",
    ])
    .unwrap();

    let Command::Build { launcher, .. } = cli.command else {
        panic!("build command should parse into Command::Build")
    };
    assert!(launcher);
}

#[test]
fn cli_rejects_zero_run_ticks() {
    let error = Cli::try_parse_from([
        "flowrt",
        "run",
        "examples/import_demo/rsdl/robot.rsdl",
        "--run-ticks",
        "0",
    ])
    .expect_err("zero run tick limit should be rejected");

    assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
}

#[test]
fn load_prepared_contract_reports_build_required() {
    let root = temp_test_dir("missing-prepared-contract");
    let out_dir = root.join("flowrt");
    let rsdl = root.join("rsdl/robot.rsdl");

    let build_hint = build_command_hint(&rsdl, None, false);
    let error = load_prepared_contract(&out_dir, &build_hint).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("generated contract"));
    assert!(message.contains("flowrt build"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn prepared_profile_must_match_explicit_run_profile() {
    let contract = contract_from_source(
        r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    let build_hint = build_command_hint(
        Path::new("examples/profile_switch_demo/rsdl/robot.rsdl"),
        Some("iox2"),
        false,
    );
    let error = ensure_prepared_profile_matches(&contract, Some("iox2"), &build_hint).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("prepared FlowRT artifacts use profile `default`"));
    assert!(message.contains("flowrt build --profile iox2"));
}

#[test]
fn build_command_hint_includes_launcher_when_launch_needs_profile() {
    let hint = build_command_hint(
        Path::new("examples/profile_switch_demo/rsdl/robot.rsdl"),
        Some("iox2"),
        true,
    );

    assert_eq!(
        hint,
        "flowrt build --launcher --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl"
    );
}

#[test]
fn launch_workspace_requires_prebuilt_supervisor() {
    let contract = contract_from_source(
        r#"
[package]
name = "launcher_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
"#,
    );
    let root = temp_test_dir("missing-launcher");

    let error = launch_workspace(&contract, &root.join("flowrt"), Some(1)).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("FlowRT supervisor"));
    assert!(message.contains("flowrt build --launcher"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_reads_channel_snapshot_from_fake_status_server() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
    let root = temp_test_dir("echo-snapshot");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 81,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_channel_publish_bytes(
        "source.imu_to_sink.imu",
        "Imu",
        vec![0x01, 0x02, 0x0a, 0xff],
        Some(123),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = echo_channel_from_image(&selfdesc, "source.imu", Some(&socket)).unwrap();

    assert!(output.contains("channel=source.imu_to_sink.imu"));
    assert!(output.contains("type=Imu"));
    assert!(output.contains("published_count=1"));
    assert!(output.contains("published_at_ms=123"));
    assert!(output.contains("payload_len=4"));
    assert!(output.contains("raw=01020aff"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_formats_fixed_abi_fields_from_self_description_layout() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.count",
      "to": "sink.count",
      "message_type": "Count"
    }]
  }],
  "message_abi": [{
    "type_name": "Count",
    "size_bytes": 4,
    "align_bytes": 4,
    "fields": [{
      "name": "value",
      "type": "u32",
      "offset_bytes": 0,
      "size_bytes": 4,
      "align_bytes": 4
    }]
  }]
}
"#;
    let root = temp_test_dir("echo-format-fields");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 88,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_channel_publish_bytes(
        "source.count_to_sink.count",
        "Count",
        vec![0x01, 0x02, 0x03, 0x04],
        Some(123),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = echo_channel_from_image(&selfdesc, "source.count", Some(&socket)).unwrap();

    assert!(output.contains("fields={value=67305985}"));
    assert!(output.contains("raw=01020304"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_formats_variable_frame_fields_from_self_description_layout() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.packet",
      "to": "sink.packet",
      "message_type": "Packet"
    }]
  }],
  "message_abi": [],
  "message_frames": [{
    "type_name": "Packet",
    "encoding": "canonical_frame_v1",
    "header_size_bytes": 17,
    "max_size_bytes": 64,
    "variable": true,
    "fields": [{
      "name": "valid",
      "type": "bool",
      "header_offset_bytes": 0,
      "header_size_bytes": 1,
      "tail_max_bytes": null
    }, {
      "name": "label",
      "type": "string<max=8>",
      "header_offset_bytes": 1,
      "header_size_bytes": 8,
      "tail_max_bytes": 8
    }, {
      "name": "samples",
      "type": "sequence<u32,max=2>",
      "header_offset_bytes": 9,
      "header_size_bytes": 8,
      "tail_max_bytes": 8
    }]
  }]
}
"#;
    let root = temp_test_dir("echo-format-frame");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 89,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let mut payload = Vec::new();
    payload.push(1);
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&2u32.to_le_bytes());
    payload.extend_from_slice(&2u32.to_le_bytes());
    payload.extend_from_slice(&8u32.to_le_bytes());
    payload.extend_from_slice(b"ok");
    payload.extend_from_slice(&10u32.to_le_bytes());
    payload.extend_from_slice(&20u32.to_le_bytes());

    let state = flowrt::IntrospectionState::new();
    state.record_channel_publish_bytes(
        "source.packet_to_sink.packet",
        "Packet",
        payload,
        Some(123),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = echo_channel_from_image(&selfdesc, "source.packet", Some(&socket)).unwrap();

    assert!(output.contains("fields={valid=true,label=\"ok\",samples=[10,20]}"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_online_loads_self_description_and_enables_probe() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.count",
      "to": "sink.count",
      "message_type": "Count"
    }]
  }],
  "message_abi": [{
    "type_name": "Count",
    "size_bytes": 4,
    "align_bytes": 4,
    "fields": [{
      "name": "value",
      "type": "u32",
      "offset_bytes": 0,
      "size_bytes": 4,
      "align_bytes": 4
    }]
  }]
}
"#;
    let root = temp_test_dir("echo-online-selfdesc");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 90,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.set_self_description_json(source);
    state.register_channel("source.count_to_sink.count", "Count");
    assert!(
        !state
            .try_probe_channel_publish_bytes(
                "source.count_to_sink.count",
                "Count",
                &[0, 0, 0, 0],
                Some(100)
            )
            .recorded
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state.clone())
        .expect("status server should start");

    let publisher = std::thread::spawn({
        let state = state.clone();
        move || {
            for _ in 0..100 {
                if state.active_probe_count("source.count_to_sink.count") == Some(1) {
                    let record = state.try_probe_channel_publish_bytes(
                        "source.count_to_sink.count",
                        "Count",
                        &[0x2a, 0x00, 0x00, 0x00],
                        Some(124),
                    );
                    assert!(record.recorded);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("echo did not enable channel probe");
        }
    });

    let output = echo_channel(
        &EchoTarget {
            image: None,
            channel: "source.count".to_string(),
        },
        Some(&socket),
    )
    .unwrap();
    publisher.join().unwrap();

    assert!(output.contains("fields={value=42}"));
    assert!(output.contains("published_at_ms=124"));
    assert!(output.contains("raw=2a000000"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_follow_outputs_changed_snapshots_from_fake_status_server() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
    let root = temp_test_dir("echo-follow");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 86,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_channel_publish_bytes(
        "source.imu_to_sink.imu",
        "Imu",
        vec![0x01, 0x02, 0x03, 0x04],
        Some(10),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state.clone())
        .expect("status server should start");
    let mut output = Vec::new();

    echo_channel_follow_for_polls(
        &EchoTarget {
            image: Some(selfdesc.clone()),
            channel: "source.imu".to_string(),
        },
        Some(&socket),
        std::time::Duration::from_millis(0),
        1,
        &mut output,
    )
    .unwrap();
    state.record_channel_publish_bytes(
        "source.imu_to_sink.imu",
        "Imu",
        vec![0x05, 0x06, 0x07, 0x08],
        Some(11),
    );
    echo_channel_follow_for_polls(
        &EchoTarget {
            image: Some(selfdesc.clone()),
            channel: "source.imu".to_string(),
        },
        Some(&socket),
        std::time::Duration::from_millis(0),
        2,
        &mut output,
    )
    .unwrap();

    let output = String::from_utf8(output).unwrap();
    let lines = output.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("published_count=1"));
    assert!(lines[0].contains("published_at_ms=10"));
    assert!(lines[0].contains("raw=01020304"));
    assert!(lines[1].contains("published_count=2"));
    assert!(lines[1].contains("published_at_ms=11"));
    assert!(lines[1].contains("raw=05060708"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_auto_socket_requires_explicit_socket_for_multiple_matches() {
    let root = temp_test_dir("echo-multiple-sockets");
    let first_socket = root.join("first.sock");
    let second_socket = root.join("second.sock");
    std::fs::create_dir_all(&root).unwrap();

    let self_description_hash = "feedface".to_string();
    let state = flowrt::IntrospectionState::new();
    let first = flowrt::spawn_status_server_at(
        first_socket.clone(),
        flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 91,
            started_at_unix_ms: 1,
            self_description_hash: self_description_hash.clone(),
            package: "robot_demo".to_string(),
            process: "first".to_string(),
            runtime: "rust".to_string(),
        },
        state.clone(),
    )
    .expect("first status server should start");
    let second = flowrt::spawn_status_server_at(
        second_socket.clone(),
        flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 92,
            started_at_unix_ms: 2,
            self_description_hash: self_description_hash.clone(),
            package: "robot_demo".to_string(),
            process: "second".to_string(),
            runtime: "rust".to_string(),
        },
        state,
    )
    .expect("second status server should start");

    let error = select_matching_runtime_socket(
        &self_description_hash,
        vec![first_socket.clone(), second_socket.clone()],
    )
    .expect_err("multiple matching sockets should require explicit selection");

    let message = error.to_string();
    assert!(message.contains("multiple live FlowRT processes match self-description hash"));
    assert!(message.contains("--socket"));
    assert!(message.contains(&first_socket.display().to_string()));
    assert!(message.contains(&second_socket.display().to_string()));

    drop(first);
    drop(second);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn params_commands_use_selfdesc_matched_runtime_socket() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "param_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [{
      "name": "controller",
      "component": "controller",
      "process": "main",
      "runtime": "rust",
      "params": [{
        "name": "kp",
        "type": "f32",
        "update": "on_tick"
      }]
    }],
    "tasks": [],
    "channels": []
  }],
  "message_abi": []
}
"#;
    let root = temp_test_dir("params-cli");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 87,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "param_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.register_param(flowrt::IntrospectionParamSchema {
        name: "controller.kp".to_string(),
        ty: "f32".to_string(),
        update: "on_tick".to_string(),
        current: serde_json::json!(1.0),
        min: Some(serde_json::json!(0.0)),
        max: Some(serde_json::json!(10.0)),
        choices: Vec::new(),
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let list = params_list(&selfdesc, Some(&socket)).unwrap();
    assert!(list.contains("controller.kp type=f32 update=on_tick current=1.0"));

    let get = params_get(&selfdesc, "controller.kp", Some(&socket)).unwrap();
    assert!(get.contains("pending=none"));

    let set = params_set(&selfdesc, "controller.kp", "2.5", Some(&socket)).unwrap();
    assert!(set.contains("current=1.0"));
    assert!(set.contains("pending=2.5"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_endpoint_alias_reports_ambiguous_channels() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "left_sink.imu",
      "message_type": "Imu"
    }, {
      "from": "source.imu",
      "to": "right_sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
    let self_description: SelfDescription = serde_json::from_str(source).unwrap();

    let error = find_echo_channel(&self_description, "source.imu").unwrap_err();

    assert!(
        error
            .to_string()
            .contains("contains multiple channels named `source.imu`")
    );
}

#[test]
fn echo_reports_no_payload_when_snapshot_is_empty() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
    let root = temp_test_dir("echo-no-payload");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 82,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.register_channel("source.imu_to_sink.imu", "Imu");
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output =
        echo_channel_snapshot_from_image(&selfdesc, "source.imu_to_sink.imu", Some(&socket))
            .unwrap();

    assert!(output.contains("payload_len=0"));
    assert!(output.contains("no payload"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_rejects_payload_length_that_does_not_match_message_abi() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
    let root = temp_test_dir("echo-bad-payload-len");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 83,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_channel_publish_bytes("source.imu_to_sink.imu", "Imu", vec![0x01, 0x02], None);
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let error =
        echo_channel_snapshot_from_image(&selfdesc, "source.imu", Some(&socket)).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("payload length 2"));
    assert!(message.contains("Message ABI size 4"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_checks_explicit_socket_hash_before_snapshot_request() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
    let root = temp_test_dir("echo-wrong-socket-hash");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 84,
        started_at_unix_ms: 1234,
        self_description_hash: "different_hash".to_string(),
        package: "other_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let error =
        echo_channel_snapshot_from_image(&selfdesc, "source.imu", Some(&socket)).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("self-description hash `different_hash` does not match"));
    assert!(!message.contains("failed to request channel snapshot"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_reports_structured_live_channel_errors() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
    let root = temp_test_dir("echo-live-channel-error");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 85,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let error =
        echo_channel_snapshot_from_image(&selfdesc, "source.imu", Some(&socket)).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("failed to read channel snapshot `source.imu_to_sink.imu`"));
    assert!(message.contains("unknown FlowRT channel"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn workspace_lock_rejects_concurrent_access_to_same_out_dir() {
    let root = temp_test_dir("workspace-lock");
    let out_dir = root.join("flowrt");

    let first = WorkspaceLock::acquire(&out_dir).expect("first lock should be acquired");
    let error =
        WorkspaceLock::acquire(&out_dir).expect_err("second lock for same out dir should fail");

    assert!(error.to_string().contains("already in use"));
    drop(first);
    WorkspaceLock::acquire(&out_dir).expect("lock should be released on drop");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn workspace_lock_reclaims_lock_owned_by_dead_pid() {
    let root = temp_test_dir("workspace-lock-stale");
    let out_dir = root.join("flowrt");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(out_dir.join(".flowrt.lock"), "pid=99999999\nold=metadata\n").unwrap();

    let lock =
        WorkspaceLock::acquire(&out_dir).expect("unlocked stale lock file should be reclaimed");

    let contents = std::fs::read_to_string(out_dir.join(".flowrt.lock")).unwrap();
    assert_eq!(contents, format!("pid={}\n", std::process::id()));
    drop(lock);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cargo_manifest_patch_is_skipped_when_flowrt_dependency_is_absent() {
    let root = temp_test_dir("cargo-patch-skip");
    let build_dir = root.join("flowrt").join("build");
    std::fs::create_dir_all(&build_dir).unwrap();
    let manifest = build_dir.join("Cargo.toml");
    std::fs::write(
        &manifest,
        r#"[package]
name = "supervisor-only"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .unwrap();

    let patched_manifest =
        cargo_manifest_with_runtime_patch(&root.join("flowrt"), Some(Path::new("/tmp/unused")))
            .expect("manifest without flowrt dependency should still be accepted");
    let content = std::fs::read_to_string(&patched_manifest).unwrap();

    assert!(!content.contains("[patch.crates-io]"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cargo_manifest_patch_uses_available_rust_runtime_dir() {
    let root = temp_test_dir("cargo-patch-runtime");
    let build_dir = root.join("flowrt").join("build");
    let runtime_dir = root.join("installed").join("runtime").join("rust");
    std::fs::create_dir_all(&build_dir).unwrap();
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let manifest = build_dir.join("Cargo.toml");
    std::fs::write(
        &manifest,
        r#"[package]
name = "robot-flowrt-app"
version = "0.1.0"
edition = "2024"

[dependencies]
flowrt = { version = "0.1" }
"#,
    )
    .unwrap();

    let patched_manifest =
        cargo_manifest_with_runtime_patch(&root.join("flowrt"), Some(&runtime_dir))
            .expect("manifest with flowrt dependency should be patched to available runtime");
    let content = std::fs::read_to_string(&patched_manifest).unwrap();

    assert!(content.contains("[patch.crates-io]"));
    assert!(content.contains(&format!(
        "flowrt = {{ path = {} }}",
        toml_basic_string(&runtime_dir)
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cargo_manifest_patch_is_skipped_when_no_runtime_dir_is_available() {
    let root = temp_test_dir("cargo-patch-no-runtime");
    let build_dir = root.join("flowrt").join("build");
    std::fs::create_dir_all(&build_dir).unwrap();
    let manifest = build_dir.join("Cargo.toml");
    std::fs::write(
        &manifest,
        r#"[package]
name = "robot-flowrt-app"
version = "0.1.0"
edition = "2024"

[dependencies]
flowrt = { version = "0.1" }
"#,
    )
    .unwrap();

    let patched_manifest = cargo_manifest_with_runtime_patch(&root.join("flowrt"), None)
        .expect("manifest should remain usable for registry-resolved flowrt");
    let content = std::fs::read_to_string(&patched_manifest).unwrap();

    assert!(!content.contains("[patch.crates-io]"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cargo_build_invocation_uses_manifest_dir_and_offline_config() {
    let root = temp_test_dir("cargo-build-offline");
    let build_dir = root.join("flowrt").join("build");
    std::fs::create_dir_all(build_dir.join(".cargo")).unwrap();
    std::fs::write(
        build_dir.join(".cargo").join("config.toml"),
        "[net]\noffline = true\n",
    )
    .unwrap();
    let manifest = build_dir.join("Cargo.toml");
    std::fs::write(
        &manifest,
        "[package]\nname = \"robot\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let invocation = cargo_build_invocation(&manifest, "robot-flowrt-app")
        .expect("cargo invocation should be derived from manifest");

    assert_eq!(invocation.current_dir, build_dir);
    assert!(invocation.args.iter().any(|arg| arg == "--offline"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cmake_configure_args_do_not_inject_runtime_dir_by_default() {
    let source_dir = Path::new("/tmp/flowrt/build");
    let build_dir = Path::new("/tmp/flowrt/build/cmake");

    let args = cmake_configure_args(source_dir, build_dir, None);

    assert_eq!(
        args,
        vec![
            "-S".to_string(),
            "/tmp/flowrt/build".to_string(),
            "-B".to_string(),
            "/tmp/flowrt/build/cmake".to_string()
        ]
    );
}

#[test]
fn cmake_configure_args_can_pass_explicit_runtime_dir() {
    let source_dir = Path::new("/tmp/flowrt/build");
    let build_dir = Path::new("/tmp/flowrt/build/cmake");
    let runtime_dir = Path::new("/opt/flowrt/runtime/cpp");

    let args = cmake_configure_args(source_dir, build_dir, Some(runtime_dir));

    assert!(args.contains(&"-DFLOWRT_CPP_RUNTIME_DIR=/opt/flowrt/runtime/cpp".to_string()));
    assert!(args.contains(&"-DCMAKE_PREFIX_PATH=/opt/flowrt/runtime/cpp".to_string()));
}

#[test]
fn installed_runtime_candidates_include_private_prefix_layout() {
    let current_exe = Path::new("/opt/flowrt/0.1.0/bin/flowrt");

    let candidates = installed_runtime_candidates(current_exe, "runtime/cpp");

    assert!(
        candidates
            .iter()
            .any(|path| path == Path::new("/opt/flowrt/0.1.0"))
    );
}

#[test]
fn prepare_workspace_projects_selected_profile_before_validation() {
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
backends = ["iox2"]
"#;
    let rsdl_dir = temp_test_dir("prepare-profile");
    let rsdl_path = rsdl_dir.join("robot.rsdl");
    std::fs::create_dir_all(&rsdl_dir).unwrap();
    std::fs::write(&rsdl_path, source).unwrap();
    let out_dir = rsdl_dir.join("flowrt");

    assert!(load_contract_from_rsdl(&rsdl_path).is_err());
    let prepared = prepare_workspace(&rsdl_path, &out_dir, Some("iox2"))
        .expect("selected profile should prepare");
    let prepared_ir =
        ContractIr::from_json_str(&std::fs::read_to_string(&prepared.contract_path).unwrap())
            .unwrap();

    assert_eq!(prepared_ir.profiles.len(), 1);
    assert_eq!(prepared_ir.profiles[0].name, "iox2");
    assert_eq!(prepared_ir.deployments.len(), 1);
    assert_eq!(prepared_ir.deployments[0].profile.name, "iox2");

    let _ = std::fs::remove_dir_all(&rsdl_dir);
}

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
fn prepare_workspace_writes_projected_channel_policy_to_managed_artifacts() {
    let source = r#"
[package]
name = "profile_policy_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["defaulted:Sample", "explicit:Sample"]

[component.consumer]
language = "rust"
input = ["defaulted:Sample", "explicit:Sample"]

[instance.producer]
component = "producer"
process = "main"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 1
output = ["defaulted", "explicit"]

[instance.consumer]
component = "consumer"
process = "main"
target = "linux"

[instance.consumer.task]
trigger = "on_message"
input = ["defaulted", "explicit"]

[[bind.dataflow]]
from = "producer.defaulted"
to = "consumer.defaulted"
channel = "fifo"
depth = 2

[[bind.dataflow]]
from = "producer.explicit"
to = "consumer.explicit"
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
    let explicit_ir = prepared_ir.graphs[0]
        .binds
        .iter()
        .find(|bind| bind.to.port == "explicit")
        .unwrap();

    assert_eq!(defaulted_ir.overflow, flowrt_ir::OverflowPolicy::Error);
    assert_eq!(defaulted_ir.stale, flowrt_ir::StalePolicy::Drop);
    assert_eq!(defaulted_ir.max_age_ms, Some(25));
    assert_eq!(explicit_ir.overflow, flowrt_ir::OverflowPolicy::DropNewest);
    assert_eq!(explicit_ir.stale, flowrt_ir::StalePolicy::HoldLast);
    assert_eq!(explicit_ir.max_age_ms, Some(7));

    let launch: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out_dir.join("launch/launch.json")).unwrap())
            .unwrap();
    let channels = launch["graphs"][0]["channels"].as_array().unwrap();
    let defaulted_launch = channels
        .iter()
        .find(|channel| channel["to"] == "consumer.defaulted")
        .unwrap();
    let explicit_launch = channels
        .iter()
        .find(|channel| channel["to"] == "consumer.explicit")
        .unwrap();

    assert_eq!(defaulted_launch["overflow"], "error");
    assert_eq!(defaulted_launch["stale_policy"], "drop");
    assert_eq!(defaulted_launch["max_age_ms"], 25);
    assert_eq!(explicit_launch["overflow"], "drop_newest");
    assert_eq!(explicit_launch["stale_policy"], "hold_last");
    assert_eq!(explicit_launch["max_age_ms"], 7);

    let _ = std::fs::remove_dir_all(&rsdl_dir);
}
