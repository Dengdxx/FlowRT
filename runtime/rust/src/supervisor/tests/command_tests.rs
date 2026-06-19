use super::prelude::*;

#[test]
fn process_command_uses_runtime_kind_not_process_name_for_ros2_bridge() {
    let mut process = test_process("ros2_bridge", vec![]);
    process.runtime_kind = "rust".into();
    let command = build_process_command(Path::new("/tmp/flowrt_app"), &process, Some(5), None);

    assert_eq!(
        command_args(&command),
        vec!["--process", "ros2_bridge", "--flowrt-run-steps", "5"]
    );
    assert_eq!(command_env(&command, "RMW_IMPLEMENTATION"), None);
}

#[test]
fn ros2_bridge_runtime_command_sets_rmw_without_process_arg() {
    let mut process = test_process("adapter", vec![]);
    process.runtime_kind = "ros2_bridge".into();
    let command = build_process_command(
        Path::new("/tmp/flowrt_ros2_bridge"),
        &process,
        Some(7),
        None,
    );

    assert_eq!(command_args(&command), vec!["--flowrt-run-steps", "7"]);
    assert_eq!(
        command_env(&command, "RMW_IMPLEMENTATION").as_deref(),
        Some("rmw_zenoh_cpp")
    );
}

#[test]
fn ros2_bridge_runtime_command_isolates_ros2_zenoh_library_path() {
    let _lock = ROS2_ENV_LOCK.lock().expect("ros2 env lock should work");
    let _ament = EnvOverride::set(
        "AMENT_PREFIX_PATH",
        Some("/opt/ros/lyrical:/home/robot/ros_overlay"),
    );
    let _ld = EnvOverride::set(
        "LD_LIBRARY_PATH",
        Some(
            "/opt/flowrt/0.8.3/lib:/opt/ros/lyrical/lib/aarch64-linux-gnu:/opt/ros/lyrical/lib:/custom/lib:/opt/flowrt/0.8.3/targets/linux-arm64/lib",
        ),
    );
    let _flowrt_prefix = EnvOverride::set("FLOWRT_PRIVATE_PREFIX", Some("/opt/flowrt/0.8.3"));

    let mut process = test_process("adapter", vec![]);
    process.runtime_kind = "ros2_bridge".into();
    let command = build_process_command(Path::new("/tmp/flowrt_ros2_bridge"), &process, None, None);
    let library_path = command_env(&command, "LD_LIBRARY_PATH")
        .expect("ros2 bridge command should set isolated LD_LIBRARY_PATH");
    let paths = path_list(&library_path);

    assert_eq!(paths[0], "/opt/ros/lyrical/opt/zenoh_cpp_vendor/lib");
    assert_eq!(paths[1], "/home/robot/ros_overlay/opt/zenoh_cpp_vendor/lib");
    assert!(paths.contains(&"/opt/ros/lyrical/lib/aarch64-linux-gnu".to_string()));
    assert!(paths.contains(&"/opt/ros/lyrical/lib".to_string()));
    assert!(paths.contains(&"/custom/lib".to_string()));
    assert!(
        !paths.iter().any(|path| path.starts_with("/opt/flowrt/")),
        "{paths:?}"
    );
}

#[test]
fn process_command_applies_manifest_env() {
    let mut process = test_process("control", vec![]);
    process.env.insert("APP_MODE".into(), "control".into());

    let command = build_process_command(Path::new("/tmp/flowrt_app"), &process, None, None);

    assert_eq!(
        command_env(&command, "APP_MODE").as_deref(),
        Some("control")
    );
}

#[test]
fn process_command_sets_status_out_when_requested() {
    let process = test_process("controller", vec![]);
    let snapshot_dir = std::env::temp_dir().join("flowrt-status-out-command-test");
    let command = build_process_command_with_status_out(
        Path::new("/bin/echo"),
        &process,
        Some(5),
        None,
        Some(&snapshot_dir),
    );

    assert_eq!(
        command_args(&command),
        vec!["--process", "controller", "--flowrt-run-steps", "5"]
    );
    assert_eq!(
        command_env(&command, "FLOWRT_STATUS_OUT").as_deref(),
        Some(
            snapshot_dir
                .join("controller.status.json")
                .to_string_lossy()
                .as_ref()
        )
    );
}

#[test]
fn cpp_runtime_executable_prefers_sibling_local_bin() {
    let root = temp_test_dir("cpp-sibling");
    let bin_dir = root.join("flowrt/build/bin/release");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let supervisor = bin_dir.join(binary_name("robot-flowrt-supervisor"));
    let cpp_app = bin_dir.join(binary_name("robot_cpp_app"));
    std::fs::write(&supervisor, "").unwrap();
    std::fs::write(&cpp_app, "").unwrap();

    let resolved =
        app_executable_for_runtime(&supervisor, "cpp", "robot-flowrt-app", "robot_cpp_app", "")
            .unwrap();

    assert_eq!(resolved, cpp_app);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn c_runtime_executable_uses_cmake_app_binary() {
    let root = temp_test_dir("c-runtime-sibling");
    let bin_dir = root.join("flowrt/build/bin/release");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let supervisor = bin_dir.join(binary_name("robot-flowrt-supervisor"));
    let cpp_app = bin_dir.join(binary_name("robot_cpp_app"));
    std::fs::write(&supervisor, "").unwrap();
    std::fs::write(&cpp_app, "").unwrap();

    let resolved =
        app_executable_for_runtime(&supervisor, "c", "robot-flowrt-app", "robot_cpp_app", "")
            .unwrap();

    assert_eq!(resolved, cpp_app);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn ros2_bridge_runtime_executable_prefers_sibling_local_bin() {
    let root = temp_test_dir("ros2-sibling");
    let bin_dir = root.join("flowrt/build/bin/release");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let supervisor = bin_dir.join(binary_name("robot-flowrt-supervisor"));
    let bridge = bin_dir.join(binary_name("robot_ros2_bridge"));
    std::fs::write(&supervisor, "").unwrap();
    std::fs::write(&bridge, "").unwrap();

    let resolved = app_executable_for_runtime(
        &supervisor,
        "ros2_bridge",
        "robot-flowrt-app",
        "",
        "robot_ros2_bridge",
    )
    .unwrap();

    assert_eq!(resolved, bridge);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn external_runtime_executable_resolves_from_external_path() {
    let _lock = EXTERNAL_ENV_LOCK
        .lock()
        .expect("external env lock should work");
    let root = temp_test_dir("external-path");
    let package_root = root.join("packages/camera_driver");
    let executable = package_root.join("bin/camera-node");
    std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
    std::fs::write(&executable, "").unwrap();
    let _path = EnvOverride::set(
        "FLOWRT_EXTERNAL_PATH",
        Some(root.join("packages").to_str().unwrap()),
    );
    let current_exe = root.join("workspace/flowrt/build/bin/release/app-supervisor");
    let external = LaunchExternalProcess {
        package: "camera_driver".into(),
        executable: "bin/camera-node".into(),
        args: vec![],
        working_dir: LaunchExternalWorkingDir::Package,
        health: LaunchExternalHealth::RuntimeSocket,
        required_backends: vec!["zenoh".into()],
        package_root: None,
    };

    let resolved = external_app_executable(&current_exe, &external).unwrap();

    assert_eq!(resolved.executable, executable.canonicalize().unwrap());
    assert_eq!(resolved.package_root, package_root.canonicalize().unwrap());
    assert_eq!(resolved.workspace_root, root.join("workspace"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn external_runtime_executable_rejects_escape_paths() {
    let root = temp_test_dir("external-escape-paths");
    let current_exe = root.join("workspace/flowrt/build/bin/release/app-supervisor");
    for executable in ["/bin/sh", "../driver", "bin/../driver", "./driver"] {
        let external = LaunchExternalProcess {
            package: "camera_driver".into(),
            executable: executable.into(),
            args: vec![],
            working_dir: LaunchExternalWorkingDir::Package,
            health: LaunchExternalHealth::RuntimeSocket,
            required_backends: vec!["zenoh".into()],
            package_root: Some(root.join("packages/camera_driver")),
        };

        let error = external_app_executable(&current_exe, &external)
            .expect_err("escape executable path should be rejected");

        assert!(
            error.contains("must be package-relative"),
            "unexpected error for {executable}: {error}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn external_runtime_executable_rejects_symlink_escape() {
    let root = temp_test_dir("external-symlink-escape");
    let package_root = root.join("packages/camera_driver");
    let outside = root.join("outside/camera-node");
    std::fs::create_dir_all(package_root.join("bin")).unwrap();
    std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
    std::fs::write(&outside, "").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, package_root.join("bin/camera-node")).unwrap();
    let current_exe = root.join("workspace/flowrt/build/bin/release/app-supervisor");
    let external = LaunchExternalProcess {
        package: "camera_driver".into(),
        executable: "bin/camera-node".into(),
        args: vec![],
        working_dir: LaunchExternalWorkingDir::Package,
        health: LaunchExternalHealth::RuntimeSocket,
        required_backends: vec!["zenoh".into()],
        package_root: Some(package_root),
    };

    #[cfg(unix)]
    {
        let error = external_app_executable(&current_exe, &external)
            .expect_err("symlink escaping package root should be rejected");
        assert!(error.contains("escapes package root"));
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn external_process_command_uses_manifest_args_and_env_contract() {
    let external = LaunchExternalProcess {
        package: "camera_driver".into(),
        executable: "bin/camera-node".into(),
        args: vec!["--device".into(), "/dev/video0".into()],
        working_dir: LaunchExternalWorkingDir::Package,
        health: LaunchExternalHealth::RuntimeSocket,
        required_backends: vec!["zenoh".into()],
        package_root: None,
    };
    let resolution = ExternalExecutableResolution {
        executable: PathBuf::from("/opt/flowrt/external/camera_driver/bin/camera-node"),
        package_root: PathBuf::from("/opt/flowrt/external/camera_driver"),
        workspace_root: PathBuf::from("/tmp/robot_ws"),
    };
    let zenoh = ZenohLaunchEnv {
        listen: "tcp/127.0.0.1:7447".into(),
        connect: String::new(),
    };
    let mut process = test_process("camera_proc", vec![]);
    process.backend = "zenoh".into();
    process.runtime_kind = "external".into();
    process.env.insert("APP_MODE".into(), "driver".into());
    process
        .env
        .insert("FLOWRT_PROCESS".into(), "user_override".into());

    let command = build_external_process_command(
        &resolution.executable,
        &process,
        Some(5),
        Some(&zenoh),
        &external,
        &resolution,
    );

    assert_eq!(command_args(&command), vec!["--device", "/dev/video0"]);
    assert_eq!(
        command.get_current_dir(),
        Some(Path::new("/opt/flowrt/external/camera_driver"))
    );
    assert_eq!(
        command_env(&command, "FLOWRT_PROCESS").as_deref(),
        Some("camera_proc")
    );
    assert_eq!(
        command_env(&command, "FLOWRT_EXTERNAL_PACKAGE").as_deref(),
        Some("camera_driver")
    );
    assert_eq!(
        command_env(&command, "FLOWRT_BACKEND").as_deref(),
        Some("zenoh")
    );
    assert_eq!(
        command_env(&command, "FLOWRT_RUN_STEPS").as_deref(),
        Some("5")
    );
    assert_eq!(
        command_env(&command, "FLOWRT_ZENOH_LISTEN").as_deref(),
        Some("tcp/127.0.0.1:7447")
    );
    assert_eq!(command_env(&command, "APP_MODE").as_deref(), Some("driver"));
}

#[test]
fn cpp_runtime_executable_keeps_legacy_cmake_fallback() {
    let supervisor = Path::new("/tmp/app/flowrt/build/bin/release/robot-flowrt-supervisor");

    let resolved =
        app_executable_for_runtime(supervisor, "cpp", "robot-flowrt-app", "robot_cpp_app", "")
            .unwrap();

    assert_eq!(
        resolved,
        Path::new("/tmp/app/flowrt/build/cmake").join(binary_name("robot_cpp_app"))
    );
}
