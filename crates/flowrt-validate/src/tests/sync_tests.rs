use super::*;

/// 多传感器同步契约构造器。默认渲染一个合法的双传感器 `on_synchronized` 融合图，
/// 各 reject 用例只覆写一个字段以制造单点缺陷。
struct SyncCase {
    imu_timestamp: bool,
    odom_timestamp: bool,
    fusion_trigger: &'static str,
    fusion_sync_line: &'static str,
    fusion_extra: &'static str,
    imu_bind: bool,
    sync_inputs: &'static str,
    sync_tolerance: &'static str,
    sync_instance: &'static str,
}

impl Default for SyncCase {
    fn default() -> Self {
        Self {
            imu_timestamp: true,
            odom_timestamp: true,
            fusion_trigger: "on_synchronized",
            fusion_sync_line: "sync = \"fused_in\"",
            fusion_extra: "",
            imu_bind: true,
            sync_inputs: "[\"imu\", \"odom\"]",
            sync_tolerance: "tolerance_ms = 10",
            sync_instance: "fusion",
        }
    }
}

impl SyncCase {
    fn source(&self) -> String {
        let imu_ts = if self.imu_timestamp {
            "\n[type.Imu.timestamp]\nfield = \"stamp_ns\"\nunit = \"ns\"\n"
        } else {
            ""
        };
        let odom_ts = if self.odom_timestamp {
            "\n[type.Odom.timestamp]\nfield = \"stamp_ns\"\nunit = \"ns\"\n"
        } else {
            ""
        };
        let imu_bind = if self.imu_bind {
            "[[bind.dataflow]]\nfrom = \"imu_src.imu\"\nto = \"fusion.imu\"\nchannel = \"latest\"\n\n"
        } else {
            ""
        };
        format!(
            r#"
[package]
name = "fusion_demo"
rsdl_version = "0.1"

[type.Imu]
ax = "f64"
stamp_ns = "u64"
{imu_ts}
[type.Odom]
vx = "f64"
stamp_ns = "u64"
{odom_ts}
[component.imu_src]
language = "rust"
output = ["imu:Imu"]

[component.odom_src]
language = "rust"
output = ["odom:Odom"]

[component.fusion]
language = "rust"
input = ["imu:Imu", "odom:Odom"]

[instance.imu_src]
component = "imu_src"
target = "linux"

[instance.imu_src.task]
trigger = "periodic"
period_ms = 10
output = ["imu"]

[instance.odom_src]
component = "odom_src"
target = "linux"

[instance.odom_src.task]
trigger = "periodic"
period_ms = 10
output = ["odom"]

[instance.fusion]
component = "fusion"
target = "linux"

[instance.fusion.task]
trigger = "{trigger}"
{sync_line}
{extra}

{imu_bind}[[bind.dataflow]]
from = "odom_src.odom"
to = "fusion.odom"
channel = "latest"

[[sync]]
name = "fused_in"
instance = "{sync_instance}"
inputs = {inputs}
{tolerance}

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
            imu_ts = imu_ts,
            odom_ts = odom_ts,
            trigger = self.fusion_trigger,
            sync_line = self.fusion_sync_line,
            extra = self.fusion_extra,
            imu_bind = imu_bind,
            sync_instance = self.sync_instance,
            inputs = self.sync_inputs,
            tolerance = self.sync_tolerance,
        )
    }

    fn normalize(&self) -> flowrt_ir::Result<ContractIr> {
        let source = self.source();
        let raw = parse_str(&source).unwrap();
        normalize_document(&raw, hash_source(&source))
    }

    fn contract(&self) -> ContractIr {
        self.normalize().unwrap()
    }
}

#[test]
fn accepts_well_formed_sync_group() {
    let ir = SyncCase::default().contract();
    validate_contract(&ir).unwrap();
}

#[test]
fn rejects_zero_tolerance() {
    let ir = SyncCase {
        sync_tolerance: "tolerance_ms = 0",
        ..Default::default()
    }
    .contract();
    let report = validate_contract(&ir).expect_err("zero tolerance should fail");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("tolerance_ms greater than zero"))
    );
}

#[test]
fn rejects_fewer_than_two_inputs() {
    let ir = SyncCase {
        sync_inputs: "[\"imu\"]",
        ..Default::default()
    }
    .contract();
    let report = validate_contract(&ir).expect_err("single input should fail");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("at least two inputs"))
    );
}

#[test]
fn rejects_undeclared_input_port() {
    let ir = SyncCase {
        sync_inputs: "[\"imu\", \"ghost\"]",
        ..Default::default()
    }
    .contract();
    let report = validate_contract(&ir).expect_err("undeclared input should fail");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("undeclared input port `ghost`"))
    );
}

#[test]
fn rejects_input_without_timestamp_source() {
    let ir = SyncCase {
        imu_timestamp: false,
        ..Default::default()
    }
    .contract();
    let report = validate_contract(&ir).expect_err("missing timestamp should fail");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("must declare a timestamp source"))
    );
}

#[test]
fn rejects_input_without_incoming_bind() {
    let ir = SyncCase {
        imu_bind: false,
        ..Default::default()
    }
    .contract();
    let report = validate_contract(&ir).expect_err("missing incoming bind should fail");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("has no incoming bind"))
    );
}

#[test]
fn rejects_on_synchronized_task_without_sync_group() {
    let ir = SyncCase {
        fusion_sync_line: "",
        ..Default::default()
    }
    .contract();
    let report = validate_contract(&ir).expect_err("missing sync reference should fail");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("must reference a sync group"))
    );
}

#[test]
fn rejects_on_synchronized_task_listing_inputs() {
    let ir = SyncCase {
        fusion_extra: "input = [\"imu\"]",
        ..Default::default()
    }
    .contract();
    let report = validate_contract(&ir).expect_err("on_synchronized must not list inputs");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("must not list inputs"))
    );
}

#[test]
fn rejects_sync_on_non_synchronized_task() {
    let ir = SyncCase {
        fusion_trigger: "periodic",
        fusion_extra: "period_ms = 10",
        ..Default::default()
    }
    .contract();
    let report = validate_contract(&ir).expect_err("periodic task must not set sync");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("must not set sync"))
    );
}

#[test]
fn normalize_rejects_sync_group_on_unknown_instance() {
    let error = SyncCase {
        sync_instance: "ghost",
        ..Default::default()
    }
    .normalize()
    .expect_err("unknown instance should fail normalization");
    assert!(error.to_string().contains("unknown instance `ghost`"));
}

#[test]
fn validator_rejects_tampered_sync_group_instance() {
    let mut ir = SyncCase::default().contract();
    // 模拟手工篡改落盘 IR：把 sync 组 instance 改成不存在的实体。
    ir.graphs[0].sync_groups[0].instance.name = "ghost".to_string();
    let report = validate_contract(&ir).expect_err("tampered instance should fail");
    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("references unknown instance `ghost`")
    }));
}
