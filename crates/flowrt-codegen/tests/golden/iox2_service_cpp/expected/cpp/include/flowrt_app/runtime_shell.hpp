// FlowRT 管理产物。不要手工修改。
#pragma once

#include <cstddef>
#include <functional>
#include <map>
#include <memory>
#include <optional>
#include <string_view>
#include <vector>

#include <flowrt/runtime.hpp>
#include <flowrt/inproc_service.hpp>

#include "flowrt_app/components.hpp"
#include "flowrt_app/messages.hpp"

namespace flowrt_app {

class App;
using FlowrtOutputCommit = std::function<flowrt::Status(App&, flowrt::IntrospectionState&, flowrt::ScheduleWaiter&, std::map<std::string, flowrt::IntrospectionTaskHealth>&)>;
using FlowrtTaskOutcome = flowrt::TaskRunOutcome<std::vector<FlowrtOutputCommit>>;

/**
 * @brief Contract IR 驱动的 C++ inproc 应用 shell。
 *
 * `App` 持有用户组件实现和 FlowRT 管理的 channel 状态。用户代码通过 `flowrt_user::build_app()` 构造该对象，runtime shell 负责生命周期、调度和数据流转发。
 */
class App {
public:
    /**
     * @brief 构造 C++ 应用 shell。
     *
     * @param plan_client 用户组件实例所有权；shell 在生命周期内独占持有该对象。
     * @param plan_svc 用户组件实例所有权；shell 在生命周期内独占持有该对象。
     */
    explicit App(
        std::unique_ptr<PlannerInterface> plan_client,
        std::unique_ptr<PlanServiceInterface> plan_svc
    );

    /**
     * @brief 使用指定 backend 运行完整 C++ 应用图。
     *
     * @param backend 提供调度器和 capability 的 FlowRT backend。
     * @param run_ticks 可选的显式 tick 上限；为空表示无限运行。
     * @return 应用执行状态。
     */
    flowrt::Status run(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks);

    /**
     * @brief 运行指定 RSDL process group。
     *
     * @param backend 提供调度器和 capability 的 FlowRT backend。
     * @param process Contract IR 中声明的 process group 名称。
     * @param run_ticks 可选的显式 tick 上限；为空表示无限运行。
     * @return 应用执行状态。
     */
    flowrt::Status run_process(const flowrt::Backend& backend, std::string_view process, std::optional<std::size_t> run_ticks);

private:
    flowrt::Status step(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);
    flowrt::Status step_startup(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);
    flowrt::Status step_shutdown(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);
    FlowrtTaskOutcome step_task_plan_client_main(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);
    FlowrtTaskOutcome step_task_plan_svc_main(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);
    flowrt::Status step_process_client_proc(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);
    flowrt::Status step_process_client_proc_startup(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);
    flowrt::Status step_process_client_proc_shutdown(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);
    FlowrtTaskOutcome step_process_client_proc_task_plan_client_main(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);
    flowrt::Status step_process_server_proc(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);
    flowrt::Status step_process_server_proc_startup(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);
    flowrt::Status step_process_server_proc_shutdown(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);
    FlowrtTaskOutcome step_process_server_proc_task_plan_svc_main(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);
    flowrt::Status run_process_client_proc(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks);
    flowrt::Status run_process_server_proc(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks);

    std::unique_ptr<PlannerInterface> plan_client_;
    std::unique_ptr<PlanServiceInterface> plan_svc_;
    ServiceClient_planner_plan service_client_plan_client_plan_;
    std::optional<flowrt::iox2::Iox2ServiceServer<PlanRequest, PlanResponse>> service_server_plan_svc_plan_;
    flowrt::Status step_service_plan_svc_plan(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);
};

/**
 * @brief 运行默认 C++ inproc 应用。
 *
 * @param run_ticks 可选的显式 tick 上限；为空表示无限运行。
 * @return runtime shell 执行状态。
 */
flowrt::Status run(std::optional<std::size_t> run_ticks);

/**
 * @brief 运行默认 C++ inproc 应用中的指定 process group。
 *
 * @param process process group 名称。
 * @param run_ticks 可选的显式 tick 上限；为空表示无限运行。
 * @return runtime shell 执行状态。
 */
flowrt::Status run_process(std::string_view process, std::optional<std::size_t> run_ticks);

}  // namespace flowrt_app

namespace flowrt_user {

/**
 * @brief 构造用户 C++ 组件实例并交给 FlowRT 管理 shell。
 *
 * 用户项目必须实现该函数。函数体应只装配用户组件对象，不写入 FlowRT 管理产物。
 *
 * @return 已注入用户组件实例的 FlowRT C++ 应用对象。
 */
flowrt_app::App build_app();

}  // namespace flowrt_user
