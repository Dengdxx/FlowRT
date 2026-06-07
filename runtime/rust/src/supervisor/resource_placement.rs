//! 进程资源提示：CPU affinity、nice 和可选 Linux RT policy / priority。
//!
//! 平台调用集中在本模块，不散落在 supervisor 主流程。
//! 无权限或平台不支持时，返回结构化诊断而不是 panic 或静默忽略。

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// 配置结构（从 manifest 反序列化）
// ---------------------------------------------------------------------------

/// 进程资源提示配置。
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct ResourcePlacement {
    /// CPU affinity 核心列表（从 0 开始）。空表示不设置。
    #[serde(default)]
    pub cpu_affinity: Vec<u32>,
    /// nice 值（-20..=19）。None 表示不设置。
    #[serde(default)]
    pub nice: Option<i32>,
    /// Linux RT 调度策略。None 表示不设置。
    #[serde(default)]
    pub rt_policy: Option<RtPolicy>,
    /// RT 优先级（1..=99）。仅在 rt_policy 为 Some 时有效。
    #[serde(default)]
    pub rt_priority: Option<u32>,
}

/// Linux RT 调度策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RtPolicy {
    /// SCHED_FIFO：先进先出实时调度。
    Fifo,
    /// SCHED_RR：轮转实时调度。
    RoundRobin,
}

// ---------------------------------------------------------------------------
// 应用结果结构（暴露给 status）
// ---------------------------------------------------------------------------

/// 资源提示的 desired / applied 状态。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourcePlacementStatus {
    /// 配置中声明的期望值。
    pub desired: ResourcePlacement,
    /// 实际应用结果。
    pub applied: ResourceApplied,
}

/// 实际应用结果。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceApplied {
    /// CPU affinity 是否成功应用。None 表示未配置。
    pub cpu_affinity: Option<ApplyResult>,
    /// nice 是否成功应用。None 表示未配置。
    pub nice: Option<ApplyResult>,
    /// RT policy 是否成功应用。None 表示未配置。
    pub rt_policy: Option<ApplyResult>,
}

/// 单项应用结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyResult {
    pub success: bool,
    pub message: String,
}

// ---------------------------------------------------------------------------
// 应用入口
// ---------------------------------------------------------------------------

/// 应用资源提示到当前进程。
///
/// 返回每项操作的详细结果。
pub fn apply(placement: &ResourcePlacement) -> ResourceApplied {
    apply_to_pid(placement, None)
}

/// 应用资源提示到指定 PID 的进程。
///
/// `pid` 为 `None` 时表示当前进程。
/// 返回每项操作的详细结果。
pub fn apply_to_pid(placement: &ResourcePlacement, pid: Option<u32>) -> ResourceApplied {
    ResourceApplied {
        cpu_affinity: if placement.cpu_affinity.is_empty() {
            None
        } else {
            Some(apply_cpu_affinity(&placement.cpu_affinity, pid))
        },
        nice: placement.nice.map(|n| apply_nice(n, pid)),
        rt_policy: placement
            .rt_policy
            .map(|policy| apply_rt_policy(policy, placement.rt_priority.unwrap_or(1), pid)),
    }
}

// ---------------------------------------------------------------------------
// CPU affinity
// ---------------------------------------------------------------------------

/// 设置进程的 CPU affinity。`pid` 为 None 时表示当前进程。
fn apply_cpu_affinity(cpus: &[u32], pid: Option<u32>) -> ApplyResult {
    #[cfg(target_os = "linux")]
    {
        apply_cpu_affinity_linux(cpus, pid)
    }
    #[cfg(not(target_os = "linux"))]
    {
        ApplyResult {
            success: false,
            message: format!("CPU affinity 不支持当前平台 (仅支持 Linux); 请求的核心: {cpus:?}"),
        }
    }
}

#[cfg(target_os = "linux")]
fn apply_cpu_affinity_linux(cpus: &[u32], pid: Option<u32>) -> ApplyResult {
    unsafe {
        let mut cpu_set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_ZERO(&mut cpu_set);
        for &cpu in cpus {
            if cpu >= libc::CPU_SETSIZE as u32 {
                return ApplyResult {
                    success: false,
                    message: format!(
                        "CPU 核心 {cpu} 超出 CPU_SETSIZE 上限 ({})",
                        libc::CPU_SETSIZE
                    ),
                };
            }
            libc::CPU_SET(cpu as usize, &mut cpu_set);
        }
        let target_pid = pid.map(|p| p as libc::pid_t).unwrap_or(0);
        let ret =
            libc::sched_setaffinity(target_pid, std::mem::size_of::<libc::cpu_set_t>(), &cpu_set);
        if ret == 0 {
            ApplyResult {
                success: true,
                message: format!("已绑定到 CPU 核心 {cpus:?}"),
            }
        } else {
            let err = std::io::Error::last_os_error();
            ApplyResult {
                success: false,
                message: format!("sched_setaffinity 失败: {err}"),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// nice
// ---------------------------------------------------------------------------

/// 设置进程的 nice 值。`pid` 为 None 时表示当前进程。
fn apply_nice(nice_value: i32, pid: Option<u32>) -> ApplyResult {
    #[cfg(target_os = "linux")]
    {
        apply_nice_linux(nice_value, pid)
    }
    #[cfg(not(target_os = "linux"))]
    {
        ApplyResult {
            success: false,
            message: format!("nice 不支持当前平台 (仅支持 Linux); 请求值: {nice_value}"),
        }
    }
}

#[cfg(target_os = "linux")]
fn apply_nice_linux(nice_value: i32, pid: Option<u32>) -> ApplyResult {
    // setpriority(PRIO_PROCESS, pid, nice) 可以设置指定进程的 nice 值。
    // 权限不足时返回 -1 并设置 errno = EPERM / EACCES。
    let requested = nice_value.clamp(-20, 19);
    let target_pid = pid.map(|p| p as libc::id_t).unwrap_or(0);
    unsafe {
        *libc::__errno_location() = 0;
        let ret = libc::setpriority(libc::PRIO_PROCESS, target_pid, requested);
        let errno = *libc::__errno_location();
        if ret == -1 && errno != 0 {
            let err = std::io::Error::from_raw_os_error(errno);
            ApplyResult {
                success: false,
                message: format!("setpriority(nice={requested}) 失败: {err}"),
            }
        } else {
            ApplyResult {
                success: true,
                message: format!("已设置 nice={requested}"),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RT policy / priority
// ---------------------------------------------------------------------------

/// 设置进程的 RT 调度策略和优先级。`pid` 为 None 时表示当前进程。
fn apply_rt_policy(policy: RtPolicy, priority: u32, pid: Option<u32>) -> ApplyResult {
    #[cfg(target_os = "linux")]
    {
        apply_rt_policy_linux(policy, priority, pid)
    }
    #[cfg(not(target_os = "linux"))]
    {
        ApplyResult {
            success: false,
            message: format!(
                "RT policy 不支持当前平台 (仅支持 Linux); 请求: {policy:?} prio={priority}"
            ),
        }
    }
}

#[cfg(target_os = "linux")]
fn apply_rt_policy_linux(policy: RtPolicy, priority: u32, pid: Option<u32>) -> ApplyResult {
    let linux_policy = match policy {
        RtPolicy::Fifo => libc::SCHED_FIFO,
        RtPolicy::RoundRobin => libc::SCHED_RR,
    };
    let clamped_priority = priority.clamp(1, 99);
    let target_pid = pid.map(|p| p as libc::pid_t).unwrap_or(0);
    unsafe {
        let mut param: libc::sched_param = std::mem::zeroed();
        param.sched_priority = clamped_priority as libc::c_int;
        let ret = libc::sched_setscheduler(target_pid, linux_policy, &param);
        if ret == 0 {
            ApplyResult {
                success: true,
                message: format!("已设置 RT policy={:?} priority={clamped_priority}", policy),
            }
        } else {
            let err = std::io::Error::last_os_error();
            ApplyResult {
                success: false,
                message: format!(
                    "sched_setscheduler({:?}, {clamped_priority}) 失败: {err} \
                     (通常需要 CAP_SYS_NICE 或 root 权限)",
                    policy
                ),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 诊断辅助
// ---------------------------------------------------------------------------

/// 检查资源提示是否有任何配置。
pub fn has_placement(placement: &ResourcePlacement) -> bool {
    !placement.cpu_affinity.is_empty() || placement.nice.is_some() || placement.rt_policy.is_some()
}

/// 生成资源提示的摘要文本（用于日志）。
pub fn summarize(placement: &ResourcePlacement) -> String {
    let mut parts = Vec::new();
    if !placement.cpu_affinity.is_empty() {
        parts.push(format!("cpu_affinity={:?}", placement.cpu_affinity));
    }
    if let Some(nice) = placement.nice {
        parts.push(format!("nice={nice}"));
    }
    if let Some(policy) = placement.rt_policy {
        parts.push(format!(
            "rt_policy={:?} rt_priority={}",
            policy,
            placement.rt_priority.unwrap_or(1)
        ));
    }
    if parts.is_empty() {
        "无".to_string()
    } else {
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_resource_placement_is_empty() {
        let placement = ResourcePlacement::default();
        assert!(placement.cpu_affinity.is_empty());
        assert!(placement.nice.is_none());
        assert!(placement.rt_policy.is_none());
        assert!(placement.rt_priority.is_none());
        assert!(!has_placement(&placement));
    }

    #[test]
    fn has_placement_detects_cpu_affinity() {
        let placement = ResourcePlacement {
            cpu_affinity: vec![0, 1],
            ..Default::default()
        };
        assert!(has_placement(&placement));
    }

    #[test]
    fn has_placement_detects_nice() {
        let placement = ResourcePlacement {
            nice: Some(-5),
            ..Default::default()
        };
        assert!(has_placement(&placement));
    }

    #[test]
    fn has_placement_detects_rt_policy() {
        let placement = ResourcePlacement {
            rt_policy: Some(RtPolicy::Fifo),
            rt_priority: Some(50),
            ..Default::default()
        };
        assert!(has_placement(&placement));
    }

    #[test]
    fn summarize_empty_placement() {
        let placement = ResourcePlacement::default();
        assert_eq!(summarize(&placement), "无");
    }

    #[test]
    fn summarize_full_placement() {
        let placement = ResourcePlacement {
            cpu_affinity: vec![0, 2],
            nice: Some(-10),
            rt_policy: Some(RtPolicy::Fifo),
            rt_priority: Some(80),
        };
        let summary = summarize(&placement);
        assert!(summary.contains("cpu_affinity=[0, 2]"));
        assert!(summary.contains("nice=-10"));
        assert!(summary.contains("rt_policy=Fifo"));
        assert!(summary.contains("rt_priority=80"));
    }

    #[test]
    fn summarize_nice_only() {
        let placement = ResourcePlacement {
            nice: Some(5),
            ..Default::default()
        };
        assert_eq!(summarize(&placement), "nice=5");
    }

    #[test]
    fn deserialize_resource_placement_from_json() {
        let json = r#"{
            "cpu_affinity": [0, 1, 2],
            "nice": -5,
            "rt_policy": "fifo",
            "rt_priority": 50
        }"#;
        let placement: ResourcePlacement = serde_json::from_str(json).unwrap();
        assert_eq!(placement.cpu_affinity, vec![0, 1, 2]);
        assert_eq!(placement.nice, Some(-5));
        assert_eq!(placement.rt_policy, Some(RtPolicy::Fifo));
        assert_eq!(placement.rt_priority, Some(50));
    }

    #[test]
    fn deserialize_resource_placement_partial() {
        let json = r#"{"nice": 10}"#;
        let placement: ResourcePlacement = serde_json::from_str(json).unwrap();
        assert!(placement.cpu_affinity.is_empty());
        assert_eq!(placement.nice, Some(10));
        assert!(placement.rt_policy.is_none());
    }

    #[test]
    fn deserialize_resource_placement_empty() {
        let json = "{}";
        let placement: ResourcePlacement = serde_json::from_str(json).unwrap();
        assert!(!has_placement(&placement));
    }

    #[test]
    fn deserialize_rt_policy_round_robin() {
        let json = r#"{"rt_policy": "round_robin"}"#;
        let placement: ResourcePlacement = serde_json::from_str(json).unwrap();
        assert_eq!(placement.rt_policy, Some(RtPolicy::RoundRobin));
    }

    #[cfg(target_os = "linux")]
    mod linux_tests {
        use super::*;

        #[test]
        fn apply_cpu_affinity_rejects_out_of_range_core() {
            let result = apply_cpu_affinity(&[libc::CPU_SETSIZE as u32], None);
            assert!(!result.success);
            assert!(result.message.contains("CPU_SETSIZE"));
        }

        #[test]
        fn apply_rt_policy_fails_without_privilege() {
            // 普通用户通常没有权限设置 RT 策略
            let result = apply_rt_policy(RtPolicy::Fifo, 50, None);
            // 不检查 success，因为以 root 运行时会成功
            // 但消息应包含有意义的信息
            assert!(
                !result.message.is_empty(),
                "result message should not be empty"
            );
        }

        #[test]
        fn apply_empty_placement_has_no_side_effects() {
            let applied = apply(&ResourcePlacement::default());
            assert!(applied.cpu_affinity.is_none());
            assert!(applied.nice.is_none());
            assert!(applied.rt_policy.is_none());
        }

        #[test]
        fn apply_to_pid_targets_child_process() {
            let cpu = first_allowed_cpu().unwrap_or(0);
            let mut child = std::process::Command::new("sleep")
                .arg("1")
                .spawn()
                .expect("sleep child should spawn");

            let result = apply_cpu_affinity(&[cpu], Some(child.id()));
            let _ = child.kill();
            let _ = child.wait();

            assert!(
                result.success,
                "apply_cpu_affinity to child failed: {}",
                result.message
            );
        }

        fn first_allowed_cpu() -> Option<u32> {
            unsafe {
                let mut cpu_set: libc::cpu_set_t = std::mem::zeroed();
                libc::CPU_ZERO(&mut cpu_set);
                let ret = libc::sched_getaffinity(
                    0,
                    std::mem::size_of::<libc::cpu_set_t>(),
                    &mut cpu_set,
                );
                if ret != 0 {
                    return None;
                }
                (0..libc::CPU_SETSIZE as usize)
                    .find(|&cpu| libc::CPU_ISSET(cpu, &cpu_set))
                    .map(|cpu| cpu as u32)
            }
        }
    }
}
