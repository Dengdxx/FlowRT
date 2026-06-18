use super::*;

/// 构造一个最小 strict 契约：单 periodic instance `flaky`，匿名 task 归一化名 `main`。
fn flaky_contract() -> crate::ContractIr {
    let source = r#"
[package]
name = "fault_injection_demo"
rsdl_version = "0.1"

[type.Tick]
value = "u32"

[component.flaky]
language = "rust"
output = ["out:Tick"]

[instance.flaky]
component = "flaky"

[instance.flaky.task]
trigger = "periodic"
period_ms = 10
output = ["out"]

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    project_contract_to_profile(&ir, None).unwrap()
}

#[test]
fn fault_injection_overlay_marks_test_only_and_resolves_refs() {
    let projected = flaky_contract();
    let scenario = FaultInjectionScenario {
        points: vec![FaultInjectionScenarioPoint {
            instance: "flaky".to_string(),
            task: "main".to_string(),
            invocations: vec![3, 1, 1, 2],
            from_invocation: None,
            reason: "  drive isolate  ".to_string(),
        }],
        generated_by: Default::default(),
    };

    let injected = apply_fault_injection_overlay(&projected, &scenario).unwrap();

    assert!(injected.artifact.test_only);
    assert_eq!(
        injected.artifact.clock_source,
        ClockSourceKind::SimulatedReplay
    );
    let fault = injected
        .artifact
        .fault_injection
        .as_ref()
        .expect("fault injection artifact must record scenario metadata");
    assert_eq!(fault.kind, "fault_injection");
    assert_eq!(fault.points.len(), 1);
    let point = &fault.points[0];
    assert_eq!(point.instance.name, "flaky");
    assert_eq!(point.task.name, "main");
    // 调用序号 canonical 升序去重。
    assert_eq!(point.invocations, vec![1, 2, 3]);
    assert_eq!(point.reason, "drive isolate");
    // EntityRef 指向归一化 task。
    let task = &injected.graphs[0].tasks[0];
    assert_eq!(point.task.id, task.id);
    assert_eq!(point.instance.id, task.instance.id);
}

#[test]
fn fault_injection_overlay_preserves_existing_temporary_overlay() {
    let projected = flaky_contract();
    let scenario = FaultInjectionScenario {
        points: vec![FaultInjectionScenarioPoint {
            instance: "flaky".to_string(),
            task: "main".to_string(),
            invocations: vec![],
            from_invocation: Some(2),
            reason: String::new(),
        }],
        generated_by: Default::default(),
    };

    let injected = apply_fault_injection_overlay(&projected, &scenario).unwrap();
    // fault injection 不改图结构，但置 test_only + SimulatedReplay。
    assert!(injected.artifact.fault_injection.is_some());
    assert_eq!(
        injected.artifact.fault_injection.unwrap().points[0].from_invocation,
        Some(2)
    );
}

#[test]
fn fault_injection_overlay_rejects_empty_scenario() {
    let projected = flaky_contract();
    let scenario = FaultInjectionScenario {
        points: vec![],
        generated_by: Default::default(),
    };
    let error = apply_fault_injection_overlay(&projected, &scenario).unwrap_err();
    assert!(matches!(error, IrError::InvalidValue { .. }));
}

#[test]
fn fault_injection_overlay_rejects_point_without_invocations() {
    let projected = flaky_contract();
    let scenario = FaultInjectionScenario {
        points: vec![FaultInjectionScenarioPoint {
            instance: "flaky".to_string(),
            task: "main".to_string(),
            invocations: vec![],
            from_invocation: None,
            reason: String::new(),
        }],
        generated_by: Default::default(),
    };
    let error = apply_fault_injection_overlay(&projected, &scenario).unwrap_err();
    assert!(matches!(error, IrError::InvalidValue { .. }));
}

#[test]
fn fault_injection_overlay_rejects_zero_invocation_index() {
    let projected = flaky_contract();
    let scenario = FaultInjectionScenario {
        points: vec![FaultInjectionScenarioPoint {
            instance: "flaky".to_string(),
            task: "main".to_string(),
            invocations: vec![0],
            from_invocation: None,
            reason: String::new(),
        }],
        generated_by: Default::default(),
    };
    let error = apply_fault_injection_overlay(&projected, &scenario).unwrap_err();
    assert!(matches!(error, IrError::InvalidValue { .. }));
}

#[test]
fn fault_injection_overlay_rejects_unknown_task() {
    let projected = flaky_contract();
    let scenario = FaultInjectionScenario {
        points: vec![FaultInjectionScenarioPoint {
            instance: "flaky".to_string(),
            task: "on_message".to_string(),
            invocations: vec![1],
            from_invocation: None,
            reason: String::new(),
        }],
        generated_by: Default::default(),
    };
    let error = apply_fault_injection_overlay(&projected, &scenario).unwrap_err();
    assert!(matches!(error, IrError::InvalidValue { .. }));
}
