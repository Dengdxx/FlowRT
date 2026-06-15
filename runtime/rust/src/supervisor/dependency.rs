//! 进程依赖排序和失败传播快照。

use std::collections::{BTreeSet, HashMap, VecDeque};

use super::manifest::{LaunchProcess, ReadinessGate};

/// 判断进程的所有依赖是否已满足。
///
/// 对于 `process_started` readiness gate，依赖进程只需已启动（PID 存在）。
/// 对于 `runtime_ready` 和 `service_ready` gate，依赖进程必须已通过 readiness 检查。
pub fn process_dependencies_satisfied(
    process: &LaunchProcess,
    spawned_names: &BTreeSet<String>,
    ready_names: &BTreeSet<String>,
) -> bool {
    process.depends_on.iter().all(|dependency| {
        // 如果依赖进程已通过 readiness gate 则满足；
        // 否则如果依赖进程已启动且本进程的 readiness 是 process_started 也满足。
        ready_names.contains(dependency)
            || (process.readiness == ReadinessGate::ProcessStarted
                && spawned_names.contains(dependency))
    })
}

/// 对进程列表做拓扑排序（BFS / Kahn 算法）。
///
/// 返回排序后的进程名列表；如果存在环或引用未声明的进程则返回错误。
pub fn resolve_dependency_order(processes: &[LaunchProcess]) -> Result<Vec<String>, String> {
    let all_names: BTreeSet<&str> = processes.iter().map(|p| p.name.as_str()).collect();

    // 校验依赖引用
    for process in processes {
        for dep in &process.depends_on {
            if !all_names.contains(dep.as_str()) {
                return Err(format!(
                    "process `{}` depends on unknown process `{}`",
                    process.name, dep
                ));
            }
            if dep == &process.name {
                return Err(format!("process `{}` depends on itself", process.name));
            }
        }
    }

    // Kahn 算法
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    for process in processes {
        in_degree.entry(&process.name).or_insert(0);
        dependents.entry(&process.name).or_default();
        for dep in &process.depends_on {
            *in_degree.entry(&process.name).or_insert(0) += 1;
            dependents
                .entry(dep.as_str())
                .or_default()
                .push(&process.name);
        }
    }

    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(name, _)| *name)
        .collect();
    let mut sorted = Vec::new();

    while let Some(name) = queue.pop_front() {
        sorted.push(name.to_string());
        if let Some(deps) = dependents.get(name) {
            for &dep in deps {
                let deg = in_degree
                    .get_mut(dep)
                    .expect("dependents entry must exist in in_degree map");
                *deg -= 1;
                if *deg == 0 {
                    queue.push_back(dep);
                }
            }
        }
    }

    if sorted.len() != processes.len() {
        return Err("FlowRT process dependencies contain a cycle".to_string());
    }

    Ok(sorted)
}

// ---------------------------------------------------------------------------
// 可执行文件解析
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 失败传播
// ---------------------------------------------------------------------------

/// BFS 传播失败：终止所有传递依赖于 `failed_process` 且 failure 策略为 propagate 的进程。
///
/// 返回被终止的进程名列表（不含原始失败进程）。
pub fn collect_propagated_failures(
    children: &[PropagatableChild],
    failed_process: &str,
) -> Vec<String> {
    let mut terminated = Vec::new();
    let mut pending = VecDeque::new();
    pending.push_back(failed_process.to_string());

    while let Some(failed) = pending.pop_front() {
        for child in children {
            if child.finished {
                continue;
            }
            if child.dependencies.iter().any(|dep| dep == &failed) {
                terminated.push(child.name.clone());
                if child.failure == "propagate" {
                    pending.push_back(child.name.clone());
                }
            }
        }
    }

    terminated
}

/// 用于失败传播计算的子进程快照。
#[derive(Debug, Clone)]
pub struct PropagatableChild {
    pub name: String,
    pub dependencies: Vec<String>,
    pub failure: String,
    pub finished: bool,
}
