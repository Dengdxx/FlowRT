// FlowRT 管理产物。不要手工修改。

use crate::messages::*;

// ── Service client typed handles ───────────────────────────────────

/// `plan_client.plan` service client typed handle（zenoh backend）。
///
/// `inner` 在所属进程启动时由 runtime shell 用 `ZenohServiceClient` 填充；
/// 其它进程不会填充，调用返回 `ServiceError::Unavailable`。
#[allow(non_camel_case_types)]
#[derive(Clone)]
pub struct ServiceClient_planner_plan {
    pub(crate) inner: std::sync::Arc<std::sync::OnceLock<flowrt::zenoh::ZenohServiceClient<PlanRequest, PlanResponse>>>,
}

impl ServiceClient_planner_plan {
    /// 发起同步阻塞 zenoh service 调用。
///
/// 所属进程未填充 transport client 时返回 `ServiceError::Unavailable`。
pub fn call(
&self,
request: PlanRequest,
timeout: std::time::Duration,
) -> flowrt::ServiceResult<PlanResponse> {
match self.inner.get() {
Some(client) => client.call(request, timeout.as_millis().min(u64::MAX as u128) as u64),
None => flowrt::ServiceResult::err(flowrt::ServiceError::Unavailable),
}
}

    /// 发起 zenoh service 调用并返回就绪 `ServiceCallHandle`。
///
/// v1 实现先同步完成 query 再包装结果；transport client 未填充时返回就绪错误。
pub fn start_call(
&self,
request: PlanRequest,
timeout: std::time::Duration,
) -> flowrt::ServiceCallHandle<PlanResponse> {
match self.inner.get() {
Some(client) => flowrt::ServiceCallHandle::ready(client.call(request, timeout.as_millis().min(u64::MAX as u128) as u64)),
None => flowrt::ServiceCallHandle::ready_error(flowrt::ServiceError::Unavailable),
}
}

}

/// `plan_service` 组件的 Rust 用户实现 trait。
///
/// 用户代码实现该 trait 并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。
pub trait PlanService: Send + Sync {
    /// 组件初始化钩子。
    ///
    /// `context` 是 runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
    /// 返回本次生命周期步骤的 FlowRT 执行状态。
    ///
    /// `restart` 故障策略会在同一对象上重新调用本钩子，实现必须可重入：不得依赖仅首次成立的前置状态。
    fn on_init(&self, _context: &mut flowrt::Context) -> flowrt::Status {
        flowrt::Status::ok()
    }

    /// 组件启动钩子。
    ///
    /// `context` 是 runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
    /// 返回本次生命周期步骤的 FlowRT 执行状态。
    fn on_start(&self, _context: &mut flowrt::Context) -> flowrt::Status {
        flowrt::Status::ok()
    }

    /// 组件停止钩子。
    ///
    /// `context` 是 runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
    /// 返回本次生命周期步骤的 FlowRT 执行状态。
    fn on_stop(&self, _context: &mut flowrt::Context) -> flowrt::Status {
        flowrt::Status::ok()
    }

    /// 组件关闭钩子。
    ///
    /// `context` 是 runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
    /// 返回本次生命周期步骤的 FlowRT 执行状态。
    fn on_shutdown(&self, _context: &mut flowrt::Context) -> flowrt::Status {
        flowrt::Status::ok()
    }

    /// 处理 `plan` service request。
///
/// runtime shell 在 hidden service task 中调用该方法。用户业务逻辑
/// 实现具体的 request -> response 转换。
///
/// 返回 `flowrt::ServiceResult::Ok(response)` 表示成功，
/// `flowrt::ServiceResult::Err(error, message)` 表示业务错误。
    fn on_plan_request(
&self,
_request: &PlanRequest,
) -> flowrt::ServiceResult<PlanResponse> {
flowrt::ServiceResult::err(flowrt::ServiceError::HandlerError)
}

    /// 执行一次 `plan_service` 组件调度回调。
    ///
    /// runtime shell 按 Contract IR 中的 task 和 dataflow 顺序调用该方法。输入使用 latest snapshot 视图，输出通过 `flowrt::Output<T>` 写入，本方法不得保存输入引用到回调之外。
    /// 返回本次回调的 FlowRT 执行状态。
    fn on_tick(&self) -> flowrt::Status;
}

/// `planner` 组件的 Rust 用户实现 trait。
///
/// 用户代码实现该 trait 并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。
pub trait Planner: Send {
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

    /// 执行一次 `planner` 组件调度回调。
    ///
    /// runtime shell 按 Contract IR 中的 task 和 dataflow 顺序调用该方法。输入使用 latest snapshot 视图，输出通过 `flowrt::Output<T>` 写入，本方法不得保存输入引用到回调之外。
    /// - `plan`: typed service client handle。
    /// 返回本次回调的 FlowRT 执行状态。
    fn on_tick(
        &mut self,
        plan: &ServiceClient_planner_plan,
    ) -> flowrt::Status;
}
