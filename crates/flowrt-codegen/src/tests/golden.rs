//! golden 等价 harness：每个 corpus case 跑 `emit_artifacts`，逐 artifact 与 `expected/` 比字节。
//!
//! 字符串断言测试只看生成文本是否含某些行；golden 反过来锁定**整份**生成输出，作为任何 codegen
//! 改动的回归 oracle——重构若意外改了输出，这里立刻 RED。`FLOWRT_UPDATE_GOLDEN=1` 重生基线
//! （人工 review diff 后入库）。
use super::*;
use std::path::{Path, PathBuf};

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn update_mode() -> bool {
    std::env::var("FLOWRT_UPDATE_GOLDEN").as_deref() == Ok("1")
}

/// 定位首处不一致的行，给出可读 diff，避免直接 dump 整份文件。
fn first_diff(want: &str, got: &str) -> String {
    for (i, (a, b)) in want.lines().zip(got.lines()).enumerate() {
        if a != b {
            return format!("line {}:\n  want: {a}\n  got : {b}", i + 1);
        }
    }
    format!(
        "行内容一致但长度不同：want={} got={}",
        want.len(),
        got.len()
    )
}

/// 对一个 corpus case：读 `input.rsdl` → 归一化 → `emit_artifacts` → 逐 artifact 比对 `expected/`。
///
/// 若 case 目录存在 `inject.toml`（与 compile 网 `flowrt prepare --inject` 同源），先投影到默认
/// profile 并叠加故障注入 overlay 再 emit，使 golden 锁定注入 shell 的整份输出。
fn check_case(case: &str) {
    let dir = corpus_root().join(case);
    let rsdl = std::fs::read_to_string(dir.join("input.rsdl"))
        .unwrap_or_else(|e| panic!("读 {case}/input.rsdl 失败：{e}"));
    let ir = contract_from_source(&rsdl);
    let ir = match load_injection_scenario(&dir) {
        Some(scenario) => {
            let projected = flowrt_ir::project_contract_to_profile(&ir, None)
                .unwrap_or_else(|e| panic!("project {case} 失败：{e:?}"));
            flowrt_ir::apply_fault_injection_overlay(&projected, &scenario)
                .unwrap_or_else(|e| panic!("inject overlay {case} 失败：{e:?}"))
        }
        None => ir,
    };
    let bundle = emit_artifacts(&ir).unwrap_or_else(|e| panic!("emit {case} 失败：{e:?}"));
    let expected_root = dir.join("expected");
    let update = update_mode();

    for artifact in &bundle.artifacts {
        let golden = expected_root.join(&artifact.relative_path);
        if update {
            std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
            std::fs::write(&golden, &artifact.content).unwrap();
            continue;
        }
        let want = std::fs::read_to_string(&golden).unwrap_or_else(|_| {
            panic!(
                "缺 golden {case}/expected/{} —— 跑 FLOWRT_UPDATE_GOLDEN=1 重生并 review",
                artifact.relative_path.display()
            )
        });
        assert!(
            want == artifact.content,
            "golden 漂移 {case}/{}：review 后用 FLOWRT_UPDATE_GOLDEN=1 重生\n{}",
            artifact.relative_path.display(),
            first_diff(&want, &artifact.content)
        );
    }
}

#[derive(serde::Deserialize)]
struct InjectionScenarioFile {
    #[serde(default)]
    inject: Vec<InjectionScenarioEntry>,
}

#[derive(serde::Deserialize)]
struct InjectionScenarioEntry {
    #[serde(default)]
    kind: flowrt_ir::FaultInjectionKind,
    instance: String,
    task: String,
    #[serde(default)]
    invocations: Vec<u64>,
    #[serde(default)]
    from_invocation: Option<u64>,
    #[serde(default)]
    reason: String,
}

/// 读取 case 目录下可选 `inject.toml`，构造与 CLI `--inject` 同形的 `FaultInjectionScenario`。
fn load_injection_scenario(dir: &Path) -> Option<flowrt_ir::FaultInjectionScenario> {
    let text = std::fs::read_to_string(dir.join("inject.toml")).ok()?;
    let parsed: InjectionScenarioFile =
        toml::from_str(&text).unwrap_or_else(|e| panic!("解析 inject.toml 失败：{e}"));
    let points = parsed
        .inject
        .into_iter()
        .map(|entry| flowrt_ir::FaultInjectionScenarioPoint {
            kind: entry.kind,
            instance: entry.instance,
            task: entry.task,
            invocations: entry.invocations,
            from_invocation: entry.from_invocation,
            reason: entry.reason,
        })
        .collect();
    Some(flowrt_ir::FaultInjectionScenario {
        points,
        generated_by: flowrt_ir::TemporaryOverlayGenerationIr {
            command: "flowrt prepare".to_string(),
            source: dir.join("input.rsdl").display().to_string(),
        },
    })
}

#[test]
fn golden_island_rust_onmsg() {
    check_case("island_rust_onmsg");
}

#[test]
fn golden_island_cpp_onmsg() {
    check_case("island_cpp_onmsg");
}

#[test]
fn golden_sensor_event_time_rust() {
    check_case("sensor_event_time_rust");
}

#[test]
fn golden_sensor_event_time_cpp() {
    check_case("sensor_event_time_cpp");
}

#[test]
fn golden_graph_latest_fifo() {
    check_case("graph_latest_fifo");
}

#[test]
fn golden_service_rust() {
    check_case("service_rust");
}

#[test]
fn golden_zenoh_service_rust() {
    check_case("zenoh_service_rust");
}

#[test]
fn golden_zenoh_service_cpp() {
    check_case("zenoh_service_cpp");
}

#[test]
fn golden_sync_fusion_rust() {
    check_case("sync_fusion_rust");
}

#[test]
fn golden_sync_fusion_cpp() {
    check_case("sync_fusion_cpp");
}

#[test]
fn golden_feedback_loop_rust() {
    check_case("feedback_loop_rust");
}

#[test]
fn golden_feedback_loop_cpp() {
    check_case("feedback_loop_cpp");
}

#[test]
fn golden_feedback_v2_rust() {
    check_case("feedback_v2_rust");
}

#[test]
fn golden_feedback_v2_cpp() {
    check_case("feedback_v2_cpp");
}

#[test]
fn golden_instance_fault_restart_rust() {
    check_case("instance_fault_restart_rust");
}

#[test]
fn golden_instance_fault_restart_cpp() {
    check_case("instance_fault_restart_cpp");
}

#[test]
fn golden_instance_degrade_rust() {
    check_case("instance_degrade_rust");
}

#[test]
fn golden_instance_degrade_cpp() {
    check_case("instance_degrade_cpp");
}

#[test]
fn golden_graph_health_stop_rust() {
    check_case("graph_health_stop_rust");
}

#[test]
fn golden_graph_health_stop_cpp() {
    check_case("graph_health_stop_cpp");
}

#[test]
fn golden_cross_process_feedback_rust() {
    check_case("cross_process_feedback_rust");
}

#[test]
fn golden_cross_process_feedback_cpp() {
    check_case("cross_process_feedback_cpp");
}

#[test]
fn golden_fault_injection_restart_rust() {
    check_case("fault_injection_restart_rust");
}

#[test]
fn golden_fault_injection_restart_cpp() {
    check_case("fault_injection_restart_cpp");
}

#[test]
fn golden_fault_injection_degrade_recover_rust() {
    check_case("fault_injection_degrade_recover_rust");
}

#[test]
fn golden_fault_injection_degrade_recover_cpp() {
    check_case("fault_injection_degrade_recover_cpp");
}
