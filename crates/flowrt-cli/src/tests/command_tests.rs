use super::*;

#[test]
fn cli_exposes_installed_binary_metadata() {
    let command = Cli::command();

    assert_eq!(command.get_name(), "flowrt");
    assert_eq!(command.get_version(), Some(env!("CARGO_PKG_VERSION")));
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

    let Command::Doctor { target } = cli.command else {
        panic!("doctor command should parse into Command::Doctor")
    };

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
    assert_eq!(name, "controller.kp");
    assert_eq!(value, "2.5");
    assert_eq!(socket, Some(PathBuf::from("/tmp/flowrt-main.sock")));
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
    } = cli.command
    else {
        panic!("deploy command should parse into Command::Deploy")
    };
    assert_eq!(bundle, PathBuf::from("dist/external-demo"));
    assert_eq!(host, "robot@192.0.2.10");
    assert_eq!(target, "pi");
    assert_eq!(remote_dir, "/tmp/flowrt-demo");
    assert!(dry_run);
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
