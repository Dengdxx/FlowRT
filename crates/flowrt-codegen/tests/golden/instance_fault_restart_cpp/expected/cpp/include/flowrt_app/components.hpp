// FlowRT 管理产物。不要手工修改。
#pragma once

#include <cstdint>
#include <map>
#include <optional>
#include <string>
#include <utility>

#include <flowrt/runtime.hpp>
#include <flowrt/inproc_service.hpp>
#include <flowrt/operation.hpp>
#include <flowrt/service.hpp>

#include "flowrt_app/messages.hpp"

namespace flowrt_app {

/**
 * @brief `producer` 组件的 C++ 用户实现接口。
 *
 * 用户代码实现该接口并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。
 */
class ProducerInterface {
public:
    virtual ~ProducerInterface() = default;
    /**
     * @brief 组件初始化钩子。
     *
     * @param context runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
     * @return 本次生命周期步骤的 FlowRT 执行状态。
     *
     * @note `restart` 故障策略会在同一对象上重新调用本钩子，实现必须可重入：不得依赖仅首次成立的前置状态。
     */
    virtual flowrt::Status on_init(flowrt::Context& context) {
        (void)context;
        return flowrt::ok();
    }
    /**
     * @brief 组件启动钩子。
     *
     * @param context runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
     * @return 本次生命周期步骤的 FlowRT 执行状态。
     */
    virtual flowrt::Status on_start(flowrt::Context& context) {
        (void)context;
        return flowrt::ok();
    }
    /**
     * @brief 组件停止钩子。
     *
     * @param context runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
     * @return 本次生命周期步骤的 FlowRT 执行状态。
     */
    virtual flowrt::Status on_stop(flowrt::Context& context) {
        (void)context;
        return flowrt::ok();
    }
    /**
     * @brief 组件关闭钩子。
     *
     * @param context runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
     * @return 本次生命周期步骤的 FlowRT 执行状态。
     */
    virtual flowrt::Status on_shutdown(flowrt::Context& context) {
        (void)context;
        return flowrt::ok();
    }
    /**
     * @brief 执行一次 `producer` 组件调度回调。
     *
     * runtime shell 按 Contract IR 中的 task 和 dataflow 顺序调用该方法。输入使用 latest snapshot 视图，输出通过 `flowrt::Output<T>` 写入，本方法不得保存输入视图内部指针到回调之外。
     *
     * @param sample 输出端口写入句柄。
     * @return 本次回调的 FlowRT 执行状态。
     */
    virtual flowrt::Status on_tick(
        flowrt::Output<Sample>& sample) = 0;
};

/**
 * @brief `sink` 组件的 C++ 用户实现接口。
 *
 * 用户代码实现该接口并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。
 */
class SinkInterface {
public:
    virtual ~SinkInterface() = default;
    /**
     * @brief 组件初始化钩子。
     *
     * @param context runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
     * @return 本次生命周期步骤的 FlowRT 执行状态。
     *
     * @note `restart` 故障策略会在同一对象上重新调用本钩子，实现必须可重入：不得依赖仅首次成立的前置状态。
     */
    virtual flowrt::Status on_init(flowrt::Context& context) {
        (void)context;
        return flowrt::ok();
    }
    /**
     * @brief 组件启动钩子。
     *
     * @param context runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
     * @return 本次生命周期步骤的 FlowRT 执行状态。
     */
    virtual flowrt::Status on_start(flowrt::Context& context) {
        (void)context;
        return flowrt::ok();
    }
    /**
     * @brief 组件停止钩子。
     *
     * @param context runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
     * @return 本次生命周期步骤的 FlowRT 执行状态。
     */
    virtual flowrt::Status on_stop(flowrt::Context& context) {
        (void)context;
        return flowrt::ok();
    }
    /**
     * @brief 组件关闭钩子。
     *
     * @param context runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
     * @return 本次生命周期步骤的 FlowRT 执行状态。
     */
    virtual flowrt::Status on_shutdown(flowrt::Context& context) {
        (void)context;
        return flowrt::ok();
    }
    /**
     * @brief 执行一次 `sink` 组件调度回调。
     *
     * runtime shell 按 Contract IR 中的 task 和 dataflow 顺序调用该方法。输入使用 latest snapshot 视图，输出通过 `flowrt::Output<T>` 写入，本方法不得保存输入视图内部指针到回调之外。
     *
     * @param sample latest snapshot 输入视图。
     * @return 本次回调的 FlowRT 执行状态。
     */
    virtual flowrt::Status on_tick(
        const flowrt::Latest<Sample>& sample) = 0;
};

}  // namespace flowrt_app
