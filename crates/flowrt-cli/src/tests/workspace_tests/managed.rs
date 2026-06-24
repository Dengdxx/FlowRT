use super::*;
use std::time::Duration;

fn write_fake_managed_bundle(root: &Path, package: &str, target: &str, payload: &[u8]) -> PathBuf {
    let payload_tag = payload
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let bundle = root.join(format!("bundle-{package}-{target}-{payload_tag}"));
    let platform = "linux-amd64";
    let supervisor = PathBuf::from("bin")
        .join(platform)
        .join("flowrt-supervisor");
    std::fs::create_dir_all(bundle.join(supervisor.parent().unwrap())).unwrap();
    std::fs::write(bundle.join(&supervisor), payload).unwrap();
    let supervisor_hash = file_sha256(&bundle.join(&supervisor)).unwrap();
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: package.to_string(),
        profile: Some("default".to_string()),
        artifact_mode: "strict".to_string(),
        temporary_overlay: false,
        test_only: false,
        target: target.to_string(),
        platform: Some(platform.to_string()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: supervisor.to_string_lossy().into_owned(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![],
        artifacts: vec![BundleArtifact {
            kind: "supervisor".to_string(),
            target: target.to_string(),
            platform: Some(platform.to_string()),
            path: supervisor,
            sha256: supervisor_hash,
        }],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();
    bundle
}

fn write_fake_managed_bundle_with_script_label(
    root: &Path,
    package: &str,
    target: &str,
    label: &str,
) -> PathBuf {
    let script = format!(
        "#!/usr/bin/env sh\n\
         echo started-{label}\n\
         trap 'echo stopped-{label}; exit 0' TERM\n\
         while true; do sleep 1; done\n"
    );
    let bundle = write_fake_managed_bundle(root, package, target, script.as_bytes());
    let script = bundle.join("bin/linux-amd64/flowrt-supervisor");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();
    }
    bundle
}

fn write_fake_managed_bundle_with_script(root: &Path, package: &str, target: &str) -> PathBuf {
    write_fake_managed_bundle_with_script_label(root, package, target, "default")
}

#[test]
fn managed_install_writes_release_metadata_and_active_pointer() {
    let root = temp_test_dir("managed-install-active");
    let bundle = write_fake_managed_bundle(&root, "managed_demo", "edge", b"supervisor-v1");
    let remote_dir = root.join("remote");

    let summary = managed_install(&bundle, &remote_dir, "edge", true).unwrap();

    assert!(
        summary.message.contains("installed FlowRT release"),
        "unexpected summary: {}",
        summary.message
    );
    let active = load_managed_active(&remote_dir).unwrap();
    assert_eq!(active.current_release, summary.release_id);
    assert_eq!(active.previous_release, None);
    assert!(
        remote_dir
            .join("releases")
            .join(&summary.release_id)
            .join("bundle.toml")
            .is_file()
    );
    assert!(
        remote_dir
            .join("releases")
            .join(&summary.release_id)
            .join("flowrt-managed-release.json")
            .is_file()
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn managed_rollback_restores_previous_release() {
    let root = temp_test_dir("managed-rollback");
    let first_bundle = write_fake_managed_bundle(&root, "managed_demo", "edge", b"supervisor-v1");
    let second_bundle = write_fake_managed_bundle(&root, "managed_demo", "edge", b"supervisor-v2");
    let remote_dir = root.join("remote");
    let first = managed_install(&first_bundle, &remote_dir, "edge", true).unwrap();
    let second = managed_install(&second_bundle, &remote_dir, "edge", true).unwrap();

    let active = load_managed_active(&remote_dir).unwrap();
    assert_eq!(active.current_release, second.release_id);
    assert_eq!(
        active.previous_release.as_deref(),
        Some(first.release_id.as_str())
    );

    let rollback = managed_rollback(&remote_dir, false).unwrap();
    let active = load_managed_active(&remote_dir).unwrap();
    assert_eq!(rollback.release_id, first.release_id);
    assert_eq!(active.current_release, first.release_id);
    assert_eq!(
        active.previous_release.as_deref(),
        Some(second.release_id.as_str())
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn managed_start_status_logs_and_stop_track_run_state() {
    let root = temp_test_dir("managed-run");
    let bundle = write_fake_managed_bundle_with_script(&root, "managed_demo", "edge");
    let remote_dir = root.join("remote");
    let install = managed_install(&bundle, &remote_dir, "edge", true).unwrap();

    let start = managed_start(&remote_dir, None).unwrap();
    assert_eq!(start.release_id, install.release_id);

    let status = managed_status(&remote_dir).unwrap();
    assert_eq!(status.active.current_release, install.release_id);
    assert_eq!(status.run.as_ref().unwrap().state, "running");

    let mut logs = String::new();
    for _ in 0..20 {
        logs = managed_logs(&remote_dir, 20).unwrap();
        if logs.contains("started") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(logs.contains("started"), "unexpected logs: {logs}");

    let stop = managed_stop(&remote_dir, Duration::from_millis(1000)).unwrap();
    assert!(
        stop.contains("stopped FlowRT release"),
        "unexpected stop summary: {stop}"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn managed_rollback_with_start_stops_current_run_before_starting_previous() {
    let root = temp_test_dir("managed-rollback-start");
    let first_bundle =
        write_fake_managed_bundle_with_script_label(&root, "managed_demo", "edge", "v1");
    let second_bundle =
        write_fake_managed_bundle_with_script_label(&root, "managed_demo", "edge", "v2");
    let remote_dir = root.join("remote");
    let first = managed_install(&first_bundle, &remote_dir, "edge", true).unwrap();
    let second = managed_install(&second_bundle, &remote_dir, "edge", true).unwrap();
    let run = managed_start(&remote_dir, None).unwrap();
    assert_eq!(run.release_id, second.release_id);

    let rollback = managed_rollback(&remote_dir, true).unwrap();

    assert_eq!(rollback.release_id, first.release_id);
    let status = managed_status(&remote_dir).unwrap();
    assert_eq!(status.active.current_release, first.release_id);
    assert_eq!(
        status.run.as_ref().map(|run| run.release_id.as_str()),
        Some(first.release_id.as_str())
    );
    assert_eq!(
        status.run.as_ref().map(|run| run.state.as_str()),
        Some("running")
    );
    let mut logs = String::new();
    for _ in 0..20 {
        logs = managed_logs(&remote_dir, 20).unwrap();
        if logs.contains("started-v1") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(logs.contains("started-v1"), "unexpected logs: {logs}");
    let _ = managed_stop(&remote_dir, Duration::from_millis(1000));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn remote_start_dry_run_projects_managed_command() {
    let output = remote_start("robot@192.0.2.10", "/opt/robot", None, true).unwrap();

    assert!(output.contains("ssh -- robot@192.0.2.10 flowrt"));
    assert!(output.contains("managed start"));
    assert!(output.contains("--remote-dir /opt/robot"));
}
