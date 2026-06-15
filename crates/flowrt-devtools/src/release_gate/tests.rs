use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::checks::check_registry_for_version;
use super::registry::ReleaseGateRegistry;

fn temp_repo_root(test_name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time must be after unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "flowrt-devtools-{test_name}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create temp repo root");
    root
}

fn write_script(repo_root: &std::path::Path, relative_path: &str) {
    let path = repo_root.join(relative_path);
    fs::create_dir_all(path.parent().expect("script has parent")).expect("create script dir");
    fs::write(path, "#!/usr/bin/env bash\n").expect("write script");
}

#[test]
fn release_gate_registered_focused_smoke_query_succeeds() {
    let repo_root = temp_repo_root("registered");
    write_script(&repo_root, "scripts/test-v0141-architecture-smoke.sh");
    let registry = ReleaseGateRegistry::from_toml_str(
        r#"
            [[focused_smoke]]
            version = "0.14.1"
            script = "scripts/test-v0141-architecture-smoke.sh"
        "#,
    )
    .expect("parse registry");

    let gate = registry
        .checked_focused_smoke(&repo_root, "0.14.1")
        .expect("registered version should resolve");

    assert_eq!(
        gate.script(),
        std::path::Path::new("scripts/test-v0141-architecture-smoke.sh")
    );
}

#[test]
fn release_gate_unknown_version_fails() {
    let repo_root = temp_repo_root("unknown");
    write_script(&repo_root, "scripts/test-v0141-architecture-smoke.sh");
    let registry = ReleaseGateRegistry::from_toml_str(
        r#"
            [[focused_smoke]]
            version = "0.14.1"
            script = "scripts/test-v0141-architecture-smoke.sh"
        "#,
    )
    .expect("parse registry");

    let err = registry
        .checked_focused_smoke(&repo_root, "9.9.9")
        .expect_err("unknown version must fail");

    assert!(err.to_string().contains("未登记"));
}

#[test]
fn release_gate_duplicate_version_fails() {
    let repo_root = temp_repo_root("duplicate");
    write_script(&repo_root, "scripts/one.sh");
    write_script(&repo_root, "scripts/two.sh");
    let registry = ReleaseGateRegistry::from_toml_str(
        r#"
            [[focused_smoke]]
            version = "0.14.1"
            script = "scripts/one.sh"

            [[focused_smoke]]
            version = "0.14.1"
            script = "scripts/two.sh"
        "#,
    )
    .expect("parse registry");

    let err = check_registry_for_version(&registry, &repo_root, "0.14.1")
        .expect_err("duplicate version must fail");

    assert!(err.to_string().contains("重复版本"));
}

#[test]
fn release_gate_missing_script_fails_for_non_planned_entry() {
    let repo_root = temp_repo_root("missing");
    let registry = ReleaseGateRegistry::from_toml_str(
        r#"
            [[focused_smoke]]
            version = "0.14.1"
            script = "scripts/missing-smoke.sh"
        "#,
    )
    .expect("parse registry");

    let err = check_registry_for_version(&registry, &repo_root, "0.14.1")
        .expect_err("missing non-planned script must fail");

    assert!(err.to_string().contains("引用脚本不存在"));
}

#[test]
fn release_gate_planned_entry_allows_missing_script() {
    let repo_root = temp_repo_root("planned");
    let registry = ReleaseGateRegistry::from_toml_str(
        r#"
            [[focused_smoke]]
            version = "0.15.0"
            script = "scripts/test-v0150-architecture-convergence-smoke.sh"
            planned = true
        "#,
    )
    .expect("parse registry");

    let gate = registry
        .checked_focused_smoke(&repo_root, "0.15.0")
        .expect("planned missing script should resolve");

    assert_eq!(
        gate.script(),
        std::path::Path::new("scripts/test-v0150-architecture-convergence-smoke.sh")
    );
}
