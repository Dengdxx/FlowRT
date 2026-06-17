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
#include <flowrt/synchronizer.hpp>

#include "flowrt_app/messages.hpp"

namespace flowrt_app {

/**
 * @brief `fusion` 组件的 C++ 用户实现接口。
 *
 * 用户代码实现该接口并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。
 */
class FusionInterface {
public:
    virtual ~FusionInterface() = default;
    /**
     * @brief 组件初始化钩子。
     *
     * @param context runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
     * @return 本次生命周期步骤的 FlowRT 执行状态。
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
     * @brief 执行一次 `fusion` 组件调度回调。
     *
     * runtime shell 按 Contract IR 中的 task 和 dataflow 顺序调用该方法。输入使用 latest snapshot 视图，输出通过 `flowrt::Output<T>` 写入，本方法不得保存输入视图内部指针到回调之外。
     *
     * @param imu latest snapshot 输入视图。
     * @param odom latest snapshot 输入视图。
     * @param estimate 输出端口写入句柄。
     * @return 本次回调的 FlowRT 执行状态。
     */
    virtual flowrt::Status on_tick(
        const flowrt::Latest<Imu>& imu,
        const flowrt::Latest<Odom>& odom,
        flowrt::Output<Estimate>& estimate) = 0;
};

/**
 * @brief `imu_src` 组件的 C++ 用户实现接口。
 *
 * 用户代码实现该接口并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。
 */
class ImuSrcInterface {
public:
    virtual ~ImuSrcInterface() = default;
    /**
     * @brief 组件初始化钩子。
     *
     * @param context runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
     * @return 本次生命周期步骤的 FlowRT 执行状态。
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
     * @brief 执行一次 `imu_src` 组件调度回调。
     *
     * runtime shell 按 Contract IR 中的 task 和 dataflow 顺序调用该方法。输入使用 latest snapshot 视图，输出通过 `flowrt::Output<T>` 写入，本方法不得保存输入视图内部指针到回调之外。
     *
     * @param imu 输出端口写入句柄。
     * @return 本次回调的 FlowRT 执行状态。
     */
    virtual flowrt::Status on_tick(
        flowrt::Output<Imu>& imu) = 0;
};

/**
 * @brief `odom_src` 组件的 C++ 用户实现接口。
 *
 * 用户代码实现该接口并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。
 */
class OdomSrcInterface {
public:
    virtual ~OdomSrcInterface() = default;
    /**
     * @brief 组件初始化钩子。
     *
     * @param context runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。
     * @return 本次生命周期步骤的 FlowRT 执行状态。
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
     * @brief 执行一次 `odom_src` 组件调度回调。
     *
     * runtime shell 按 Contract IR 中的 task 和 dataflow 顺序调用该方法。输入使用 latest snapshot 视图，输出通过 `flowrt::Output<T>` 写入，本方法不得保存输入视图内部指针到回调之外。
     *
     * @param odom 输出端口写入句柄。
     * @return 本次回调的 FlowRT 执行状态。
     */
    virtual flowrt::Status on_tick(
        flowrt::Output<Odom>& odom) = 0;
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
     * @param estimate latest snapshot 输入视图。
     * @return 本次回调的 FlowRT 执行状态。
     */
    virtual flowrt::Status on_tick(
        const flowrt::Latest<Estimate>& estimate) = 0;
};

}  // namespace flowrt_app
