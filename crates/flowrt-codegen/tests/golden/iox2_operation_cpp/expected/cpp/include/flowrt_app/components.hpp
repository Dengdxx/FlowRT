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
 * @brief `controller.plan` Operation client（iox2 backend）。
 *
 * `slot_` 在所属进程启动时由 runtime shell 经 `bind()` 填充内部 iox2 service clients；
 * 用户代码仍只看到 Operation start/cancel/status API。
 */
class OperationClient_controller_plan {
public:
    OperationClient_controller_plan() : slot_(std::make_shared<Slot>()) {}

    void bind(
        std::shared_ptr<flowrt::iox2::Iox2ServiceClient<flowrt::OperationStartRequest<PlanGoal>, flowrt::OperationStartAck>> start_client,
        std::shared_ptr<flowrt::iox2::Iox2ServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>> cancel_client,
        std::shared_ptr<flowrt::iox2::Iox2ServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>> status_client) {
        if (slot_) {
            slot_->start_client = std::move(start_client);
            slot_->cancel_client = std::move(cancel_client);
            slot_->status_client = std::move(status_client);
        }
    }

    flowrt::OperationClientResult<flowrt::OperationStartAck> start(const PlanGoal& goal, std::uint64_t timeout_ms = 5000) {
        if (!slot_ || !slot_->start_client) {
            return flowrt::OperationClientResult<flowrt::OperationStartAck>::err(flowrt::OperationClientError::Unavailable);
        }
        const auto owner = flowrt::OperationOwner{.scope_key = flowrt::fnv1a64("controller.plan"), .owner_key = flowrt::fnv1a64("controller.plan")};
        const auto request = flowrt::OperationStartRequest<PlanGoal>{.goal = goal, .owner = owner, .timeout = std::chrono::milliseconds{static_cast<std::chrono::milliseconds::rep>(timeout_ms)}};
        return flowrt::operation_client_result_from_service(slot_->start_client->call(request, timeout_ms));
    }

    flowrt::OperationClientResult<flowrt::OperationStatusSnapshot> cancel(flowrt::OperationId id, std::uint64_t timeout_ms = 5000) {
        if (!slot_ || !slot_->cancel_client) {
            return flowrt::OperationClientResult<flowrt::OperationStatusSnapshot>::err(flowrt::OperationClientError::Unavailable);
        }
        return flowrt::operation_client_result_from_service(slot_->cancel_client->call(id, timeout_ms));
    }

    flowrt::OperationClientResult<flowrt::OperationStatusSnapshot> status(flowrt::OperationId id, std::uint64_t timeout_ms = 5000) {
        if (!slot_ || !slot_->status_client) {
            return flowrt::OperationClientResult<flowrt::OperationStatusSnapshot>::err(flowrt::OperationClientError::Unavailable);
        }
        return flowrt::operation_client_result_from_service(slot_->status_client->call(id, timeout_ms));
    }

private:
    struct Slot {
        std::shared_ptr<flowrt::iox2::Iox2ServiceClient<flowrt::OperationStartRequest<PlanGoal>, flowrt::OperationStartAck>> start_client;
        std::shared_ptr<flowrt::iox2::Iox2ServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>> cancel_client;
        std::shared_ptr<flowrt::iox2::Iox2ServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>> status_client;
    };
    std::shared_ptr<Slot> slot_;
};

/**
 * @brief `controller` 组件的 C++ 用户实现接口。
 *
 * 用户代码实现该接口并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。
 */
class ControllerInterface {
public:
    virtual ~ControllerInterface() = default;
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
     * @brief 执行一次 `controller` 组件调度回调。
     *
     * runtime shell 按 Contract IR 中的 task 和 dataflow 顺序调用该方法。输入使用 latest snapshot 视图，输出通过 `flowrt::Output<T>` 写入，本方法不得保存输入视图内部指针到回调之外。
     * @param plan typed Operation client handle。
     * @return 本次回调的 FlowRT 执行状态。
     */
    virtual flowrt::Status on_tick(
        OperationClient_controller_plan& plan) = 0;
};

/**
 * @brief `navigator` 组件的 C++ 用户实现接口。
 *
 * 用户代码实现该接口并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。
 */
class NavigatorInterface {
public:
    virtual ~NavigatorInterface() = default;
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
     * @brief 处理 `plan` Operation goal。
     */
    virtual flowrt::OperationHandlerResult<PlanResult> on_plan_operation(
        const PlanGoal& goal,
        flowrt::OperationCancelToken cancel,
        flowrt::OperationProgressPublisher<PlanFeedback>& progress) {
        (void)goal;
        (void)cancel;
        (void)progress;
        return flowrt::OperationHandlerResult<PlanResult>::failed();
    }

    /**
     * @brief 执行一次 `navigator` 组件调度回调。
     *
     * runtime shell 按 Contract IR 中的 task 和 dataflow 顺序调用该方法。输入使用 latest snapshot 视图，输出通过 `flowrt::Output<T>` 写入，本方法不得保存输入视图内部指针到回调之外。
     * @return 本次回调的 FlowRT 执行状态。
     */
    virtual flowrt::Status on_tick() = 0;
};

}  // namespace flowrt_app
