//! Rust service codegen：typed client handle、server handler、hidden task 注册。
//!
//! 从 `ServiceRuntimePlan` 生成：
//! - client 端：typed handle struct，暴露 `call` / `start_call`。
//! - server 端：component trait 中新增 `on_{port}_request` handler 方法。
//! - runtime shell：`ServiceRegistry` 注册 + hidden service task + scheduler wake glue。

use flowrt_ir::{ContractIr, GraphIr, ServiceOverflowPolicy};

use crate::messages::{
    frame_max_size_for_type, rust_type, rust_wire_size, type_contains_variable_data,
};
use crate::runtime_plan::{
    SchedulerHiddenTaskPlan, ServiceRuntimePlan, service_runtime_plans, service_server_lane,
};
use crate::rust_string_literal;

// ── Component trait handler 签名 ────────────────────────────────────────

/// 为有 service server 端口的 component 生成 trait 中的 handler 方法签名。
///
/// `plans` 是该 component 所在 graph 的全部 service plans。函数内部过滤出该 component
/// 作为 server 的 plans。
pub(crate) fn rust_service_handler_methods(
    component: &flowrt_ir::ComponentIr,
    graph: &GraphIr,
    plans: &[ServiceRuntimePlan],
) -> String {
    // 找出该 component 的所有实例作为 server 的 plans
    let server_instances: std::collections::BTreeSet<&str> = graph
        .instances
        .iter()
        .filter(|i| {
            i.component.name == component.name
                || i.component.name == component.generated_name
                || i.component.name == component.qualified_name
        })
        .map(|i| i.name.as_str())
        .collect();

    let relevant_plans: Vec<&ServiceRuntimePlan> = plans
        .iter()
        .filter(|p| server_instances.contains(p.server_instance.as_str()))
        .collect();

    if relevant_plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    let mut emitted = std::collections::BTreeSet::new();
    for plan in relevant_plans {
        let method_name = service_handler_method_name(&plan.server_port);
        if !emitted.insert(method_name.clone()) {
            continue;
        }
        let req_ty = rust_type(&plan.request_type);
        let resp_ty = rust_type(&plan.response_type);
        let port_name = &plan.server_port;

        output.push_str(&format!(
            "    /// 处理 `{port_name}` service request。\n\
             ///\n\
             /// runtime shell 在 hidden service task 中调用该方法。用户业务逻辑\n\
             /// 实现具体的 request -> response 转换。\n\
             ///\n\
             /// 返回 `flowrt::ServiceResult::Ok(response)` 表示成功，\n\
             /// `flowrt::ServiceResult::Err(error, message)` 表示业务错误。\n",
        ));
        output.push_str(&format!(
            "    fn {method_name}(\n\
                 {}self,\n\
                 _request: &{req_ty},\n\
             ) -> flowrt::ServiceResult<{resp_ty}> {{\n\
                 flowrt::ServiceResult::err(flowrt::ServiceError::HandlerError)\n\
             }}\n\n",
            super::rust_component_receiver(component),
        ));
    }

    output
}

// ── Client handle 代码生成 ──────────────────────────────────────────────

/// 为每个 service edge 生成 client handle struct 定义和 impl。
pub(crate) fn emit_rust_service_client_handles(contract: &ContractIr, graph: &GraphIr) -> String {
    let plans = service_runtime_plans(contract, graph);
    if plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    output.push_str("// ── Service client typed handles ───────────────────────────────────\n\n");
    let mut emitted_handles = std::collections::BTreeSet::new();

    for plan in &plans {
        let handle_name = client_handle_name(plan);
        if !emitted_handles.insert(handle_name.clone()) {
            continue;
        }
        let req_ty = rust_type(&plan.request_type);
        let resp_ty = rust_type(&plan.response_type);
        let is_zenoh = plan.backend.0 == "zenoh";
        let is_iox2 = plan.backend.0 == "iox2";

        // struct 定义
        if is_zenoh {
            output.push_str(&format!(
                "/// `{client}.{port}` service client typed handle（zenoh backend）。\n\
                 ///\n\
                 /// `inner` 在所属进程启动时由 runtime shell 用 `ZenohServiceClient` 填充；\n\
                 /// 其它进程不会填充，调用返回 `ServiceError::Unavailable`。\n",
                client = plan.client_instance,
                port = plan.client_port,
            ));
        } else if is_iox2 {
            output.push_str(&format!(
                "/// `{client}.{port}` service client typed handle（iox2 backend）。\n\
                 ///\n\
                 /// `inner` 在所属进程启动时由 runtime shell 用 `Iox2ServiceClient` 填充；\n\
                 /// 其它进程不会填充，调用返回 `ServiceError::Unavailable`。\n",
                client = plan.client_instance,
                port = plan.client_port,
            ));
        } else {
            output.push_str(&format!(
                "/// `{client}.{port}` service client typed handle。\n\
                 ///\n\
                 /// 封装 `flowrt::InprocServiceClient`，提供同步 `call()` 和\n\
                 /// 非阻塞 `start_call()` 调用路径。\n",
                client = plan.client_instance,
                port = plan.client_port,
            ));
        }
        output.push_str("#[allow(non_camel_case_types)]\n#[derive(Clone)]\n");
        output.push_str(&format!("pub struct {handle_name} {{\n"));
        if is_zenoh {
            output.push_str(&format!(
                "    pub(crate) inner: std::sync::Arc<std::sync::OnceLock<flowrt::zenoh::ZenohServiceClient<{req_ty}, {resp_ty}>>>,\n",
            ));
        } else if is_iox2 {
            let transport_ty = service_client_transport_type(contract, plan, &req_ty, &resp_ty);
            output.push_str(&format!(
                "    pub(crate) inner: std::sync::Arc<std::sync::OnceLock<{transport_ty}>>,\n",
            ));
        } else {
            output.push_str(&format!(
                "    pub(crate) inner: flowrt::InprocServiceClient<{req_ty}, {resp_ty}>,\n",
            ));
        }
        output.push_str("}\n\n");

        // impl 块
        output.push_str(&format!("impl {handle_name} {{\n"));

        if is_zenoh || is_iox2 {
            let backend = if is_iox2 { "iox2" } else { "zenoh" };
            // transport backend: 经 transport client 同步 request/response；未填充时 Unavailable
            output.push_str(&format!(
                "    /// 发起同步阻塞 {backend} service 调用。\n\
                 ///\n\
                 /// 所属进程未填充 transport client 时返回 `ServiceError::Unavailable`。\n\
                 pub fn call(\n\
                     &self,\n\
                     request: {req_ty},\n\
                     timeout: std::time::Duration,\n\
                 ) -> flowrt::ServiceResult<{resp_ty}> {{\n\
                     match self.inner.get() {{\n\
                         Some(client) => client.call(request, timeout.as_millis().min(u64::MAX as u128) as u64),\n\
                         None => flowrt::ServiceResult::err(flowrt::ServiceError::Unavailable),\n\
                     }}\n\
                 }}\n\n",
            ));
            output.push_str(&format!(
                "    /// 发起 {backend} service 调用并返回就绪 `ServiceCallHandle`。\n\
                 ///\n\
                 /// v1 实现先同步完成 query 再包装结果；transport client 未填充时返回就绪错误。\n\
                 pub fn start_call(\n\
                     &self,\n\
                     request: {req_ty},\n\
                     timeout: std::time::Duration,\n\
                 ) -> flowrt::ServiceCallHandle<{resp_ty}> {{\n\
                     match self.inner.get() {{\n\
                         Some(client) => flowrt::ServiceCallHandle::ready(client.call(request, timeout.as_millis().min(u64::MAX as u128) as u64)),\n\
                         None => flowrt::ServiceCallHandle::ready_error(flowrt::ServiceError::Unavailable),\n\
                     }}\n\
                 }}\n\n",
            ));
        } else {
            // inproc backend
            output.push_str(&format!(
                "    /// 发起同步阻塞 service 调用。\n\
                 ///\n\
                 /// 超时返回 `ServiceError::Timeout`，服务不可用返回 `Unavailable`，\n\
                 /// 队列满返回 `Busy`。\n\
                 pub fn call(\n\
                     &self,\n\
                     request: {req_ty},\n\
                     timeout: std::time::Duration,\n\
                 ) -> flowrt::ServiceResult<{resp_ty}> {{\n\
                     self.inner.call(request, timeout)\n\
                 }}\n\n",
            ));
            output.push_str(&format!(
                "    /// 发起非阻塞 service 调用，返回 `ServiceCallHandle`。\n\
                 ///\n\
                 /// handle 支持 `poll()` 查询就绪状态和 `complete()` 阻塞等待结果。\n\
                 pub fn start_call(\n\
                     &self,\n\
                     request: {req_ty},\n\
                     timeout: std::time::Duration,\n\
                 ) -> flowrt::ServiceCallHandle<{resp_ty}> {{\n\
                     self.inner.start_call(request, timeout)\n\
                 }}\n\n",
            ));
        }

        output.push_str("}\n\n");
    }

    output
}

// ── App struct 字段 ────────────────────────────────────────────────────

/// 生成 App struct 中的 service client 和 server 字段声明。
pub(crate) fn rust_app_service_fields(contract: &ContractIr, graph: &GraphIr) -> String {
    let plans = service_runtime_plans(contract, graph);
    if plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();

    for plan in &plans {
        let client_field = client_field_name(plan);
        let handle_name = client_handle_name(plan);
        output.push_str(&format!("    {client_field}: {handle_name},\n"));

        if plan.backend.0 == "zenoh" {
            continue;
        }
        let server_field = server_field_name(plan);
        let req_ty = rust_type(&plan.request_type);
        let resp_ty = rust_type(&plan.response_type);
        if plan.backend.0 == "iox2" {
            let transport_ty = service_server_transport_type(contract, plan, &req_ty, &resp_ty);
            output.push_str(&format!(
                "    {server_field}: std::sync::Arc<std::sync::OnceLock<std::sync::Mutex<{transport_ty}>>>,\n",
            ));
        } else {
            output.push_str(&format!(
                "    {server_field}: flowrt::InprocServiceServer<{req_ty}, {resp_ty}>,\n",
            ));
        }
    }

    output
}

// ── App::new() 注册 ────────────────────────────────────────────────────

/// 生成 App::new() 中的 service registry 注册和字段初始化代码。
///
/// `dataflow_lane_count` 是 dataflow task 占用的 lane 数量，service lane ID 从
/// `dataflow_lane_count + 1` 开始分配，与 scheduler 中的 lane 注册保持一致。
pub(crate) fn emit_rust_service_new(
    contract: &ContractIr,
    graph: &GraphIr,
    dataflow_lane_count: usize,
) -> (String, String) {
    let plans = service_runtime_plans(contract, graph);
    if plans.is_empty() {
        return (String::new(), String::new());
    }
    let has_inproc_service = plans.iter().any(|plan| plan.backend.0 == "inproc");

    let mut registration = String::new();
    if has_inproc_service {
        registration.push_str("        // ── Service registration\n");
        registration.push_str("        let service_registry = flowrt::ServiceRegistry::new();\n");
    }

    let mut initializers = String::new();
    let mut service_lane_offset: usize = 0;

    for plan in &plans {
        let is_zenoh = plan.backend.0 == "zenoh";
        let is_iox2 = plan.backend.0 == "iox2";
        let req_ty = rust_type(&plan.request_type);
        let resp_ty = rust_type(&plan.response_type);

        if is_zenoh || is_iox2 {
            // transport backend: skip inproc registration, generate placeholder initializers.
            // transport client/server 在所属进程启动时填充。
            let client_field = client_field_name(plan);
            let handle_name = client_handle_name(plan);

            initializers.push_str(&format!(
                "            {client_field}: {handle_name} {{ inner: std::sync::Arc::new(std::sync::OnceLock::new()) }},\n",
            ));
            if is_iox2 {
                let server_field = server_field_name(plan);
                initializers.push_str(&format!(
                    "            {server_field}: std::sync::Arc::new(std::sync::OnceLock::new()),\n",
                ));
            }
            continue;
        }

        let service_name_literal = rust_string_literal(&plan.service_name);
        let queue_depth = plan.queue_depth.max(1);
        let max_in_flight = plan.max_in_flight.max(1);
        let overflow = match plan.overflow {
            ServiceOverflowPolicy::Busy => "flowrt::ServiceOverflowPolicy::Busy",
            ServiceOverflowPolicy::Error => "flowrt::ServiceOverflowPolicy::Error",
        };
        let _server_lane = service_server_lane(plan);
        let server_instance = &plan.server_instance;
        let method_name = service_handler_method_name(&plan.server_port);
        let component_var = format!("{server_instance}_handler");
        let server_instance_ir = graph
            .instances
            .iter()
            .find(|instance| instance.name == *server_instance)
            .expect("validated service server instance must exist");
        let server_component =
            crate::component_by_name(contract, &server_instance_ir.component.name);
        let handler_call = if super::rust_component_is_parallel(server_component) {
            format!("{component_var}.as_ref().as_ref().{method_name}(&req)")
        } else {
            format!(
                "{component_var}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).{method_name}(&req)"
            )
        };

        // service lane ID 与 scheduler 中的 lane 注册保持一致
        let service_lane_id = dataflow_lane_count + service_lane_offset + 1;
        service_lane_offset += 1;

        let reg_var = format!("service_reg_{}", plan.index);
        let handler_var = format!("service_handler_{}", plan.index);

        // 注册 handler：捕获 component Arc clone，调用其 service handler 方法
        registration.push_str(&format!(
            "        let {component_var} = {server_instance}.clone();\n\
             let {handler_var} = move |req: {req_ty}| -> flowrt::ServiceResult<{resp_ty}> {{\n\
                 {handler_call}\n\
             }};\n\
             let {reg_var} = service_registry.register_result_with_config::<{req_ty}, {resp_ty}, _>(\n\
                 {service_name_literal},\n\
                 flowrt::LaneId({service_lane_id}),\n\
                 flowrt::InprocServiceConfig {{\n\
                     queue_depth: {queue_depth},\n\
                     max_in_flight: {max_in_flight},\n\
                     overflow: {overflow},\n\
                     ..Default::default()\n\
                 }},\n\
                 {handler_var},\n\
             );\n",
        ));

        let client_field = client_field_name(plan);
        let server_field = server_field_name(plan);
        let handle_name = client_handle_name(plan);

        initializers.push_str(&format!(
            "            {client_field}: {handle_name} {{ inner: {reg_var}.0 }},\n",
        ));
        initializers.push_str(&format!("            {server_field}: {reg_var}.1,\n",));
    }

    (registration, initializers)
}

// ── Service step 函数 ──────────────────────────────────────────────────

/// 生成每个 service server 的 hidden task step 函数。
pub(crate) fn emit_rust_service_step_functions(contract: &ContractIr, graph: &GraphIr) -> String {
    let plans = service_runtime_plans(contract, graph);
    if plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    output.push_str("// ── Service step functions ─────────────────────────────────────────\n\n");

    for plan in &plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        let fn_name = service_step_fn_name(plan);
        let server_field = server_field_name(plan);
        let server_instance = &plan.server_instance;
        let server_instance_ir = graph
            .instances
            .iter()
            .find(|instance| instance.name == *server_instance)
            .expect("validated service server instance must exist");
        let server_component =
            crate::component_by_name(contract, &server_instance_ir.component.name);
        let handler_method = service_handler_method_name(&plan.server_port);
        let handler_call = if super::rust_component_is_parallel(server_component) {
            format!("self.{server_instance}.as_ref().as_ref().{handler_method}(&request)")
        } else {
            format!(
                "self.{server_instance}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).{handler_method}(&request)"
            )
        };

        if plan.backend.0 == "iox2" {
            output.push_str(&format!(
                "    /// Hidden service task: process pending iox2 requests for `{server_instance}.{server_port}`。\n\
                 fn {fn_name}(&self, introspection_state: &flowrt::IntrospectionState, _health_map: &mut std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth>) -> flowrt::Status {{\n\
                     let Some(server) = self.{server_field}.get() else {{ return flowrt::Status::Error; }};\n\
                     let handled = match server.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).poll_requests(|request| {handler_call}) {{\n\
                         Ok(handled) => handled,\n\
                         Err(_) => return flowrt::Status::Error,\n\
                     }};\n\
                     {status_update}\
                     let _ = handled;\n\
                     flowrt::Status::Ok\n\
                 }}\n\n",
                server_instance = server_instance,
                server_port = plan.server_port,
                handler_call = handler_call,
                status_update = rust_iox2_service_status_update(plan, "handled"),
            ));
        } else {
            output.push_str(&format!(
                "    /// Hidden service task: process pending requests for `{server_instance}.{server_port}`。\n\
                 fn {fn_name}(&self, introspection_state: &flowrt::IntrospectionState, _health_map: &mut std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth>) -> flowrt::Status {{\n\
                     self.{server_field}.process_pending_requests();\n\
                     {status_update}\
                     flowrt::Status::Ok\n\
                 }}\n\n",
                server_instance = server_instance,
                server_port = plan.server_port,
                status_update = rust_service_status_update(plan),
            ));
        }
    }

    output
}

/// 生成运行开始时的 service introspection 注册代码。
pub(crate) fn emit_rust_service_introspection_registration(
    contract: &ContractIr,
    graph: &GraphIr,
) -> String {
    let plans = service_runtime_plans(contract, graph);
    if plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    for plan in &plans {
        if plan.backend.0 != "inproc" {
            continue;
        }
        let service_name = rust_string_literal(&plan.service_name);
        output.push_str(&format!(
            "        introspection_state.register_service({service_name});\n"
        ));
    }
    output
}

/// 生成 lifecycle startup 完成后的 service ready 标记代码。
pub(crate) fn emit_rust_service_ready_marks(contract: &ContractIr, graph: &GraphIr) -> String {
    let plans = service_runtime_plans(contract, graph);
    if plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    for plan in &plans {
        if plan.backend.0 != "inproc" {
            continue;
        }
        let service_name = rust_string_literal(&plan.service_name);
        output.push_str(&format!(
            "        introspection_state.mark_service_ready({service_name});\n"
        ));
    }
    output
}

fn rust_service_status_update(plan: &ServiceRuntimePlan) -> String {
    let server_field = server_field_name(plan);
    let service_name = rust_string_literal(&plan.service_name);
    format!(
        "{{\n\
             let service_stats = self.{server_field}.stats();\n\
             introspection_state.record_service_health(flowrt::IntrospectionServiceStatus {{\n\
                 name: {service_name}.to_string(),\n\
                 ready: true,\n\
                 in_flight: self.{server_field}.in_flight_count() as u64,\n\
                 queued: self.{server_field}.pending_count() as u64,\n\
                 total_requests: service_stats.requests,\n\
                 timeout_count: service_stats.timeout,\n\
                 busy_count: service_stats.busy,\n\
                 unavailable_count: service_stats.unavailable,\n\
                 late_drop_count: service_stats.late_dropped,\n\
             }});\n\
         }}\n"
    )
}

pub(crate) fn rust_iox2_service_status_update(
    plan: &ServiceRuntimePlan,
    handled_var: &str,
) -> String {
    let service_name = rust_string_literal(&plan.service_name);
    format!(
        "introspection_state.record_service_health(flowrt::IntrospectionServiceStatus {{\n\
             name: {service_name}.to_string(),\n\
             ready: true,\n\
             in_flight: 0,\n\
             queued: 0,\n\
             total_requests: {handled_var} as u64,\n\
             timeout_count: 0,\n\
             busy_count: 0,\n\
             unavailable_count: 0,\n\
             late_drop_count: 0,\n\
         }});\n"
    )
}

// ── Scheduler 集成 ─────────────────────────────────────────────────────

/// 生成 service task 的 scheduler lane 和 task 注册代码。
pub(crate) fn emit_rust_service_scheduler_registration(
    service_tasks: &[&SchedulerHiddenTaskPlan],
) -> String {
    let mut task_output = String::new();
    for task in service_tasks {
        let task_id = task.id;
        let lane_id = task.lane_id;
        let priority = task.priority;
        let service = &task.source_name;
        task_output.push_str(&format!(
            "        // Service task {task_id}: {service}\n\
             scheduler.add_task(flowrt::TaskSpec {{ id: flowrt::TaskId({task_id}), lane: flowrt::LaneId({lane_id}), priority: {priority} }});\n",
        ));
    }
    task_output
}

/// 生成 iox2 service hidden task 的每 tick 驱动状态。
pub(crate) fn emit_rust_service_tick_driver_state(
    contract: &ContractIr,
    graph: &GraphIr,
    service_tasks: &[&SchedulerHiddenTaskPlan],
) -> String {
    let plans = service_runtime_plans(contract, graph);
    if plans.is_empty() || service_tasks.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    for task in service_tasks {
        let Some(plan) = plans.iter().find(|plan| plan.index == task.source_index) else {
            continue;
        };
        if plan.backend.0 == "iox2" {
            output.push_str(&format!(
                "            let mut flowrt_service_tick_driven_{} = false;\n",
                plan.index
            ));
        }
    }
    output
}

/// 生成 service request arrival wake 检查代码。
pub(crate) fn emit_rust_service_wake_checks(
    contract: &ContractIr,
    graph: &GraphIr,
    service_tasks: &[&SchedulerHiddenTaskPlan],
) -> String {
    let plans = service_runtime_plans(contract, graph);
    if plans.is_empty() || service_tasks.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    for task in service_tasks {
        let plan = plans
            .iter()
            .find(|plan| plan.index == task.source_index)
            .expect("scheduler service task must reference a service plan");
        let task_id = task.id;
        let server_field = server_field_name(plan);
        if plan.backend.0 == "iox2" {
            let tick_driven_flag = format!("flowrt_service_tick_driven_{}", plan.index);
            output.push_str(&format!(
                "                if self.{server_field}.get().is_some() && !{tick_driven_flag} {{\n\
                         scheduler.wake(flowrt::TaskId({task_id}));\n\
                         {tick_driven_flag} = true;\n\
                         woke_on_message = true;\n\
                     }}\n",
            ));
        } else {
            output.push_str(&format!(
                "                if self.{server_field}.pending_count() > 0 {{\n\
                         scheduler.wake(flowrt::TaskId({task_id}));\n\
                         woke_on_message = true;\n\
                     }}\n",
            ));
        }
    }

    output
}

// ── Helper 函数 ────────────────────────────────────────────────────────

/// client handle struct 名称。
pub(crate) fn client_handle_name(plan: &ServiceRuntimePlan) -> String {
    format!(
        "ServiceClient_{}_{}",
        crate::snake_identifier(&plan.client_component),
        crate::snake_identifier(&plan.client_port)
    )
}

/// App struct 中 client handle 的字段名。
pub(crate) fn client_field_name(plan: &ServiceRuntimePlan) -> String {
    format!(
        "service_client_{}_{}",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

/// App struct 中 server handle 的字段名。
pub(crate) fn server_field_name(plan: &ServiceRuntimePlan) -> String {
    format!(
        "service_server_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

/// service hidden task step 函数名。
fn service_step_fn_name(plan: &ServiceRuntimePlan) -> String {
    format!(
        "step_service_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

/// server handler trait 方法名。
pub(crate) fn service_handler_method_name(port_name: &str) -> String {
    format!("on_{}_request", crate::snake_identifier(port_name))
}

/// 生成进程级 zenoh service 端点构造代码，注入 `run_process_*` 函数体。
///
/// 在所属进程为 zenoh service client 填充 transport client、为 server 打开 queryable，
/// 并登记/标记 server service ready。必须在 `app`（`Arc<Self>`）、`status` 和
/// `introspection_state` 可见的作用域注入；返回字符串直接使用这些名字，不经
/// `run_scope_receiver` 改写。server component 由 validator 保证为 `parallel`
/// （`Arc<Box<dyn Trait + Send + Sync>>`），handler 可在 queryable 回调线程安全调用。
pub(crate) fn emit_rust_zenoh_service_endpoints(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&flowrt_ir::InstanceIr],
) -> String {
    let plans = service_runtime_plans(contract, graph);
    let active: std::collections::BTreeSet<&str> = order
        .iter()
        .map(|instance| instance.name.as_str())
        .collect();
    let zenoh_plans: Vec<&ServiceRuntimePlan> = plans
        .iter()
        .filter(|plan| plan.backend.0 == "zenoh")
        .filter(|plan| {
            active.contains(plan.client_instance.as_str())
                || active.contains(plan.server_instance.as_str())
        })
        .collect();
    if zenoh_plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    output.push_str(
        "        let zenoh_service_session = match flowrt::zenoh::open_session_from_environment() {\n\
         \x20           Ok(session) => Some(session),\n\
         \x20           Err(error) => {\n\
         \x20               eprintln!(\"FlowRT: failed to open zenoh service session: {error}\");\n\
         \x20               status = flowrt::Status::Error;\n\
         \x20               None\n\
         \x20           }\n\
         \x20       };\n",
    );

    for plan in &zenoh_plans {
        let service_name_literal = rust_string_literal(&plan.service_name);
        if active.contains(plan.client_instance.as_str()) {
            let client_field = client_field_name(plan);
            output.push_str(&format!(
                "        if let Some(session) = zenoh_service_session.as_ref() {{\n\
                 \x20           let _ = app.{client_field}.inner.set(flowrt::zenoh::ZenohServiceClient::open({service_name_literal}, session.clone()));\n\
                 \x20       }}\n",
            ));
        }
        if active.contains(plan.server_instance.as_str()) {
            let server_instance = &plan.server_instance;
            let method_name = service_handler_method_name(&plan.server_port);
            let server_var = format!(
                "_zenoh_service_server_{}_{}",
                crate::snake_identifier(server_instance),
                crate::snake_identifier(&plan.server_port)
            );
            output.push_str(&format!(
                "        let {server_var} = if let Some(session) = zenoh_service_session.as_ref() {{\n\
                 \x20           let handler_component = app.{server_instance}.clone();\n\
                 \x20           match flowrt::zenoh::ZenohServiceServer::open(\n\
                 \x20               {service_name_literal},\n\
                 \x20               session.clone(),\n\
                 \x20               move |request| handler_component.as_ref().as_ref().{method_name}(&request),\n\
                 \x20           ) {{\n\
                 \x20               Ok(server) => {{\n\
                 \x20                   introspection_state.register_service({service_name_literal});\n\
                 \x20                   introspection_state.mark_service_ready({service_name_literal});\n\
                 \x20                   Some(server)\n\
                 \x20               }}\n\
                 \x20               Err(error) => {{\n\
                 \x20                   eprintln!(\"FlowRT: failed to open zenoh service server: {{error}}\");\n\
                 \x20                   status = flowrt::Status::Error;\n\
                 \x20                   None\n\
                 \x20               }}\n\
                 \x20           }}\n\
                 \x20       }} else {{\n\
                 \x20           None\n\
                 \x20       }};\n",
            ));
        }
    }
    output
}

/// 生成进程级 iox2 service endpoint 构造代码，注入 `run_process_*` 函数体。
///
/// client/server 都在所属进程启动后打开；server 只注册 transport endpoint，
/// request drain 仍由 scheduler hidden service task 调用 `poll_requests`。
pub(crate) fn emit_rust_iox2_service_endpoints(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&flowrt_ir::InstanceIr],
) -> String {
    let plans = service_runtime_plans(contract, graph);
    let active: std::collections::BTreeSet<&str> = order
        .iter()
        .map(|instance| instance.name.as_str())
        .collect();
    let iox2_plans: Vec<&ServiceRuntimePlan> = plans
        .iter()
        .filter(|plan| plan.backend.0 == "iox2")
        .filter(|plan| {
            active.contains(plan.client_instance.as_str())
                || active.contains(plan.server_instance.as_str())
        })
        .collect();
    if iox2_plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    for plan in &iox2_plans {
        let transport_service_name = plan
            .endpoint
            .service_name()
            .expect("iox2 service plan must have transport service name");
        let transport_service_name = rust_string_literal(transport_service_name);
        let service_name_literal = rust_string_literal(&plan.service_name);
        if active.contains(plan.client_instance.as_str()) {
            let client_field = client_field_name(plan);
            let client_ty = service_client_open_type(contract, plan);
            output.push_str(&format!(
                "        let _ = app.{client_field}.inner.set(match {client_ty}::open({transport_service_name}) {{\n\
                 \x20           Ok(client) => client,\n\
                 \x20           Err(error) => {{\n\
                 \x20               eprintln!(\"FlowRT: failed to open iox2 service client {{}}: {{error}}\", {transport_service_name});\n\
                 \x20               status = flowrt::Status::Error;\n\
                 \x20               {client_ty}::unavailable({transport_service_name}, error.to_string())\n\
                 \x20           }}\n\
                 \x20       }});\n",
            ));
        }
        if active.contains(plan.server_instance.as_str()) {
            let server_field = server_field_name(plan);
            let max_in_flight = plan.max_in_flight.max(1);
            let server_ty = service_server_open_type(contract, plan);
            output.push_str(&format!(
                "        match {server_ty}::open({transport_service_name}, {max_in_flight}usize) {{\n\
                 \x20           Ok(mut server) => {{\n\
                 \x20               server.set_schedule_waiter(scheduler_events.clone());\n\
                 \x20               let _ = app.{server_field}.set(std::sync::Mutex::new(server));\n\
                 \x20               introspection_state.register_service({service_name_literal});\n\
                 \x20               introspection_state.mark_service_ready({service_name_literal});\n\
                 \x20           }}\n\
                 \x20           Err(error) => {{\n\
                 \x20               eprintln!(\"FlowRT: failed to open iox2 service server {{}}: {{error}}\", {transport_service_name});\n\
                 \x20               status = flowrt::Status::Error;\n\
                 \x20           }}\n\
                 \x20       }}\n",
            ));
        }
    }
    output
}

fn service_client_transport_type(
    contract: &ContractIr,
    plan: &ServiceRuntimePlan,
    req_ty: &str,
    resp_ty: &str,
) -> String {
    if let Some((req_cap, resp_cap)) = iox2_frame_service_caps(contract, plan) {
        format!("flowrt::iox2::Iox2FrameServiceClient<{req_ty}, {resp_ty}, {req_cap}, {resp_cap}>")
    } else {
        format!("flowrt::iox2::Iox2ServiceClient<{req_ty}, {resp_ty}>")
    }
}

fn service_server_transport_type(
    contract: &ContractIr,
    plan: &ServiceRuntimePlan,
    req_ty: &str,
    resp_ty: &str,
) -> String {
    if let Some((req_cap, resp_cap)) = iox2_frame_service_caps(contract, plan) {
        format!("flowrt::iox2::Iox2FrameServiceServer<{req_ty}, {resp_ty}, {req_cap}, {resp_cap}>")
    } else {
        format!("flowrt::iox2::Iox2ServiceServer<{req_ty}, {resp_ty}>")
    }
}

fn service_client_open_type(contract: &ContractIr, plan: &ServiceRuntimePlan) -> String {
    if let Some((req_cap, resp_cap)) = iox2_frame_service_caps(contract, plan) {
        let req_ty = rust_type(&plan.request_type);
        let resp_ty = rust_type(&plan.response_type);
        format!(
            "flowrt::iox2::Iox2FrameServiceClient::<{req_ty}, {resp_ty}, {req_cap}, {resp_cap}>"
        )
    } else {
        "flowrt::iox2::Iox2ServiceClient".to_string()
    }
}

fn service_server_open_type(contract: &ContractIr, plan: &ServiceRuntimePlan) -> String {
    if let Some((req_cap, resp_cap)) = iox2_frame_service_caps(contract, plan) {
        let req_ty = rust_type(&plan.request_type);
        let resp_ty = rust_type(&plan.response_type);
        format!(
            "flowrt::iox2::Iox2FrameServiceServer::<{req_ty}, {resp_ty}, {req_cap}, {resp_cap}>"
        )
    } else {
        "flowrt::iox2::Iox2ServiceServer".to_string()
    }
}

fn iox2_frame_service_caps(
    contract: &ContractIr,
    plan: &ServiceRuntimePlan,
) -> Option<(usize, usize)> {
    let request_variable = type_contains_variable_data(contract, &plan.request_type);
    let response_variable = type_contains_variable_data(contract, &plan.response_type);
    if !request_variable && !response_variable {
        return None;
    }
    Some((
        frame_cap_for_service_expr(contract, &plan.request_type)?,
        frame_cap_for_service_expr(contract, &plan.response_type)?,
    ))
}

fn frame_cap_for_service_expr(contract: &ContractIr, expr: &flowrt_ir::TypeExpr) -> Option<usize> {
    match expr {
        flowrt_ir::TypeExpr::Named { name } => {
            frame_max_size_for_type(contract, crate::type_by_name(contract, name))
        }
        _ if type_contains_variable_data(contract, expr) => None,
        _ => Some(rust_wire_size(contract, expr)),
    }
}
