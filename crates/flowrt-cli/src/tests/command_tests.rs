use super::*;

#[test]
fn cli_exposes_installed_binary_metadata() {
    let command = Cli::command();

    assert_eq!(command.get_name(), "flowrt");
    assert_eq!(command.get_version(), Some(env!("CARGO_PKG_VERSION")));
}

#[test]
fn check_summary_exposes_generated_handler_signature_with_params() {
    let root = temp_test_dir("check-generated-signature");
    std::fs::create_dir_all(&root).unwrap();
    let rsdl = root.join("robot.rsdl");
    std::fs::write(
        &rsdl,
        r#"
[package]
name = "check_signature_demo"
rsdl_version = "0.1"

[type.Cmd]
value = "f32"

[component.controller]
language = "cpp"
output = ["cmd:Cmd"]

[component.controller.params]
kp = { type = "f32", default = 1.0, min = 0.0, max = 10.0, update = "on_tick" }

[instance.controller]
component = "controller"

[instance.controller.task]
trigger = "periodic"
period_ms = 5
output = ["cmd"]
"#,
    )
    .unwrap();

    let contract = load_contract_from_rsdl(&rsdl).unwrap();
    let output = format!(
        "OK {}\n{}",
        summary(&contract),
        handler_signature_summary(&contract)
    );

    assert!(output.contains("generated user API summary:"));
    assert!(output.contains("component controller language=cpp"));
    assert!(output.contains(
        "flowrt::Status on_tick(const ControllerParams& params, flowrt::Output<Cmd>& cmd)"
    ));
    assert!(!root.join("flowrt").exists());
}

#[test]
fn command_build_parses_target_platform() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "build",
        "rsdl/robot.rsdl",
        "--target",
        "linux-arm64",
    ])
    .unwrap();

    let Command::Build { target, .. } = cli.command else {
        panic!("build command should parse into Command::Build")
    };

    assert_eq!(target.as_deref(), Some("linux-arm64"));
}

#[test]
fn cli_parses_project_commands_without_explicit_rsdl_path() {
    for command in [
        "check", "explain", "prepare", "build", "run", "deps", "doctor",
    ] {
        let cli = Cli::try_parse_from(["flowrt", command])
            .unwrap_or_else(|error| panic!("{command} should accept omitted RSDL: {error}"));

        match (command, cli.command) {
            ("check", Command::Check { .. })
            | ("explain", Command::Explain { .. })
            | ("prepare", Command::Prepare { .. })
            | ("build", Command::Build { .. })
            | ("run", Command::Run { .. })
            | ("deps", Command::Deps { .. })
            | ("doctor", Command::Doctor { .. }) => {}
            _ => panic!("{command} parsed into unexpected command"),
        }
    }
}

#[test]
fn cli_parses_init_command_with_default_rust_language() {
    let cli = Cli::try_parse_from(["flowrt", "init", "demo_bot"]).unwrap();

    let Command::Init { path, language } = cli.command else {
        panic!("init command should parse into Command::Init")
    };

    assert_eq!(path, PathBuf::from("demo_bot"));
    assert_eq!(language, AppInitLanguage::Rust);
}

#[test]
fn cli_rejects_c_init_language_until_c_component_entry_is_ready() {
    let error = Cli::try_parse_from(["flowrt", "init", "demo_bot", "--lang", "c"])
        .expect_err("flowrt init must not expose C app skeletons yet");

    assert!(error.to_string().contains("invalid value 'c'"));
}

#[test]
fn cli_parses_add_message_module_and_component_commands() {
    let message = Cli::try_parse_from(["flowrt", "add", "message", "Sample", "value:u32"])
        .expect("add message should parse");
    let Command::Add {
        command: AddCommand::Message { name, fields, rsdl },
    } = message.command
    else {
        panic!("add message should parse into Command::Add")
    };
    assert_eq!(name, "Sample");
    assert_eq!(fields, vec!["value:u32"]);
    assert_eq!(rsdl, None);

    let module =
        Cli::try_parse_from(["flowrt", "add", "module", "perception"]).expect("module parses");
    let Command::Add {
        command: AddCommand::Module { name, rsdl },
    } = module.command
    else {
        panic!("add module should parse into Command::Add")
    };
    assert_eq!(name, "perception");
    assert_eq!(rsdl, None);

    let component = Cli::try_parse_from([
        "flowrt",
        "add",
        "component",
        "Source",
        "--lang",
        "rust",
        "--input",
        "scan:Sample",
        "--output",
        "sample:Sample",
    ])
    .expect("add component should parse");
    let Command::Add {
        command:
            AddCommand::Component {
                name,
                language,
                inputs,
                outputs,
                rsdl,
            },
    } = component.command
    else {
        panic!("add component should parse into Command::Add")
    };
    assert_eq!(name, "Source");
    assert_eq!(language, AppAddLanguage::Rust);
    assert_eq!(inputs, vec!["scan:Sample"]);
    assert_eq!(outputs, vec!["sample:Sample"]);
    assert_eq!(rsdl, None);
}

#[test]
fn cli_rejects_c_add_component_language_until_c_adapter_is_ready() {
    let error = Cli::try_parse_from(["flowrt", "add", "component", "Sensor", "--lang", "c"])
        .expect_err("flowrt add component must not expose C skeletons yet");

    assert!(error.to_string().contains("invalid value 'c'"));
}

#[test]
fn cli_rejects_legacy_init_language_flag() {
    let error = Cli::try_parse_from(["flowrt", "init", "demo_bot", "--language", "cpp"])
        .expect_err("flowrt init exposes --lang, not a legacy --language alias");

    assert!(
        error
            .to_string()
            .contains("unexpected argument '--language'")
    );
}

#[test]
fn init_app_project_writes_rust_modern_app_layout() {
    let root = temp_test_dir("init-rust").join("demo-bot");

    let output = init_app_project(&root, AppInitLanguage::Rust).unwrap();

    assert!(output.contains("language=rust"));
    assert_eq!(
        project_manifest::load_manifest_rsdl(&root.join("flowrt.toml")).unwrap(),
        root.join("rsdl/robot.rsdl")
    );
    assert!(!root.join("app/cpp").exists());
    let app = std::fs::read_to_string(root.join("app/rust/mod.rs")).unwrap();
    assert!(app.contains("use crate::messages::Tick;"));
    assert!(app.contains("impl Controller for ControllerImpl"));
    assert!(app.contains("tick: &mut flowrt::Output<Tick>"));
    assert!(app.contains("pub fn build_app() -> crate::App"));

    let contract = load_contract_from_rsdl(&root.join("rsdl/robot.rsdl")).unwrap();
    assert_eq!(contract.package.name, "demo_bot");
    assert_eq!(contract.types.len(), 1);
    assert_eq!(contract.components[0].language, LanguageKind::Rust);
    assert_eq!(contract.targets[0].runtime, vec![LanguageKind::Rust]);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn init_app_project_writes_cpp_modern_app_layout() {
    let root = temp_test_dir("init-cpp").join("demo-bot");

    let output = init_app_project(&root, AppInitLanguage::Cpp).unwrap();

    assert!(output.contains("language=cpp"));
    assert!(root.join("flowrt.toml").is_file());
    assert!(root.join("rsdl/robot.rsdl").is_file());
    assert!(!root.join("app/rust").exists());
    let app = std::fs::read_to_string(root.join("app/cpp/components.cpp")).unwrap();
    assert!(app.contains("class Controller final"));
    assert!(app.contains("flowrt::Output<flowrt_app::Tick>& tick"));
    assert!(app.contains("flowrt_user"));

    let contract = load_contract_from_rsdl(&root.join("rsdl/robot.rsdl")).unwrap();
    assert_eq!(contract.package.name, "demo_bot");
    assert_eq!(contract.types.len(), 1);
    assert_eq!(contract.components[0].language, LanguageKind::Cpp);
    assert_eq!(contract.targets[0].runtime, vec![LanguageKind::Cpp]);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn init_app_project_refuses_to_overwrite_existing_files() {
    let root = temp_test_dir("init-no-overwrite");
    let rsdl = root.join("rsdl/robot.rsdl");
    std::fs::create_dir_all(rsdl.parent().unwrap()).unwrap();
    std::fs::write(&rsdl, "user-owned\n").unwrap();

    let error = init_app_project(&root, AppInitLanguage::Rust).unwrap_err();

    assert!(error.to_string().contains("refusing to overwrite"));
    assert_eq!(std::fs::read_to_string(&rsdl).unwrap(), "user-owned\n");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn add_message_appends_type_and_keeps_contract_checkable() {
    let root = temp_test_dir("add-message").join("demo-bot");
    init_app_project(&root, AppInitLanguage::Rust).unwrap();
    let rsdl = root.join("rsdl/robot.rsdl");

    let output = add_message_to_rsdl(&rsdl, "Sample", &["value:u32".to_string()]).unwrap();

    assert!(output.contains("added message `Sample`"));
    let source = std::fs::read_to_string(&rsdl).unwrap();
    assert!(source.contains("[type.Sample]"));
    assert!(source.contains("value = \"u32\""));
    let contract = load_contract_from_rsdl(&rsdl).unwrap();
    assert!(contract.types.iter().any(|ty| ty.name == "Sample"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn add_message_rejects_duplicate_and_rolls_back_invalid_contract() {
    let root = temp_test_dir("add-message-rollback").join("demo-bot");
    init_app_project(&root, AppInitLanguage::Rust).unwrap();
    let rsdl = root.join("rsdl/robot.rsdl");

    let duplicate = add_message_to_rsdl(&rsdl, "Tick", &["value:u32".to_string()]).unwrap_err();
    assert!(duplicate.to_string().contains("type `Tick` already exists"));

    let before = std::fs::read_to_string(&rsdl).unwrap();
    let invalid =
        add_message_to_rsdl(&rsdl, "Bad", &["value:MissingType".to_string()]).unwrap_err();
    assert!(invalid.to_string().contains("contract validation failed"));
    assert_eq!(std::fs::read_to_string(&rsdl).unwrap(), before);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn add_message_rejects_invalid_field_format() {
    let root = temp_test_dir("add-message-field-format").join("demo-bot");
    init_app_project(&root, AppInitLanguage::Rust).unwrap();
    let rsdl = root.join("rsdl/robot.rsdl");

    let error = add_message_to_rsdl(&rsdl, "Sample", &["value".to_string()]).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("field spec must use `field:type`")
    );
    assert!(
        !std::fs::read_to_string(&rsdl)
            .unwrap()
            .contains("[type.Sample]")
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn add_module_creates_workspace_module_without_overwriting() {
    let root = temp_test_dir("add-module").join("demo-bot");
    init_app_project(&root, AppInitLanguage::Rust).unwrap();
    let rsdl = root.join("rsdl/robot.rsdl");

    let output = add_module_to_rsdl(&rsdl, "perception").unwrap();

    assert!(output.contains("added module `perception`"));
    let module = root.join("rsdl/modules/perception.rsdl");
    assert_eq!(
        std::fs::read_to_string(&module).unwrap(),
        "[module]\nname = \"perception\"\n"
    );
    let source = std::fs::read_to_string(&rsdl).unwrap();
    assert!(source.contains("[workspace]"));
    assert!(source.contains("modules = [\"modules/*.rsdl\"]"));
    let contract = load_contract_from_rsdl(&rsdl).unwrap();
    assert!(
        contract
            .modules
            .iter()
            .any(|module| module.name == "perception")
    );

    let error = add_module_to_rsdl(&rsdl, "perception").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("refusing to overwrite existing file")
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn add_component_rust_appends_contract_and_modern_skeleton() {
    let root = temp_test_dir("add-component-rust").join("demo-bot");
    init_app_project(&root, AppInitLanguage::Rust).unwrap();
    let rsdl = root.join("rsdl/robot.rsdl");
    add_message_to_rsdl(&rsdl, "Sample", &["value:u32".to_string()]).unwrap();

    let output = add_component_to_rsdl(
        &rsdl,
        AddComponentSpec {
            name: "Source".to_string(),
            language: AppAddLanguage::Rust,
            inputs: Vec::new(),
            outputs: vec!["sample:Sample".to_string()],
        },
    )
    .unwrap();

    assert!(output.contains("added component `source`"));
    let source = std::fs::read_to_string(&rsdl).unwrap();
    assert!(source.contains("[component.source]"));
    assert!(source.contains("language = \"rust\""));
    assert!(source.contains("output = [\"sample:Sample\"]"));
    assert!(source.contains("[instance.source]"));
    assert!(source.contains("[instance.source.task]"));
    let app = std::fs::read_to_string(root.join("app/rust/mod.rs")).unwrap();
    assert!(app.contains("use crate::components::Source;"));
    assert!(app.contains("use crate::messages::Sample;"));
    assert!(app.contains("pub struct SourceImpl;"));
    assert!(app.contains("impl Source for SourceImpl"));
    assert!(app.contains("sample.write(Sample::default());"));
    assert!(app.contains("Box::new(SourceImpl::default())"));
    let contract = load_contract_from_rsdl(&rsdl).unwrap();
    assert!(
        contract
            .components
            .iter()
            .any(|component| component.name == "source")
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn add_component_cpp_appends_contract_and_modern_skeleton() {
    let root = temp_test_dir("add-component-cpp").join("demo-bot");
    init_app_project(&root, AppInitLanguage::Cpp).unwrap();
    let rsdl = root.join("rsdl/robot.rsdl");
    add_message_to_rsdl(&rsdl, "Sample", &["value:u32".to_string()]).unwrap();

    add_component_to_rsdl(
        &rsdl,
        AddComponentSpec {
            name: "Source".to_string(),
            language: AppAddLanguage::Cpp,
            inputs: Vec::new(),
            outputs: vec!["sample:Sample".to_string()],
        },
    )
    .unwrap();

    let app = std::fs::read_to_string(root.join("app/cpp/components.cpp")).unwrap();
    assert!(app.contains("class Source final : public flowrt_app::SourceInterface"));
    assert!(app.contains("sample.write(flowrt_app::Sample{});"));
    assert!(app.contains("std::make_unique<Source>()"));
    let contract = load_contract_from_rsdl(&rsdl).unwrap();
    assert!(
        contract
            .components
            .iter()
            .any(|component| component.name == "source")
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn add_component_rejects_existing_component_without_touching_user_file() {
    let root = temp_test_dir("add-component-existing").join("demo-bot");
    init_app_project(&root, AppInitLanguage::Rust).unwrap();
    let rsdl = root.join("rsdl/robot.rsdl");
    let app_path = root.join("app/rust/mod.rs");
    let before = std::fs::read_to_string(&app_path).unwrap();

    let error = add_component_to_rsdl(
        &rsdl,
        AddComponentSpec {
            name: "controller".to_string(),
            language: AppAddLanguage::Rust,
            inputs: Vec::new(),
            outputs: Vec::new(),
        },
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("component `controller` already exists")
    );
    assert_eq!(std::fs::read_to_string(app_path).unwrap(), before);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn add_component_rejects_custom_user_file_without_touching_rsdl() {
    let root = temp_test_dir("add-component-custom-user-file").join("demo-bot");
    init_app_project(&root, AppInitLanguage::Rust).unwrap();
    let rsdl = root.join("rsdl/robot.rsdl");
    add_message_to_rsdl(&rsdl, "Sample", &["value:u32".to_string()]).unwrap();
    let app_path = root.join("app/rust/mod.rs");
    std::fs::write(&app_path, "pub fn custom_user_code() {}\n").unwrap();
    let before_rsdl = std::fs::read_to_string(&rsdl).unwrap();

    let error = add_component_to_rsdl(
        &rsdl,
        AddComponentSpec {
            name: "Source".to_string(),
            language: AppAddLanguage::Rust,
            inputs: Vec::new(),
            outputs: vec!["sample:Sample".to_string()],
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("no `crate::App::new(...)` call"));
    assert_eq!(std::fs::read_to_string(&rsdl).unwrap(), before_rsdl);
    assert_eq!(
        std::fs::read_to_string(app_path).unwrap(),
        "pub fn custom_user_code() {}\n"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cli_parses_explain_format() {
    let cli =
        Cli::try_parse_from(["flowrt", "explain", "rsdl/robot.rsdl", "--format", "json"]).unwrap();

    let Command::Explain { rsdl, format } = cli.command else {
        panic!("explain command should parse into Command::Explain")
    };

    assert_eq!(rsdl, Some(PathBuf::from("rsdl/robot.rsdl")));
    assert_eq!(format, ExplainFormat::Json);
}

#[test]
fn cli_parses_temporary_island_overlay_flags() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "prepare",
        "rsdl/robot.rsdl",
        "--temporary-island",
        "--boundary-input",
        "scan_in=planner.scan",
        "--boundary-output",
        "cmd_out=planner.cmd",
    ])
    .unwrap();

    let Command::Prepare {
        temporary_island,
        boundary_input,
        boundary_output,
        ..
    } = cli.command
    else {
        panic!("prepare command should parse into Command::Prepare")
    };

    assert!(temporary_island);
    assert_eq!(boundary_input, vec!["scan_in=planner.scan"]);
    assert_eq!(boundary_output, vec!["cmd_out=planner.cmd"]);
}

#[test]
fn command_deps_parses_target_platform() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "deps",
        "rsdl/robot.rsdl",
        "--target",
        "linux-arm64",
        "--backend",
        "zenoh",
    ])
    .unwrap();

    let Command::Deps {
        target, backend, ..
    } = cli.command
    else {
        panic!("deps command should parse into Command::Deps")
    };

    assert_eq!(target.as_deref(), Some("linux-arm64"));
    assert_eq!(backend, Some(DepsBackend::Zenoh));
}

#[test]
fn command_doctor_parses_target_platform() {
    let cli = Cli::try_parse_from(["flowrt", "doctor", "--target", "linux-arm64"]).unwrap();

    let Command::Doctor { rsdl, target } = cli.command else {
        panic!("doctor command should parse into Command::Doctor")
    };

    assert_eq!(rsdl, None);
    assert_eq!(target.as_deref(), Some("linux-arm64"));
}

#[test]
fn command_doctor_parses_rsdl_path_and_target_platform() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "doctor",
        "examples/libjpeg_cross_demo/rsdl/robot.rsdl",
        "--target",
        "linux-arm64",
    ])
    .unwrap();

    let Command::Doctor { rsdl, target } = cli.command else {
        panic!("doctor command should parse into Command::Doctor")
    };

    assert_eq!(
        rsdl,
        Some(PathBuf::from("examples/libjpeg_cross_demo/rsdl/robot.rsdl"))
    );
    assert_eq!(target.as_deref(), Some("linux-arm64"));
}

#[test]
fn command_cache_parses_status_subcommand() {
    let cli = Cli::try_parse_from(["flowrt", "cache", "status"]).unwrap();

    let Command::Cache { command } = cli.command else {
        panic!("cache command should parse into Command::Cache")
    };
    let CacheCommand::Status = command else {
        panic!("cache status should parse into CacheCommand::Status")
    };
}

#[test]
fn command_cache_parses_clean_filters_and_scopes() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "cache",
        "clean",
        "--target",
        "linux-arm64",
        "--build-mode",
        "debug",
        "--dry-run",
        "--flowrt-deps",
        "--project-build",
        "--incremental",
        "--stale-temp",
    ])
    .unwrap();

    let Command::Cache { command } = cli.command else {
        panic!("cache command should parse into Command::Cache")
    };
    let CacheCommand::Clean {
        target,
        build_mode,
        dry_run,
        flowrt_deps,
        project_build,
        incremental,
        stale_temp,
    } = command
    else {
        panic!("cache clean should parse into CacheCommand::Clean")
    };

    assert_eq!(target.as_deref(), Some("linux-arm64"));
    assert_eq!(build_mode, Some(BuildMode::Debug));
    assert!(dry_run);
    assert!(flowrt_deps);
    assert!(project_build);
    assert!(incremental);
    assert!(stale_temp);
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
        raw,
        interval_ms,
    } = cli.command
    else {
        panic!("echo command should parse into Command::Echo")
    };

    assert_eq!(target, "flowrt/selfdesc/selfdesc.json");
    assert_eq!(image, None);
    assert_eq!(channel, vec!["source.imu_to_sink.imu".to_string()]);
    assert_eq!(socket, Some(PathBuf::from("/tmp/flowrt-main.sock")));
    assert!(!follow);
    assert!(!raw);
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
    assert!(channel.is_empty());
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
    assert!(channel.is_empty());
}

#[test]
fn cli_parses_echo_multiple_channels_with_image_option() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "echo",
        "result_a",
        "result_b",
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

    assert_eq!(target, "result_a");
    assert_eq!(image, Some(PathBuf::from("flowrt/selfdesc/selfdesc.json")));
    assert_eq!(channel, vec!["result_b".to_string()]);
}

#[test]
fn cli_parses_pub_boundary_input_command() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "pub",
        "sample_in",
        "--json",
        r#"{"value":7}"#,
        "--image",
        "flowrt/selfdesc/selfdesc.json",
        "--socket",
        "/tmp/flowrt-main.sock",
    ])
    .unwrap();

    let Command::Pub {
        endpoint,
        json,
        file,
        freq,
        image,
        socket,
        published_at_ms,
    } = cli.command
    else {
        panic!("pub command should parse into Command::Pub")
    };

    assert_eq!(endpoint, "sample_in");
    assert_eq!(json.as_deref(), Some(r#"{"value":7}"#));
    assert_eq!(file, None);
    assert_eq!(freq, None);
    assert_eq!(image, Some(PathBuf::from("flowrt/selfdesc/selfdesc.json")));
    assert_eq!(socket, Some(PathBuf::from("/tmp/flowrt-main.sock")));
    assert_eq!(published_at_ms, None);
}

#[test]
fn cli_parses_pub_boundary_file_input() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "pub",
        "sample_in",
        "--file",
        "input.jsonl",
        "--freq",
        "200",
        "--image",
        "flowrt/selfdesc/selfdesc.json",
    ])
    .unwrap();

    let Command::Pub {
        endpoint,
        json,
        file,
        freq,
        image,
        ..
    } = cli.command
    else {
        panic!("pub command should parse into Command::Pub")
    };

    assert_eq!(endpoint, "sample_in");
    assert_eq!(json, None);
    assert_eq!(file, Some(PathBuf::from("input.jsonl")));
    assert_eq!(freq, Some(200.0));
    assert_eq!(image, Some(PathBuf::from("flowrt/selfdesc/selfdesc.json")));
}

#[test]
fn cli_rejects_pub_without_json_or_file() {
    let error = Cli::try_parse_from(["flowrt", "pub", "sample_in"])
        .expect_err("pub requires json or file input");

    assert_eq!(
        error.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
}

#[test]
fn cli_rejects_pub_mixing_json_and_file() {
    let error = Cli::try_parse_from([
        "flowrt",
        "pub",
        "sample_in",
        "--json",
        r#"{"value":7}"#,
        "--file",
        "input.jsonl",
    ])
    .expect_err("pub json and file inputs are mutually exclusive");

    assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
}

#[test]
fn cli_rejects_pub_non_positive_freq() {
    let error = Cli::try_parse_from([
        "flowrt",
        "pub",
        "sample_in",
        "--file",
        "input.jsonl",
        "--freq",
        "0",
    ])
    .expect_err("pub freq must be positive");

    assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
}

#[test]
fn cli_parses_island_bundle_and_deploy_escape_hatches() {
    let bundle_cli = Cli::try_parse_from([
        "flowrt",
        "bundle",
        "rsdl/robot.rsdl",
        "--output",
        "dist/island",
        "--allow-island",
    ])
    .unwrap();
    let Command::Bundle { allow_island, .. } = bundle_cli.command else {
        panic!("bundle command should parse into Command::Bundle")
    };
    assert!(allow_island);

    let deploy_cli = Cli::try_parse_from([
        "flowrt",
        "deploy",
        "dist/island",
        "--host",
        "robot@192.0.2.10",
        "--target",
        "pi",
        "--remote-dir",
        "/tmp/flowrt-demo",
        "--allow-island",
    ])
    .unwrap();
    let Command::Deploy { allow_island, .. } = deploy_cli.command else {
        panic!("deploy command should parse into Command::Deploy")
    };
    assert!(allow_island);
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
fn cli_parses_echo_raw_option() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "echo",
        "flowrt/selfdesc/selfdesc.json",
        "source.scan",
        "--raw",
    ])
    .unwrap();

    let Command::Echo { raw, .. } = cli.command else {
        panic!("echo --raw should parse into Command::Echo")
    };

    assert!(raw);
}

#[test]
fn cli_parses_replay_fixture_command() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "replay",
        "--file",
        "fixtures/scan.jsonl",
        "--image",
        "flowrt/selfdesc/selfdesc.json",
        "--speed",
        "2.0",
        "--socket",
        "/tmp/flowrt-main.sock",
    ])
    .unwrap();

    let Command::Replay {
        file,
        image,
        socket,
        speed,
        as_fast_as_possible,
    } = cli.command
    else {
        panic!("replay command should parse into Command::Replay")
    };

    assert_eq!(file, PathBuf::from("fixtures/scan.jsonl"));
    assert_eq!(image, PathBuf::from("flowrt/selfdesc/selfdesc.json"));
    assert_eq!(socket, Some(PathBuf::from("/tmp/flowrt-main.sock")));
    assert_eq!(speed, 2.0);
    assert!(!as_fast_as_possible);
}

#[test]
fn cli_parses_params_set_command() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "params",
        "set",
        "controller.kp",
        "2.5",
        "--image",
        "flowrt/selfdesc/selfdesc.json",
        "--socket",
        "/tmp/flowrt-main.sock",
    ])
    .unwrap();

    let Command::Params {
        command:
            ParamsCommand::Set {
                name,
                value,
                image,
                socket,
                ..
            },
    } = cli.command
    else {
        panic!("params set command should parse into Command::Params")
    };

    assert_eq!(image, Some(PathBuf::from("flowrt/selfdesc/selfdesc.json")));
    assert_eq!(name.as_deref(), Some("controller.kp"));
    assert_eq!(value.as_deref(), Some("2.5"));
    assert_eq!(socket, Some(PathBuf::from("/tmp/flowrt-main.sock")));
}

#[test]
fn cli_parses_params_set_file_command() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "params",
        "set",
        "--file",
        "params.json",
        "--image",
        "flowrt/selfdesc/selfdesc.json",
        "--socket",
        "/tmp/flowrt-main.sock",
    ])
    .unwrap();

    let Command::Params {
        command:
            ParamsCommand::Set {
                name,
                value,
                file,
                image,
                socket,
                ..
            },
    } = cli.command
    else {
        panic!("params set --file should parse into Command::Params")
    };

    assert_eq!(name, None);
    assert_eq!(value, None);
    assert_eq!(file, Some(PathBuf::from("params.json")));
    assert_eq!(image, Some(PathBuf::from("flowrt/selfdesc/selfdesc.json")));
    assert_eq!(socket, Some(PathBuf::from("/tmp/flowrt-main.sock")));
}

#[test]
fn cli_rejects_params_set_file_mixed_with_single_value() {
    let error = Cli::try_parse_from([
        "flowrt",
        "params",
        "set",
        "controller.kp",
        "2.5",
        "--file",
        "params.json",
    ])
    .expect_err("params set should reject mixing --file with positional name/value");

    assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
}

#[test]
fn cli_rejects_params_set_without_single_value_or_file() {
    let error =
        Cli::try_parse_from(["flowrt", "params", "set"]).expect_err("params set requires input");

    assert_eq!(
        error.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
}

#[test]
fn cli_parses_external_check_and_list_commands() {
    let check = Cli::try_parse_from(["flowrt", "external", "check", "external/fake_sensor_driver"])
        .unwrap();
    let Command::External {
        command: ExternalCommand::Check { package_dir },
    } = check.command
    else {
        panic!("external check should parse into Command::External")
    };
    assert_eq!(package_dir, PathBuf::from("external/fake_sensor_driver"));

    let list = Cli::try_parse_from(["flowrt", "external", "list", "--path", "external"]).unwrap();
    let Command::External {
        command: ExternalCommand::List { path },
    } = list.command
    else {
        panic!("external list should parse into Command::External")
    };
    assert_eq!(path, PathBuf::from("external"));
}

#[test]
fn cli_parses_remote_params_runtime_selector() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "params",
        "get",
        "controller.kp",
        "--image",
        "flowrt/selfdesc/selfdesc.json",
        "--remote",
        "--runtime",
        "flowrt/params/robot/hash1/42",
    ])
    .unwrap();

    let Command::Params {
        command:
            ParamsCommand::Get {
                image,
                remote,
                runtime,
                socket,
                ..
            },
    } = cli.command
    else {
        panic!("params get command should parse into Command::Params")
    };

    assert_eq!(image, Some(PathBuf::from("flowrt/selfdesc/selfdesc.json")));
    assert!(remote);
    assert_eq!(runtime.as_deref(), Some("flowrt/params/robot/hash1/42"));
    assert_eq!(socket, None);
}

#[test]
fn cli_parses_operation_commands() {
    let list_cli = Cli::try_parse_from([
        "flowrt",
        "op",
        "list",
        "--image",
        "flowrt/selfdesc/selfdesc.json",
    ])
    .unwrap();
    let Command::Op {
        command: OpCommand::List { image, socket },
    } = list_cli.command
    else {
        panic!("op list should parse into Command::Op")
    };
    assert_eq!(image, Some(PathBuf::from("flowrt/selfdesc/selfdesc.json")));
    assert_eq!(socket, None);

    let status_cli = Cli::try_parse_from([
        "flowrt",
        "op",
        "status",
        "controller.plan",
        "--socket",
        "/tmp/flowrt-main.sock",
    ])
    .unwrap();
    let Command::Op {
        command: OpCommand::Status { name, socket },
    } = status_cli.command
    else {
        panic!("op status should parse into Command::Op")
    };
    assert_eq!(name.as_deref(), Some("controller.plan"));
    assert_eq!(socket, Some(PathBuf::from("/tmp/flowrt-main.sock")));

    let cancel_cli = Cli::try_parse_from([
        "flowrt",
        "op",
        "cancel",
        "111:7:3",
        "--socket",
        "/tmp/flowrt-main.sock",
    ])
    .unwrap();
    let Command::Op {
        command: OpCommand::Cancel {
            operation_id,
            socket,
        },
    } = cancel_cli.command
    else {
        panic!("op cancel should parse into Command::Op")
    };
    assert_eq!(operation_id, "111:7:3");
    assert_eq!(socket, Some(PathBuf::from("/tmp/flowrt-main.sock")));
}

#[test]
fn cli_parses_status_live_only() {
    let cli = Cli::try_parse_from(["flowrt", "status", "--live-only"]).unwrap();

    let Command::Status { live_only } = cli.command else {
        panic!("status command should parse into Command::Status")
    };

    assert!(live_only);
}

#[test]
fn cli_parses_record_command_with_filters() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "record",
        "--output",
        "run.mcap",
        "--socket",
        "/tmp/flowrt-main.sock",
        "--duration",
        "10s",
        "--channel",
        "source.imu_to_sink.imu",
        "--operation",
        "controller.plan",
        "--force",
    ])
    .unwrap();

    let Command::Record {
        output,
        socket,
        duration,
        channel,
        operation,
        all,
        force,
    } = cli.command
    else {
        panic!("record command should parse into Command::Record")
    };

    assert_eq!(output, PathBuf::from("run.mcap"));
    assert_eq!(socket, Some(PathBuf::from("/tmp/flowrt-main.sock")));
    assert_eq!(duration, Some(Duration::from_secs(10)));
    assert_eq!(channel, vec!["source.imu_to_sink.imu"]);
    assert_eq!(operation, vec!["controller.plan"]);
    assert!(!all);
    assert!(force);
}

#[test]
fn cli_rejects_zero_record_duration() {
    let error = Cli::try_parse_from([
        "flowrt",
        "record",
        "--output",
        "run.mcap",
        "--duration",
        "0s",
    ])
    .expect_err("zero record duration should be rejected");

    assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
}

#[test]
fn params_remote_runtime_arg_rejects_socket() {
    let error = params_remote_runtime_arg(true, Some(Path::new("/tmp/flowrt.sock")), None)
        .expect_err("--remote must not accept a local socket selector");

    assert!(error.to_string().contains("cannot be used with `--remote`"));
    assert!(error.to_string().contains("--runtime <key_expr>"));
}

#[test]
fn params_runtime_arg_requires_remote_mode() {
    let error = params_remote_runtime_arg(false, None, Some("flowrt/params/robot/hash1/42"))
        .expect_err("--runtime must require remote mode");

    assert!(
        error
            .to_string()
            .contains("can only be used with `--remote`")
    );
    assert!(error.to_string().contains("--socket <path>"));
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

/// 兼容测试：`--run-ticks` 作为 `--run-steps` 的别名仍可被 CLI 解析。
#[test]
fn cli_parses_run_ticks_compat_alias_for_run_and_launch() {
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

/// 主路径测试：`--run-steps` 是推荐的外部运行上限名称。
#[test]
fn cli_parses_run_steps_as_primary_run_limit() {
    let run_cli = Cli::try_parse_from([
        "flowrt",
        "run",
        "examples/import_demo/rsdl/robot.rsdl",
        "--run-steps",
        "5",
    ])
    .unwrap();
    let Command::Run { run_ticks, .. } = run_cli.command else {
        panic!("run command should parse into Command::Run")
    };
    assert_eq!(run_ticks, Some(5));

    let launch_cli = Cli::try_parse_from([
        "flowrt",
        "launch",
        "examples/import_demo/rsdl/robot.rsdl",
        "--run-steps",
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
fn cli_build_defaults_to_release_mode_and_accepts_debug() {
    let cli =
        Cli::try_parse_from(["flowrt", "build", "examples/import_demo/rsdl/robot.rsdl"]).unwrap();
    let Command::Build { build_mode, .. } = cli.command else {
        panic!("build command should parse into Command::Build")
    };
    assert_eq!(build_mode, BuildMode::Release);

    let cli = Cli::try_parse_from([
        "flowrt",
        "build",
        "examples/import_demo/rsdl/robot.rsdl",
        "--build-mode",
        "debug",
    ])
    .unwrap();
    let Command::Build { build_mode, .. } = cli.command else {
        panic!("build command should parse into Command::Build")
    };
    assert_eq!(build_mode, BuildMode::Debug);
}

#[test]
fn cli_parses_build_target_platform() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "build",
        "examples/cpp_counter_demo/rsdl/robot.rsdl",
        "--target",
        "linux-arm64",
    ])
    .unwrap();

    let Command::Build { target, .. } = cli.command else {
        panic!("build command should parse into Command::Build")
    };
    assert_eq!(target.as_deref(), Some("linux-arm64"));
}

#[test]
fn cli_parses_deps_command_with_backend_and_build_mode() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "deps",
        "examples/import_demo/rsdl/robot.rsdl",
        "--profile",
        "default",
        "--backend",
        "all",
        "--build-mode",
        "debug",
        "--check",
    ])
    .unwrap();

    let Command::Deps {
        rsdl,
        backend,
        profile,
        build_mode,
        check,
        ..
    } = cli.command
    else {
        panic!("deps command should parse into Command::Deps")
    };
    assert_eq!(
        rsdl,
        Some(PathBuf::from("examples/import_demo/rsdl/robot.rsdl"))
    );
    assert_eq!(backend, Some(DepsBackend::All));
    assert_eq!(profile.as_deref(), Some("default"));
    assert_eq!(build_mode, BuildMode::Debug);
    assert!(check);
}

#[test]
fn cli_parses_bundle_command() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "bundle",
        "examples/external_driver_demo/rsdl/robot.rsdl",
        "--out-dir",
        "flowrt",
        "--output",
        "dist/external-demo",
        "--profile",
        "default",
        "--build-mode",
        "release",
    ])
    .unwrap();

    let Command::Bundle {
        rsdl,
        out_dir,
        output,
        profile,
        build_mode,
        allow_island,
    } = cli.command
    else {
        panic!("bundle command should parse into Command::Bundle")
    };
    assert_eq!(
        rsdl,
        PathBuf::from("examples/external_driver_demo/rsdl/robot.rsdl")
    );
    assert_eq!(out_dir, PathBuf::from("flowrt"));
    assert_eq!(output, PathBuf::from("dist/external-demo"));
    assert_eq!(profile.as_deref(), Some("default"));
    assert_eq!(build_mode, Some(BuildMode::Release));
    assert!(!allow_island);
}

#[test]
fn cli_parses_deploy_command() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "deploy",
        "dist/external-demo",
        "--host",
        "robot@192.0.2.10",
        "--target",
        "pi",
        "--remote-dir",
        "/tmp/flowrt-demo",
        "--dry-run",
    ])
    .unwrap();

    let Command::Deploy {
        bundle,
        host,
        target,
        remote_dir,
        dry_run,
        allow_island,
    } = cli.command
    else {
        panic!("deploy command should parse into Command::Deploy")
    };
    assert_eq!(bundle, PathBuf::from("dist/external-demo"));
    assert_eq!(host, "robot@192.0.2.10");
    assert_eq!(target, "pi");
    assert_eq!(remote_dir, "/tmp/flowrt-demo");
    assert!(dry_run);
    assert!(!allow_island);
}

/// 兼容测试：`--run-ticks 0` 仍会被 CLI 拒绝。
#[test]
fn cli_rejects_zero_run_ticks_compat() {
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
