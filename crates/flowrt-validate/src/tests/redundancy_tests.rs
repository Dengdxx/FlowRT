use super::*;

#[test]
fn accepts_standby_redundancy_group_with_global_tick_profile() {
    let source = r#"
[package]
name = "redundancy_ok"
rsdl_version = "0.1"

[type.Command]
value = "u32"

[component.controller]
language = "rust"
output = ["command:Command"]

[instance.controller_a]
component = "controller"

[instance.controller_a.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[instance.controller_b]
component = "controller"

[instance.controller_b.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[[redundancy.group]]
name = "controller_ha"
mode = "standby"
primary = "controller_a"
standby = ["controller_b"]
trigger = "critical_fault"

[profile.default]
backend = "inproc"

[profile.default.determinism]
mode = "global_tick"
timeout_ms = 1000

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    validate_contract(&ir).expect("global_tick standby redundancy group should validate");
}

#[test]
fn rejects_standby_redundancy_without_global_tick_profile() {
    let source = r#"
[package]
name = "redundancy_no_tick"
rsdl_version = "0.1"

[type.Command]
value = "u32"

[component.controller]
language = "rust"
output = ["command:Command"]

[instance.controller_a]
component = "controller"

[instance.controller_a.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[instance.controller_b]
component = "controller"

[instance.controller_b.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[[redundancy.group]]
name = "controller_ha"
mode = "standby"
primary = "controller_a"
standby = ["controller_b"]
trigger = "critical_fault"

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("standby redundancy requires global_tick");
    assert!(
        report.errors.iter().any(|error| error.message.contains(
            "redundancy group `controller_ha` standby requires profile determinism mode global_tick"
        )),
        "{:?}",
        report.errors
    );
}

#[test]
fn rejects_redundancy_group_member_shape_mismatch() {
    let source = r#"
[package]
name = "redundancy_shape_bad"
rsdl_version = "0.1"

[type.Command]
value = "u32"

[type.Other]
value = "u64"

[component.controller]
language = "rust"
output = ["command:Command"]

[component.backup]
language = "rust"
output = ["command:Other"]

[instance.controller_a]
component = "controller"

[instance.controller_a.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[instance.controller_b]
component = "backup"

[instance.controller_b.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[[redundancy.group]]
name = "controller_ha"
mode = "standby"
primary = "controller_a"
standby = ["controller_b"]
trigger = "critical_fault"

[profile.default]
backend = "inproc"

[profile.default.determinism]
mode = "global_tick"
timeout_ms = 1000
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("redundancy members must expose matching shape");
    assert!(
        report.errors.iter().any(|error| error
            .message
            .contains("redundancy group `controller_ha` members must use the same component type")),
        "{:?}",
        report.errors
    );
    assert!(
        report.errors.iter().any(|error| error
            .message
            .contains("redundancy group `controller_ha` members must have identical port shape")),
        "{:?}",
        report.errors
    );
}

#[test]
fn rejects_direct_bind_from_standby_redundancy_output() {
    let source = r#"
[package]
name = "redundancy_standby_output_bad"
rsdl_version = "0.1"

[type.Command]
value = "u32"

[component.controller]
language = "rust"
output = ["command:Command"]

[component.sink]
language = "rust"
input = ["command:Command"]

[instance.controller_a]
component = "controller"

[instance.controller_a.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[instance.controller_b]
component = "controller"

[instance.controller_b.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["command"]

[[bind.dataflow]]
from = "controller_b.command"
to = "sink.command"
channel = "latest"

[[redundancy.group]]
name = "controller_ha"
mode = "standby"
primary = "controller_a"
standby = ["controller_b"]
trigger = "critical_fault"

[profile.default]
backend = "inproc"

[profile.default.determinism]
mode = "global_tick"
timeout_ms = 1000
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("standby outputs require group endpoint");
    assert!(
        report.errors.iter().any(|error| error
            .message
            .contains("standby output `controller_b.command` cannot be consumed directly")),
        "{:?}",
        report.errors
    );
}

#[test]
fn rejects_instance_joining_multiple_redundancy_groups() {
    let source = r#"
[package]
name = "redundancy_membership_bad"
rsdl_version = "0.1"

[type.Command]
value = "u32"

[component.controller]
language = "rust"
output = ["command:Command"]

[instance.controller_a]
component = "controller"

[instance.controller_a.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[instance.controller_b]
component = "controller"

[instance.controller_b.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[instance.controller_c]
component = "controller"

[instance.controller_c.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[[redundancy.group]]
name = "controller_ha"
mode = "standby"
primary = "controller_a"
standby = ["controller_b"]
trigger = "critical_fault"

[[redundancy.group]]
name = "controller_ha_2"
mode = "standby"
primary = "controller_c"
standby = ["controller_b"]
trigger = "critical_fault"

[profile.default]
backend = "inproc"

[profile.default.determinism]
mode = "global_tick"
timeout_ms = 1000
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report =
        validate_contract(&ir).expect_err("instance cannot join multiple redundancy groups");
    assert!(
        report.errors.iter().any(|error| error
            .message
            .contains("instance `controller_b` cannot join more than one active redundancy group")),
        "{:?}",
        report.errors
    );
}
