//! Rust service codegen：typed client handle、server handler、hidden task 注册。
//!
//! 从 `ServiceRuntimePlan` 生成：
//! - client 端：typed handle struct，暴露 `call` / `start_call`。
//! - server 端：component trait 中新增 `on_{port}_request` handler 方法。
//! - runtime shell：`ServiceRegistry` 注册 + hidden service task + scheduler wake glue。

use std::collections::BTreeMap;

use flowrt_ir::{ContractIr, GraphIr, ServiceOverflowPolicy};

use crate::messages::rust_type;
use crate::runtime_plan::{ServiceRuntimePlan, service_runtime_plans, service_server_lane};
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
    for plan in relevant_plans {
        let method_name = service_handler_method_name(&plan.server_port);
        let req_ty = rust_type(&plan.request_type);
        let resp_ty = rust_type(&plan.response_type);
        let port_name = &plan.server_port;

        output.push_str(&format!(
            "    /// 处理 `{port}` service request。\n\
             ///\n\
             /// runtime shell 在 hidden service task 中调用该方法。用户业务逻辑\n\
             /// 实现具体的 request -> response 转换。\n\
             ///\n\
             /// 返回 `flowrt::ServiceResult::Ok(response)` 表示成功，\n\
             /// `flowrt::ServiceResult::Err(error, message)` 表示业务错误。\n",
            port = port_name,
        ));
        output.push_str(&format!(
            "    fn {method_name}(\n\
                 &mut self,\n\
                 request: &{req_ty},\n\
             ) -> flowrt::ServiceResult<{resp_ty}> {{\n\
                 flowrt::ServiceResult::err(flowrt::ServiceError::HandlerError)\n\
             }}\n\n",
        ));
    }

    output
}

// ── Client handle 代码生成 ──────────────────────────────────────────────

/// 为每个 service edge 生成 client handle struct 定义和 impl。
pub(crate) fn emit_rust_service_client_handles(
    contract: &ContractIr,
    graph: &GraphIr,
) -> String {
    let plans = service_runtime_plans(contract, graph);
    if plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    output.push_str("// ── Service client typed handles ───────────────────────────────────\n\n");

    for plan in &plans {
        let handle_name = client_handle_name(plan);
        let req_ty = rust_type(&plan.request_type);
        let resp_ty = rust_type(&plan.response_type);
        let is_zenoh = plan.backend.0 == "zenoh";

        // struct 定义
        if is_zenoh {
            output.push_str(&format!(
                "/// `{client}.{port}` service client typed handle（zenoh backend，未实现）。\n\
                 ///\n\
                 /// 当前版本不支持 zenoh service transport。所有调用返回 `ServiceError::Backend`。\n",
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
        output.push_str("#[derive(Clone)]\n");
        output.push_str(&format!("pub struct {handle_name} {{\n"));
        if is_zenoh {
            output.push_str("    _marker: std::marker::PhantomData<()>,\n");
        } else {
            output.push_str(&format!(
                "    inner: flowrt::InprocServiceClient<{req_ty}, {resp_ty}>,\n",
            ));
        }
        output.push_str("}\n\n");

        // impl 块
        output.push_str(&format!("impl {handle_name} {{\n"));

        if is_zenoh {
            // zenoh backend: 返回 Unsupported
            output.push_str(&format!(
                "    /// zenoh service transport 尚未实现。\n\
                 pub fn call(\n\
                     &self,\n\
                     _request: {req_ty},\n\
                     _timeout: std::time::Duration,\n\
                 ) -> flowrt::ServiceResult<{resp_ty}> {{\n\
                     flowrt::ServiceResult::err(flowrt::ServiceError::Backend)\n\
                 }}\n\n",
            ));
            output.push_str(&format!(
                "    /// zenoh service transport 尚未实现。\n\
                 pub fn start_call(\n\
                     &self,\n\
                     _request: {req_ty},\n\
                     _timeout: std::time::Duration,\n\
                 ) -> flowrt::ServiceCallHandle<{resp_ty}> {{\n\
                     unimplemented!(\"zenoh service transport is not yet supported\")\n\
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

        let server_field = server_field_name(plan);
        let req_ty = rust_type(&plan.request_type);
        let resp_ty = rust_type(&plan.response_type);
        output.push_str(&format!(
            "    {server_field}: flowrt::InprocServiceServer<{req_ty}, {resp_ty}>,\n",
        ));
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

    let mut registration = String::new();
    registration.push_str("        // ── Service registration\n");
    registration.push_str("        let service_registry = flowrt::ServiceRegistry::new();\n");

    let mut initializers = String::new();
    let mut service_lane_offset: usize = 0;

    for plan in &plans {
        let is_zenoh = plan.backend.0 == "zenoh";
        let req_ty = rust_type(&plan.request_type);
        let resp_ty = rust_type(&plan.response_type);

        if is_zenoh {
            // zenoh backend: skip inproc registration, generate placeholder initializers
            let client_field = client_field_name(plan);
            let server_field = server_field_name(plan);
            let handle_name = client_handle_name(plan);
            let _req_ty2 = req_ty.clone();
            let _resp_ty2 = resp_ty.clone();

            initializers.push_str(&format!(
                "            {client_field}: {handle_name} {{ _marker: std::marker::PhantomData }},\n",
            ));
            initializers.push_str(&format!(
                "            {server_field}: unimplemented!(\"zenoh service transport is not yet supported\"),\n",
            ));
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

        // service lane ID 与 scheduler 中的 lane 注册保持一致
        let service_lane_id = dataflow_lane_count + service_lane_offset + 1;
        service_lane_offset += 1;

        let reg_var = format!("service_reg_{}", plan.index);
        let handler_var = format!("service_handler_{}", plan.index);
        let component_var = format!("{}_handler", server_instance);

        // 注册 handler：捕获 component Rc clone，调用其 service handler 方法
        registration.push_str(&format!(
            "        let {component_var} = {server_instance}.clone();\n\
             let {handler_var} = move |req: {req_ty}| -> flowrt::ServiceResult<{resp_ty}> {{\n\
                 {component_var}.borrow_mut().{method_name}(&req)\n\
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
        initializers.push_str(&format!(
            "            {server_field}: {reg_var}.1,\n",
        ));
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
        let fn_name = service_step_fn_name(plan);
        let server_field = server_field_name(plan);
        let server_instance = &plan.server_instance;

        output.push_str(&format!(
            "    /// Hidden service task: process pending requests for `{server_instance}.{server_port}`。\n\
             fn {fn_name}(&mut self) -> flowrt::Status {{\n\
                 self.{server_field}.process_pending_requests();\n\
                 flowrt::Status::Ok\n\
             }}\n\n",
            server_instance = server_instance,
            server_port = plan.server_port,
        ));
    }

    output
}

// ── Scheduler 集成 ─────────────────────────────────────────────────────

/// 生成 service task 的 scheduler lane 和 task 注册代码。
pub(crate) fn emit_rust_service_scheduler_registration(
    contract: &ContractIr,
    graph: &GraphIr,
    next_task_id: usize,
    lane_ids: &mut BTreeMap<String, usize>,
) -> (String, String, usize) {
    let plans = service_runtime_plans(contract, graph);
    if plans.is_empty() {
        return (String::new(), String::new(), next_task_id);
    }

    let mut lane_output = String::new();
    let mut task_output = String::new();
    let mut task_id = next_task_id;

    for plan in &plans {
        let server_lane = service_server_lane(plan);

        // 注册 lane（如果尚未注册）
        if !lane_ids.contains_key(&server_lane) {
            let lane_id = lane_ids.len() + 1;
            lane_ids.insert(server_lane.clone(), lane_id);
            lane_output.push_str(&format!(
                "        scheduler.add_lane(flowrt::LaneId({lane_id}), flowrt::LaneKind::Serial);\n        let _ = {lane:?};\n",
                lane = server_lane,
            ));
        }

        let lane_id = lane_ids[&server_lane];
        task_id += 1;
        task_output.push_str(&format!(
            "        // Service task {task_id}: {service}\n\
             scheduler.add_task(flowrt::TaskSpec {{ id: flowrt::TaskId({task_id}), lane: flowrt::LaneId({lane_id}), priority: 0 }});\n",
            service = plan.service_name,
        ));
    }

    (lane_output, task_output, task_id)
}

/// 生成 scheduler dispatch 中的 service task case。
pub(crate) fn rust_service_dispatch_cases(
    contract: &ContractIr,
    graph: &GraphIr,
    task_id_offset: usize,
) -> (String, usize) {
    let plans = service_runtime_plans(contract, graph);
    if plans.is_empty() {
        return (String::new(), task_id_offset);
    }

    let mut output = String::new();
    let mut task_id = task_id_offset;

    for plan in &plans {
        task_id += 1;
        let fn_name = service_step_fn_name(plan);
        output.push_str(&format!(
            "                flowrt::TaskId({task_id}) => self.{fn_name}(),\n"
        ));
    }

    (output, task_id)
}

/// 生成 service request arrival wake 检查代码。
pub(crate) fn emit_rust_service_wake_checks(
    contract: &ContractIr,
    graph: &GraphIr,
    task_id_offset: usize,
) -> String {
    let plans = service_runtime_plans(contract, graph);
    if plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    let mut task_id = task_id_offset;

    for plan in &plans {
        task_id += 1;
        let server_field = server_field_name(plan);
        output.push_str(&format!(
            "                if self.{server_field}.pending_count() > 0 {{\n\
                     scheduler.wake(flowrt::TaskId({task_id}));\n\
                     woke_on_message = true;\n\
                 }}\n",
        ));
    }

    output
}

// ── Helper 函数 ────────────────────────────────────────────────────────

/// client handle struct 名称。
pub(crate) fn client_handle_name(plan: &ServiceRuntimePlan) -> String {
    format!(
        "ServiceClient_{}_{}",
        crate::snake_identifier(&plan.client_instance),
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
