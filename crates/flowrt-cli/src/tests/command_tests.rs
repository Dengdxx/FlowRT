use super::*;

#[test]
fn cli_exposes_installed_binary_metadata() {
    let command = Cli::command();

    assert_eq!(command.get_name(), "flowrt");
    assert_eq!(command.get_version(), Some(env!("CARGO_PKG_VERSION")));
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
