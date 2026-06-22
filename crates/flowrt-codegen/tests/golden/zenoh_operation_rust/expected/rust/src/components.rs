// FlowRT 管理产物。不要手工修改。

use crate::messages::*;

// ── Operation client typed handles ────────────────────────────────

fn flowrt_operation_result<T>(result: flowrt::ServiceResult<T>) -> Result<T, flowrt::OperationClientError> {
    match result {
        flowrt::ServiceResult::Ok(value) => Ok(value),
        flowrt::ServiceResult::Err(error, _) => Err(flowrt::OperationClientError::from_service_error(error)),
    }
}

pub(crate) fn flowrt_operation_id_string(id: flowrt::OperationId) -> String {
    format!("{}:{}:{}", id.operation_key, id.client_id, id.sequence)
}

pub(crate) fn flowrt_operation_id_from_string(value: &str) -> Result<flowrt::OperationId, String> {
    let mut parts = value.split(':');
    let operation_key = parts.next().ok_or_else(|| format!("invalid operation id `{value}`"))?.parse::<u64>().map_err(|_| format!("invalid operation id `{value}`"))?;
    let client_id = parts.next().ok_or_else(|| format!("invalid operation id `{value}`"))?.parse::<u64>().map_err(|_| format!("invalid operation id `{value}`"))?;
    let sequence = parts.next().ok_or_else(|| format!("invalid operation id `{value}`"))?.parse::<u64>().map_err(|_| format!("invalid operation id `{value}`"))?;
    if parts.next().is_some() {
        return Err(format!("invalid operation id `{value}`"));
    }
    Ok(flowrt::OperationId::new(operation_key, client_id, sequence))
}

pub(crate) fn flowrt_operation_status_from_snapshot(name: &str, owner: &str, snapshot: flowrt::OperationStatusSnapshot) -> flowrt::IntrospectionOperationStatus {
    let active = !snapshot.state.is_terminal() && snapshot.state != flowrt::OperationState::Idle;
    flowrt::IntrospectionOperationStatus {
        name: name.to_string(),
        ready: true,
        running: if active { 1 } else { 0 },
        queued: 0,
        current_operation_ids: if active { vec![flowrt_operation_id_string(snapshot.id)] } else { Vec::new() },
        total_started: snapshot.health.started,
        succeeded_count: snapshot.health.succeeded,
        failed_count: snapshot.health.failed,
        canceled_count: snapshot.health.canceled,
        timeout_count: snapshot.health.timeout,
        preempted_count: snapshot.health.preempted,
        current_state: Some(snapshot.state.as_str().to_string()),
        current_owner: if snapshot.owner.owner_key == 0 { None } else { Some(owner.to_string()) },
        current_deadline_ms: if active { Some(snapshot.deadline_ms) } else { None },
        last_event: Some("flowrt.operation.state_changed".to_string()),
        last_error: None,
        last_transition_ms: Some(flowrt::monotonic_time_ms()),
    }
}

pub(crate) fn flowrt_operation_control_error<T>(error: flowrt::OperationControlError) -> flowrt::ServiceResult<T> {
    let code = match error {
        flowrt::OperationControlError::Busy { .. } | flowrt::OperationControlError::OwnerConflict { .. } => flowrt::ServiceError::Busy,
        flowrt::OperationControlError::StaleInvocation { .. } | flowrt::OperationControlError::AlreadyTerminal { .. } => flowrt::ServiceError::Rejected,
        flowrt::OperationControlError::InvalidPolicy(_) | flowrt::OperationControlError::InvalidTransition { .. } => flowrt::ServiceError::HandlerError,
        flowrt::OperationControlError::Ok => flowrt::ServiceError::HandlerError,
    };
    flowrt::ServiceResult::err_with_message(code, error.to_string())
}

#[allow(non_camel_case_types)]
#[derive(Clone)]
pub struct OperationClient_controller_plan {
    pub(crate) start_client: std::sync::Arc<std::sync::OnceLock<flowrt::zenoh::ZenohServiceClient<flowrt::OperationStartRequest<PlanGoal>, flowrt::OperationStartAck>>>,
pub(crate) cancel_client: std::sync::Arc<std::sync::OnceLock<flowrt::zenoh::ZenohServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>>>,
pub(crate) status_client: std::sync::Arc<std::sync::OnceLock<flowrt::zenoh::ZenohServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>>>,
}

impl OperationClient_controller_plan {
    pub fn start(&self, goal: PlanGoal, timeout: std::time::Duration) -> Result<flowrt::OperationStartAck, flowrt::OperationClientError> {
        let owner = flowrt::OperationOwner::new(flowrt::fnv1a64("controller.plan".as_bytes()), flowrt::fnv1a64("controller.plan".as_bytes()));
        let request = flowrt::OperationStartRequest::new(goal, owner, timeout);
        let timeout_ms = timeout.as_millis().min(u128::from(u64::MAX)) as u64;
        let Some(client) = self.start_client.get() else {
            return Err(flowrt::OperationClientError::Unavailable);
        };
        flowrt_operation_result(client.call(request, timeout_ms))
    }

    pub fn cancel(&self, id: flowrt::OperationId, timeout: std::time::Duration) -> Result<flowrt::OperationStatusSnapshot, flowrt::OperationClientError> {
        let timeout_ms = timeout.as_millis().min(u128::from(u64::MAX)) as u64;
        let Some(client) = self.cancel_client.get() else {
            return Err(flowrt::OperationClientError::Unavailable);
        };
        flowrt_operation_result(client.call(id, timeout_ms))
    }

    pub fn status(&self, id: flowrt::OperationId, timeout: std::time::Duration) -> Result<flowrt::OperationStatusSnapshot, flowrt::OperationClientError> {
        let timeout_ms = timeout.as_millis().min(u128::from(u64::MAX)) as u64;
        let Some(client) = self.status_client.get() else {
            return Err(flowrt::OperationClientError::Unavailable);
        };
        flowrt_operation_result(client.call(id, timeout_ms))
    }
}

/// `controller` 组件的 Rust 用户实现 trait。
///
/// 用户代码实现该 trait 并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。
pub trait Controller: Send {
    /// 组件初始化钩子。
    ///
    /// `context` 是 runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
    /// 返回本次生命周期步骤的 FlowRT 执行状态。
    ///
    /// `restart` 故障策略会在同一对象上重新调用本钩子，实现必须可重入：不得依赖仅首次成立的前置状态。
    fn on_init(&mut self, _context: &mut flowrt::Context) -> flowrt::Status {
        flowrt::Status::ok()
    }

    /// 组件启动钩子。
    ///
    /// `context` 是 runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
    /// 返回本次生命周期步骤的 FlowRT 执行状态。
    fn on_start(&mut self, _context: &mut flowrt::Context) -> flowrt::Status {
        flowrt::Status::ok()
    }

    /// 组件停止钩子。
    ///
    /// `context` 是 runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
    /// 返回本次生命周期步骤的 FlowRT 执行状态。
    fn on_stop(&mut self, _context: &mut flowrt::Context) -> flowrt::Status {
        flowrt::Status::ok()
    }

    /// 组件关闭钩子。
    ///
    /// `context` 是 runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
    /// 返回本次生命周期步骤的 FlowRT 执行状态。
    fn on_shutdown(&mut self, _context: &mut flowrt::Context) -> flowrt::Status {
        flowrt::Status::ok()
    }

    /// 执行一次 `controller` 组件调度回调。
    ///
    /// runtime shell 按 Contract IR 中的 task 和 dataflow 顺序调用该方法。输入使用 latest snapshot 视图，输出通过 `flowrt::Output<T>` 写入，本方法不得保存输入引用到回调之外。
    /// - `plan`: typed Operation client handle。
    /// 返回本次回调的 FlowRT 执行状态。
    fn on_tick(
        &mut self,
        plan: &OperationClient_controller_plan,
    ) -> flowrt::Status;
}

/// `navigator` 组件的 Rust 用户实现 trait。
///
/// 用户代码实现该 trait 并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。
pub trait Navigator: Send {
    /// 组件初始化钩子。
    ///
    /// `context` 是 runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
    /// 返回本次生命周期步骤的 FlowRT 执行状态。
    ///
    /// `restart` 故障策略会在同一对象上重新调用本钩子，实现必须可重入：不得依赖仅首次成立的前置状态。
    fn on_init(&mut self, _context: &mut flowrt::Context) -> flowrt::Status {
        flowrt::Status::ok()
    }

    /// 组件启动钩子。
    ///
    /// `context` 是 runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
    /// 返回本次生命周期步骤的 FlowRT 执行状态。
    fn on_start(&mut self, _context: &mut flowrt::Context) -> flowrt::Status {
        flowrt::Status::ok()
    }

    /// 组件停止钩子。
    ///
    /// `context` 是 runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
    /// 返回本次生命周期步骤的 FlowRT 执行状态。
    fn on_stop(&mut self, _context: &mut flowrt::Context) -> flowrt::Status {
        flowrt::Status::ok()
    }

    /// 组件关闭钩子。
    ///
    /// `context` 是 runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
    /// 返回本次生命周期步骤的 FlowRT 执行状态。
    fn on_shutdown(&mut self, _context: &mut flowrt::Context) -> flowrt::Status {
        flowrt::Status::ok()
    }

    /// 处理 `plan` Operation goal。
///
/// runtime shell 在 hidden operation task 中调用该方法。用户业务逻辑
/// 负责长任务执行，在安全边界检查 cancel token，并通过 progress 发布 typed feedback。
    fn on_plan_operation(
&mut self,
_goal: &PlanGoal,
_cancel: flowrt::OperationCancelToken,
_progress: &mut flowrt::OperationProgressPublisher<PlanFeedback>,
) -> flowrt::OperationHandlerResult<PlanResult> {
flowrt::OperationHandlerResult::failed()
}

    /// 执行一次 `navigator` 组件调度回调。
    ///
    /// runtime shell 按 Contract IR 中的 task 和 dataflow 顺序调用该方法。输入使用 latest snapshot 视图，输出通过 `flowrt::Output<T>` 写入，本方法不得保存输入引用到回调之外。
    /// 返回本次回调的 FlowRT 执行状态。
    fn on_tick(&mut self) -> flowrt::Status;
}
