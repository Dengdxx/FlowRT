// FlowRT 管理产物。不要手工修改。

use crate::messages::*;

/// `consumer` 组件的 Rust 用户实现 trait。
///
/// 用户代码实现该 trait 并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。
pub trait Consumer: Send {
    /// 组件初始化钩子。
    ///
    /// `context` 是 runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
    /// 返回本次生命周期步骤的 FlowRT 执行状态。
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

    /// 执行一次 `consumer` 组件调度回调。
    ///
    /// runtime shell 按 Contract IR 中的 task 和 dataflow 顺序调用该方法。输入使用 latest snapshot 视图，输出通过 `flowrt::Output<T>` 写入，本方法不得保存输入引用到回调之外。
    ///
    /// - `sample`: latest snapshot 输入视图。
    /// - `echo`: 输出端口写入句柄。
    /// 返回本次回调的 FlowRT 执行状态。
    fn on_tick(
        &mut self,
        sample: flowrt::Latest<'_, ImuSample>,
        echo: &mut flowrt::Output<ImuSample>,
    ) -> flowrt::Status;
}

