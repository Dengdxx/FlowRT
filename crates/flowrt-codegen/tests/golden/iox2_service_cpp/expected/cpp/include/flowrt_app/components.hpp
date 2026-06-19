// FlowRT 管理产物。不要手工修改。
#pragma once

#include <cstdint>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <utility>

#include <flowrt/runtime.hpp>
#include <flowrt/inproc_service.hpp>
#include <flowrt/operation.hpp>
#include <flowrt/service.hpp>
#include <flowrt/iox2.hpp>

#include "flowrt_app/messages.hpp"

namespace flowrt_app {

/**
 * @brief `plan_client.plan` service client（iox2 backend）。
 *
 * `slot_` 在所属进程启动时由 runtime shell 经 `bind()` 填充 `Iox2ServiceClient`；
 * 其它进程不填充，调用返回 `ServiceError::Unavailable`。handle 经 shared_ptr 共享 slot，可拷贝传入回调。
 */
class ServiceClient_planner_plan {
public:
    ServiceClient_planner_plan() : slot_(std::make_shared<Slot>()) {}

    /** @brief 由所属进程 runtime shell 填充 transport client。 */
    void bind(flowrt::iox2::Iox2ServiceClient<PlanRequest, PlanResponse> client) {
        if (slot_) {
            slot_->client.emplace(std::move(client));
        }
    }

    flowrt::ServiceResult<PlanResponse> call(const PlanRequest& request, std::uint64_t timeout_ms = 1000) {
        if (!slot_ || !slot_->client.has_value()) {
            return flowrt::ServiceResult<PlanResponse>::err(flowrt::ServiceError::Unavailable);
        }
        return slot_->client->call(request, timeout_ms);
    }

    flowrt::InprocServiceHandle<PlanResponse> start_call(const PlanRequest& request, std::uint64_t timeout_ms = 1000) {
        if (!slot_ || !slot_->client.has_value()) {
            return flowrt::InprocServiceHandle<PlanResponse>::ready_error(flowrt::ServiceError::Unavailable);
        }
        return flowrt::InprocServiceHandle<PlanResponse>::ready(slot_->client->call(request, timeout_ms));
    }

private:
    struct Slot {
        std::optional<flowrt::iox2::Iox2ServiceClient<PlanRequest, PlanResponse>> client;
    };
    std::shared_ptr<Slot> slot_;
};

/**
 * @brief `plan_service` 组件的 C++ 用户实现接口。
 *
 * 用户代码实现该接口并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。
 */
class PlanServiceInterface {
public:
    virtual ~PlanServiceInterface() = default;
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
* @brief 处理 `plan` service request。
*
* runtime shell 在 hidden service task 中调用该方法。用户业务逻辑
* 实现具体的 request -> response 转换。
*
* @param request 请求消息引用。
* @return 成功返回 `ServiceResult::ok(response)`，业务错误返回
*         `ServiceResult::err(error_code, message)`。
*/
    virtual flowrt::ServiceResult<PlanResponse> on_plan_request(const PlanRequest& request) {
(void)request;
return flowrt::ServiceResult<PlanResponse>::err(flowrt::ServiceError::HandlerError);
}

    /**
     * @brief 执行一次 `plan_service` 组件调度回调。
     *
     * runtime shell 按 Contract IR 中的 task 和 dataflow 顺序调用该方法。输入使用 latest snapshot 视图，输出通过 `flowrt::Output<T>` 写入，本方法不得保存输入视图内部指针到回调之外。
     * @return 本次回调的 FlowRT 执行状态。
     */
    virtual flowrt::Status on_tick() = 0;
};

/**
 * @brief `planner` 组件的 C++ 用户实现接口。
 *
 * 用户代码实现该接口并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。
 */
class PlannerInterface {
public:
    virtual ~PlannerInterface() = default;
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
     * @brief 执行一次 `planner` 组件调度回调。
     *
     * runtime shell 按 Contract IR 中的 task 和 dataflow 顺序调用该方法。输入使用 latest snapshot 视图，输出通过 `flowrt::Output<T>` 写入，本方法不得保存输入视图内部指针到回调之外。
     * @param plan typed service client handle。
     * @return 本次回调的 FlowRT 执行状态。
     */
    virtual flowrt::Status on_tick(
        ServiceClient_planner_plan& plan) = 0;
};

}  // namespace flowrt_app
