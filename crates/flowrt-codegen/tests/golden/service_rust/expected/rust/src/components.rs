// FlowRT 管理产物。不要手工修改。

use crate::messages::*;

// ── Service client typed handles ───────────────────────────────────

/// `plan_client.plan` service client typed handle。
///
/// 封装 `flowrt::InprocServiceClient`，提供同步 `call()` 和
/// 非阻塞 `start_call()` 调用路径。
#[allow(non_camel_case_types)]
#[derive(Clone)]
pub struct ServiceClient_planner_plan {
    pub(crate) inner: flowrt::InprocServiceClient<PlanRequest, PlanResponse>,
}

impl ServiceClient_planner_plan {
    /// 发起同步阻塞 service 调用。
///
/// 超时返回 `ServiceError::Timeout`，服务不可用返回 `Unavailable`，
/// 队列满返回 `Busy`。
pub fn call(
&self,
request: PlanRequest,
timeout: std::time::Duration,
) -> flowrt::ServiceResult<PlanResponse> {
self.inner.call(request, timeout)
}

    /// 发起非阻塞 service 调用，返回 `ServiceCallHandle`。
///
/// handle 支持 `poll()` 查询就绪状态和 `complete()` 阻塞等待结果。
pub fn start_call(
&self,
request: PlanRequest,
timeout: std::time::Duration,
) -> flowrt::ServiceCallHandle<PlanResponse> {
self.inner.start_call(request, timeout)
}

}

/// `plan_service` 组件的 Rust 用户实现 trait。
///
/// 用户代码实现该 trait 并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。
pub trait PlanService: Send {
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

    /// 处理 `plan` service request。
///
/// runtime shell 在 hidden service task 中调用该方法。用户业务逻辑
/// 实现具体的 request -> response 转换。
///
/// 返回 `flowrt::ServiceResult::Ok(response)` 表示成功，
/// `flowrt::ServiceResult::Err(error, message)` 表示业务错误。
    fn on_plan_request(
&mut self,
_request: &PlanRequest,
) -> flowrt::ServiceResult<PlanResponse> {
flowrt::ServiceResult::err(flowrt::ServiceError::HandlerError)
}

    /// 执行一次 `plan_service` 组件调度回调。
    ///
    /// runtime shell 按 Contract IR 中的 task 和 dataflow 顺序调用该方法。输入使用 latest snapshot 视图，输出通过 `flowrt::Output<T>` 写入，本方法不得保存输入引用到回调之外。
    /// 返回本次回调的 FlowRT 执行状态。
    fn on_tick(&mut self) -> flowrt::Status;
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

