use crate::{
    ClockSourceKind, ContractIr, EntityRef, FaultInjectionIr, FaultInjectionPointIr, IrError,
    Result, TemporaryOverlayGenerationIr,
};

/// CLI 故障注入场景的一条注入点（投影前，按名引用契约实体）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaultInjectionScenarioPoint {
    pub instance: String,
    pub task: String,
    pub invocations: Vec<u64>,
    pub from_invocation: Option<u64>,
    pub reason: String,
}

/// 归一化 IR 上的一次性 test-only 故障注入场景。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaultInjectionScenario {
    pub points: Vec<FaultInjectionScenarioPoint>,
    pub generated_by: TemporaryOverlayGenerationIr,
}

/// 把 test-only 故障注入场景投影进已归一化的 Contract IR。
///
/// 该函数不修改源 RSDL，也不改图结构：它只把每条注入点的 `(instance, task)` 名解析为
/// canonical `EntityRef`，写入 [`ContractArtifactIr::fault_injection`]，并置 `test_only=true`、
/// `clock_source=SimulatedReplay`（注入产物的 run-to-run / record→replay 确定性依赖逻辑时钟）。
/// 已存在的 `temporary_overlay` 字段被保留，二者可叠加。调用方必须在投影后重新运行 validator。
pub fn apply_fault_injection_overlay(
    contract: &ContractIr,
    scenario: &FaultInjectionScenario,
) -> Result<ContractIr> {
    if scenario.points.is_empty() {
        return Err(IrError::InvalidValue {
            context: "fault injection overlay".to_string(),
            message: "at least one injection point is required".to_string(),
        });
    }

    let mut projected = contract.clone();
    let mut points = Vec::with_capacity(scenario.points.len());
    for point in &scenario.points {
        if point.invocations.is_empty() && point.from_invocation.is_none() {
            return Err(IrError::InvalidValue {
                context: "fault injection overlay".to_string(),
                message: format!(
                    "injection point `{}.{}` must set `invocations` or `from_invocation`",
                    point.instance, point.task
                ),
            });
        }
        if point.invocations.contains(&0) || point.from_invocation == Some(0) {
            return Err(IrError::InvalidValue {
                context: "fault injection overlay".to_string(),
                message: format!(
                    "injection point `{}.{}` uses 1-based invocation indices; 0 is invalid",
                    point.instance, point.task
                ),
            });
        }
        let (instance_ref, task_ref) = resolve_task(contract, &point.instance, &point.task)?;
        let mut invocations = point.invocations.clone();
        invocations.sort_unstable();
        invocations.dedup();
        points.push(FaultInjectionPointIr {
            instance: instance_ref,
            task: task_ref,
            invocations,
            from_invocation: point.from_invocation,
            reason: point.reason.trim().to_string(),
        });
    }

    // canonical：按 instance / task 名稳定排序，使落盘 IR 与声明顺序无关。
    points.sort_by(|left, right| {
        (&left.instance.name, &left.task.name).cmp(&(&right.instance.name, &right.task.name))
    });
    reject_duplicate_points(&points)?;

    projected.artifact.test_only = true;
    projected.artifact.clock_source = ClockSourceKind::SimulatedReplay;
    projected.artifact.fault_injection = Some(FaultInjectionIr {
        kind: "fault_injection".to_string(),
        generated_by: scenario.generated_by.clone(),
        points,
    });

    Ok(projected)
}

/// 把 `(instance, task)` 名解析为 canonical `(instance_ref, task_ref)`。
///
/// 在所有 graph 的 task 中查找唯一匹配；找不到或跨 graph 重名都视为错误，避免歧义注入。
fn resolve_task(
    contract: &ContractIr,
    instance: &str,
    task: &str,
) -> Result<(EntityRef, EntityRef)> {
    let mut found: Option<(EntityRef, EntityRef)> = None;
    for graph in &contract.graphs {
        for candidate in &graph.tasks {
            if candidate.instance.name == instance && candidate.name == task {
                if found.is_some() {
                    return Err(IrError::InvalidValue {
                        context: "fault injection overlay".to_string(),
                        message: format!(
                            "injection target `{instance}.{task}` is ambiguous across graphs"
                        ),
                    });
                }
                found = Some((
                    candidate.instance.clone(),
                    EntityRef {
                        id: candidate.id.clone(),
                        name: candidate.name.clone(),
                    },
                ));
            }
        }
    }
    found.ok_or_else(|| IrError::InvalidValue {
        context: "fault injection overlay".to_string(),
        message: format!("injection target `{instance}.{task}` not found in any graph"),
    })
}

fn reject_duplicate_points(points: &[FaultInjectionPointIr]) -> Result<()> {
    for window in points.windows(2) {
        if window[0].task.id == window[1].task.id {
            return Err(IrError::InvalidValue {
                context: "fault injection overlay".to_string(),
                message: format!(
                    "duplicate injection point for task `{}.{}`",
                    window[1].instance.name, window[1].task.name
                ),
            });
        }
    }
    Ok(())
}
