//! Rust Operation codegen：typed client handle、server handler、内部 lowering task。

use flowrt_ir::{
    ContractIr, GraphIr, OperationConcurrencyPolicy, OperationFeedbackPolicy,
    OperationPreemptPolicy,
};

use crate::messages::rust_type;
use crate::runtime_plan::{OperationRuntimePlan, SchedulerHiddenTaskPlan, operation_runtime_plans};
use crate::rust_string_literal;

pub(crate) fn rust_operation_handler_methods(
    component: &flowrt_ir::ComponentIr,
    graph: &GraphIr,
    plans: &[OperationRuntimePlan],
) -> String {
    let server_instances: std::collections::BTreeSet<&str> = graph
        .instances
        .iter()
        .filter(|instance| {
            instance.component.name == component.name
                || instance.component.name == component.generated_name
                || instance.component.name == component.qualified_name
        })
        .map(|instance| instance.name.as_str())
        .collect();

    let relevant_plans = plans
        .iter()
        .filter(|plan| server_instances.contains(plan.server_instance.as_str()))
        .collect::<Vec<_>>();
    if relevant_plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    let mut emitted = std::collections::BTreeSet::new();
    for plan in relevant_plans {
        let method_name = operation_handler_method_name(&plan.server_port);
        if !emitted.insert(method_name.clone()) {
            continue;
        }
        let goal_ty = rust_type(&plan.goal_type);
        let feedback_ty = rust_type(&plan.feedback_type);
        let result_ty = rust_type(&plan.result_type);
        let port_name = &plan.server_port;
        output.push_str(&format!(
            "    /// 处理 `{port_name}` Operation goal。\n\
             ///\n\
             /// runtime shell 在 hidden operation task 中调用该方法。用户业务逻辑\n\
             /// 负责长任务执行，在安全边界检查 cancel token，并通过 progress 发布 typed feedback。\n",
        ));
        output.push_str(&format!(
            "    fn {method_name}(\n\
                 {}self,\n\
                 _goal: &{goal_ty},\n\
                 _cancel: flowrt::OperationCancelToken,\n\
                 _progress: &mut flowrt::OperationProgressPublisher<{feedback_ty}>,\n\
             ) -> flowrt::OperationHandlerResult<{result_ty}> {{\n\
                 flowrt::OperationHandlerResult::failed()\n\
             }}\n\n",
            super::rust_component_receiver(component),
        ));
    }

    output
}

pub(crate) fn emit_rust_operation_client_handles(contract: &ContractIr, graph: &GraphIr) -> String {
    let plans = operation_runtime_plans(contract, graph);
    if plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    output.push_str("// ── Operation client typed handles ────────────────────────────────\n\n");
    output.push_str(
        "fn flowrt_operation_result<T>(result: flowrt::ServiceResult<T>) -> Result<T, flowrt::OperationClientError> {\n    match result {\n        flowrt::ServiceResult::Ok(value) => Ok(value),\n        flowrt::ServiceResult::Err(error, _) => Err(flowrt::OperationClientError::from_service_error(error)),\n    }\n}\n\npub(crate) fn flowrt_operation_id_string(id: flowrt::OperationId) -> String {\n    format!(\"{}:{}:{}\", id.operation_key, id.client_id, id.sequence)\n}\n\npub(crate) fn flowrt_operation_status_from_snapshot(name: &str, owner: &str, snapshot: flowrt::OperationStatusSnapshot) -> flowrt::IntrospectionOperationStatus {\n    let active = !snapshot.state.is_terminal() && snapshot.state != flowrt::OperationState::Idle;\n    flowrt::IntrospectionOperationStatus {\n        name: name.to_string(),\n        ready: true,\n        running: if active { 1 } else { 0 },\n        queued: 0,\n        current_operation_ids: if active { vec![flowrt_operation_id_string(snapshot.id)] } else { Vec::new() },\n        total_started: snapshot.health.started,\n        succeeded_count: snapshot.health.succeeded,\n        failed_count: snapshot.health.failed,\n        canceled_count: snapshot.health.canceled,\n        timeout_count: snapshot.health.timeout,\n        preempted_count: snapshot.health.preempted,\n        current_state: Some(snapshot.state.as_str().to_string()),\n        current_owner: if snapshot.owner.owner_key == 0 { None } else { Some(owner.to_string()) },\n        current_deadline_ms: if active { Some(snapshot.deadline_ms) } else { None },\n        last_event: Some(\"flowrt.operation.state_changed\".to_string()),\n        last_error: None,\n        last_transition_ms: Some(flowrt::monotonic_time_ms()),\n    }\n}\n\npub(crate) fn flowrt_operation_control_error<T>(error: flowrt::OperationControlError) -> flowrt::ServiceResult<T> {\n    let code = match error {\n        flowrt::OperationControlError::Busy { .. } | flowrt::OperationControlError::OwnerConflict { .. } => flowrt::ServiceError::Busy,\n        flowrt::OperationControlError::StaleInvocation { .. } | flowrt::OperationControlError::AlreadyTerminal { .. } => flowrt::ServiceError::Rejected,\n        flowrt::OperationControlError::InvalidPolicy(_) | flowrt::OperationControlError::InvalidTransition { .. } => flowrt::ServiceError::HandlerError,\n        flowrt::OperationControlError::Ok => flowrt::ServiceError::HandlerError,\n    };\n    flowrt::ServiceResult::err_with_message(code, error.to_string())\n}\n\n",
    );

    let mut emitted_handles = std::collections::BTreeSet::new();
    for plan in &plans {
        let handle_name = operation_client_handle_name(plan);
        if !emitted_handles.insert(handle_name.clone()) {
            continue;
        }
        let goal_ty = rust_type(&plan.goal_type);
        let is_zenoh = plan.backend.0 == "zenoh";
        let is_iox2 = plan.backend.0 == "iox2";
        let transport_module = if is_iox2 { "iox2" } else { "zenoh" };
        let transport_client = if is_iox2 {
            "Iox2ServiceClient"
        } else {
            "ZenohServiceClient"
        };

        output.push_str("#[allow(non_camel_case_types)]\n#[derive(Clone)]\n");
        output.push_str(&format!("pub struct {handle_name} {{\n"));
        if is_zenoh || is_iox2 {
            output.push_str(&format!(
                "    pub(crate) start_client: std::sync::Arc<std::sync::OnceLock<flowrt::{transport_module}::{transport_client}<flowrt::OperationStartRequest<{goal_ty}>, flowrt::OperationStartAck>>>,\n\
                 pub(crate) cancel_client: std::sync::Arc<std::sync::OnceLock<flowrt::{transport_module}::{transport_client}<flowrt::OperationId, flowrt::OperationStatusSnapshot>>>,\n\
                 pub(crate) status_client: std::sync::Arc<std::sync::OnceLock<flowrt::{transport_module}::{transport_client}<flowrt::OperationId, flowrt::OperationStatusSnapshot>>>,\n",
            ));
        } else {
            output.push_str(&format!(
                "    pub(crate) start_client: flowrt::InprocServiceClient<flowrt::OperationStartRequest<{goal_ty}>, flowrt::OperationStartAck>,\n\
                 pub(crate) cancel_client: flowrt::InprocServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>,\n\
                 pub(crate) status_client: flowrt::InprocServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>,\n",
            ));
        }
        output.push_str("}\n\n");

        output.push_str(&format!("impl {handle_name} {{\n"));
        if is_zenoh || is_iox2 {
            output.push_str(&format!(
                "    pub fn start(&self, goal: {goal_ty}, timeout: std::time::Duration) -> Result<flowrt::OperationStartAck, flowrt::OperationClientError> {{\n        let owner = flowrt::OperationOwner::new(flowrt::fnv1a64({owner_scope}.as_bytes()), flowrt::fnv1a64({owner_name}.as_bytes()));\n        let request = flowrt::OperationStartRequest::new(goal, owner, timeout);\n        let timeout_ms = timeout.as_millis().min(u128::from(u64::MAX)) as u64;\n        let Some(client) = self.start_client.get() else {{\n            return Err(flowrt::OperationClientError::Unavailable);\n        }};\n        flowrt_operation_result(client.call(request, timeout_ms))\n    }}\n\n",
                owner_scope = rust_string_literal(&plan.operation_name),
                owner_name = rust_string_literal(&format!("{}.{}", plan.client_instance, plan.client_port)),
            ));
            output.push_str(
                "    pub fn cancel(&self, id: flowrt::OperationId, timeout: std::time::Duration) -> Result<flowrt::OperationStatusSnapshot, flowrt::OperationClientError> {\n        let timeout_ms = timeout.as_millis().min(u128::from(u64::MAX)) as u64;\n        let Some(client) = self.cancel_client.get() else {\n            return Err(flowrt::OperationClientError::Unavailable);\n        };\n        flowrt_operation_result(client.call(id, timeout_ms))\n    }\n\n",
            );
            output.push_str(
                "    pub fn status(&self, id: flowrt::OperationId, timeout: std::time::Duration) -> Result<flowrt::OperationStatusSnapshot, flowrt::OperationClientError> {\n        let timeout_ms = timeout.as_millis().min(u128::from(u64::MAX)) as u64;\n        let Some(client) = self.status_client.get() else {\n            return Err(flowrt::OperationClientError::Unavailable);\n        };\n        flowrt_operation_result(client.call(id, timeout_ms))\n    }\n",
            );
        } else {
            output.push_str(&format!(
                "    pub fn start(&self, goal: {goal_ty}, timeout: std::time::Duration) -> Result<flowrt::OperationStartAck, flowrt::OperationClientError> {{\n        let owner = flowrt::OperationOwner::new(flowrt::fnv1a64({owner_scope}.as_bytes()), flowrt::fnv1a64({owner_name}.as_bytes()));\n        let request = flowrt::OperationStartRequest::new(goal, owner, timeout);\n        flowrt_operation_result(self.start_client.call(request, timeout))\n    }}\n\n",
                owner_scope = rust_string_literal(&plan.operation_name),
                owner_name = rust_string_literal(&format!("{}.{}", plan.client_instance, plan.client_port)),
            ));
            output.push_str(
                "    pub fn cancel(&self, id: flowrt::OperationId, timeout: std::time::Duration) -> Result<flowrt::OperationStatusSnapshot, flowrt::OperationClientError> {\n        flowrt_operation_result(self.cancel_client.call(id, timeout))\n    }\n\n",
            );
            output.push_str(
                "    pub fn status(&self, id: flowrt::OperationId, timeout: std::time::Duration) -> Result<flowrt::OperationStatusSnapshot, flowrt::OperationClientError> {\n        flowrt_operation_result(self.status_client.call(id, timeout))\n    }\n",
            );
        }
        output.push_str("}\n\n");
    }

    output
}

pub(crate) fn rust_app_operation_fields(contract: &ContractIr, graph: &GraphIr) -> String {
    let plans = operation_runtime_plans(contract, graph);
    if plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    for plan in &plans {
        output.push_str(&format!(
            "    {}: {},\n",
            operation_client_field_name(plan),
            operation_client_handle_name(plan)
        ));
        output.push_str(&format!(
            "    operation_control_{}: std::sync::Arc<std::sync::Mutex<flowrt::OperationControl>>,\n",
            plan.index
        ));
        if plan.backend.0 == "zenoh" {
            continue;
        }
        let goal_ty = rust_type(&plan.goal_type);
        if plan.backend.0 == "iox2" {
            output.push_str(&format!(
                "    {}: std::sync::Arc<std::sync::OnceLock<std::sync::Mutex<flowrt::iox2::Iox2ServiceServer<flowrt::OperationStartRequest<{goal_ty}>, flowrt::OperationStartAck>>>>,\n\
                 {}: std::sync::Arc<std::sync::OnceLock<std::sync::Mutex<flowrt::iox2::Iox2ServiceServer<flowrt::OperationId, flowrt::OperationStatusSnapshot>>>>,\n\
                 {}: std::sync::Arc<std::sync::OnceLock<std::sync::Mutex<flowrt::iox2::Iox2ServiceServer<flowrt::OperationId, flowrt::OperationStatusSnapshot>>>>,\n",
                operation_start_server_field_name(plan),
                operation_cancel_server_field_name(plan),
                operation_status_server_field_name(plan),
            ));
            continue;
        }
        output.push_str(&format!(
            "    {}: flowrt::InprocServiceServer<flowrt::OperationStartRequest<{goal_ty}>, flowrt::OperationStartAck>,\n\
             {}: flowrt::InprocServiceServer<flowrt::OperationId, flowrt::OperationStatusSnapshot>,\n\
             {}: flowrt::InprocServiceServer<flowrt::OperationId, flowrt::OperationStatusSnapshot>,\n",
            operation_start_server_field_name(plan),
            operation_cancel_server_field_name(plan),
            operation_status_server_field_name(plan),
        ));
    }
    output
}

pub(crate) fn emit_rust_operation_new(
    contract: &ContractIr,
    graph: &GraphIr,
    lane_id_base: usize,
) -> (String, String) {
    let plans = operation_runtime_plans(contract, graph);
    if plans.is_empty() {
        return (String::new(), String::new());
    }
    let has_inproc_operation = plans
        .iter()
        .any(|plan| !matches!(plan.backend.0.as_str(), "zenoh" | "iox2"));

    let mut registration = String::new();
    if has_inproc_operation {
        registration.push_str("        // ── Operation lowering registration\n");
        registration.push_str("        let operation_registry = flowrt::ServiceRegistry::new();\n");
    }

    let mut initializers = String::new();
    let mut operation_lane_offset = 0usize;
    for plan in &plans {
        let client_field = operation_client_field_name(plan);
        let handle_name = operation_client_handle_name(plan);
        if matches!(plan.backend.0.as_str(), "zenoh" | "iox2") {
            let operation_key_name = rust_string_literal(&plan.operation_name);
            let operation_key = format!("flowrt::fnv1a64({operation_key_name}.as_bytes())");
            let queue_depth = plan.queue_depth.max(1);
            let max_in_flight = plan.max_in_flight.max(1);
            let timeout_ms = plan.timeout_ms.max(1);
            let result_retention_ms = plan.result_retention_ms;
            let concurrency = rust_operation_concurrency(plan.concurrency);
            let preempt = rust_operation_preempt(plan.preempt);
            let control_var = format!("operation_control_{}", plan.index);
            registration.push_str(&format!(
                "        let operation_policy_{index} = match flowrt::OperationPolicy::new(\n\
                 std::time::Duration::from_millis({timeout_ms}),\n\
                 {concurrency},\n\
                 {preempt},\n\
                 {queue_depth},\n\
                 {max_in_flight},\n\
             ).and_then(|policy| policy.with_result_retention(std::time::Duration::from_millis({result_retention_ms}))) {{\n\
                 Ok(policy) => policy,\n\
                 Err(error) => panic!(\"validated operation policy rejected at runtime: {{error}}\"),\n\
             }};\n\
             let {control_var} = std::sync::Arc::new(std::sync::Mutex::new(flowrt::OperationControl::new({operation_key}, operation_policy_{index})));\n",
                index = plan.index,
            ));
            initializers.push_str(&format!(
                "            {client_field}: {handle_name} {{ start_client: std::sync::Arc::new(std::sync::OnceLock::new()), cancel_client: std::sync::Arc::new(std::sync::OnceLock::new()), status_client: std::sync::Arc::new(std::sync::OnceLock::new()) }},\n\
                 {control_var}: {control_var}.clone(),\n"
            ));
            if plan.backend.0 == "iox2" {
                initializers.push_str(&format!(
                    "            {}: std::sync::Arc::new(std::sync::OnceLock::new()),\n\
                     {}: std::sync::Arc::new(std::sync::OnceLock::new()),\n\
                     {}: std::sync::Arc::new(std::sync::OnceLock::new()),\n",
                    operation_start_server_field_name(plan),
                    operation_cancel_server_field_name(plan),
                    operation_status_server_field_name(plan),
                ));
            }
            continue;
        }

        let goal_ty = rust_type(&plan.goal_type);
        let feedback_ty = rust_type(&plan.feedback_type);
        let start_name = rust_string_literal(&operation_start_endpoint_name(plan));
        let cancel_name = rust_string_literal(&operation_cancel_endpoint_name(plan));
        let status_name = rust_string_literal(&operation_status_endpoint_name(plan));
        let feedback_name = rust_string_literal(&operation_feedback_endpoint_name(plan));
        let result_name = rust_string_literal(&operation_result_endpoint_name(plan));
        let operation_key_name = rust_string_literal(&plan.operation_name);
        let operation_key = format!("flowrt::fnv1a64({operation_key_name}.as_bytes())");
        let queue_depth = plan.queue_depth.max(1);
        let max_in_flight = plan.max_in_flight.max(1);
        let timeout_ms = plan.timeout_ms.max(1);
        let result_retention_ms = plan.result_retention_ms;
        let concurrency = rust_operation_concurrency(plan.concurrency);
        let preempt = rust_operation_preempt(plan.preempt);
        let lane_id = lane_id_base + operation_lane_offset + 1;
        operation_lane_offset += 1;
        let server_instance = &plan.server_instance;
        let method_name = operation_handler_method_name(&plan.server_port);
        let server_instance_ir = graph
            .instances
            .iter()
            .find(|instance| instance.name == *server_instance)
            .expect("validated operation server instance must exist");
        let server_component =
            crate::component_by_name(contract, &server_instance_ir.component.name);
        let operation_handler_call = if super::rust_component_is_parallel(server_component) {
            format!(
                "operation_worker_server.as_ref().as_ref().{method_name}(&goal_for_worker, cancel.clone(), &mut progress)"
            )
        } else {
            format!(
                "operation_worker_server\n\
                                 .lock()\n\
                                 .unwrap_or_else(|poisoned| poisoned.into_inner())\n\
                                 .{method_name}(&goal_for_worker, cancel.clone(), &mut progress)"
            )
        };
        let control_var = format!("operation_control_{}", plan.index);
        let start_handler = format!("operation_start_handler_{}", plan.index);
        let cancel_handler = format!("operation_cancel_handler_{}", plan.index);
        let status_handler = format!("operation_status_handler_{}", plan.index);
        let server_component = format!("operation_server_{}", plan.index);
        let start_reg = format!("operation_start_reg_{}", plan.index);
        let cancel_reg = format!("operation_cancel_reg_{}", plan.index);
        let status_reg = format!("operation_status_reg_{}", plan.index);

        registration.push_str(&format!(
            "        let _operation_feedback_endpoint_{index} = {feedback_name};\n\
             let _operation_result_endpoint_{index} = {result_name};\n\
",
            index = plan.index,
        ));

        registration.push_str(&format!(
            "        let operation_policy_{index} = match flowrt::OperationPolicy::new(\n\
                 std::time::Duration::from_millis({timeout_ms}),\n\
                 {concurrency},\n\
                 {preempt},\n\
                 {queue_depth},\n\
                 {max_in_flight},\n\
             ).and_then(|policy| policy.with_result_retention(std::time::Duration::from_millis({result_retention_ms}))) {{\n\
                 Ok(policy) => policy,\n\
                 Err(error) => panic!(\"validated operation policy rejected at runtime: {{error}}\"),\n\
             }};\n\
             let {control_var} = std::sync::Arc::new(std::sync::Mutex::new(flowrt::OperationControl::new({operation_key}, operation_policy_{index})));\n\
             let {server_component} = {server_instance}.clone();\n\
             let {start_handler}_control = {control_var}.clone();\n\
             let {start_handler} = move |request: flowrt::OperationStartRequest<{goal_ty}>| -> flowrt::ServiceResult<flowrt::OperationStartAck> {{\n\
                 let ack = match {start_handler}_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).start_with_timeout(request.owner, flowrt::monotonic_time_ms(), request.timeout()) {{\n\
                     Ok(ack) => ack,\n\
                     Err(error) => return flowrt_operation_control_error(error),\n\
                 }};\n\
                 let id = ack.id;\n\
                 let operation_worker_server = {server_component}.clone();\n\
                 let operation_worker_control = {start_handler}_control.clone();\n\
                 let goal_for_worker = request.goal;\n\
                 let spawn_result = std::thread::Builder::new()\n\
                     .name(\"flowrt-operation-{index}\".to_string())\n\
                     .spawn(move || {{\n\
                         loop {{\n\
                             let should_start = {{\n\
                                 let control = operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n\
                                 let status = match control.status(id) {{\n\
                                     Ok(status) => status,\n\
                                     Err(_) => return,\n\
                                 }};\n\
                                 if status.state.is_terminal() {{\n\
                                     return;\n\
                                 }}\n\
                                 control.ready_to_run(id)\n\
                             }};\n\
                             if should_start {{\n\
                                 break;\n\
                             }}\n\
                             std::thread::sleep(std::time::Duration::from_millis(1));\n\
                         }}\n\
                         let cancel = match operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).cancel_token_for(id) {{\n\
                             Some(cancel) => cancel,\n\
                             None => return,\n\
                         }};\n\
                         if operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).mark_running(id).is_err() {{\n\
                             return;\n\
                         }}\n\
                         let operation_progress_control = operation_worker_control.clone();\n\
                         let progress_hook: std::sync::Arc<dyn Fn(flowrt::OperationId, u64) + Send + Sync> = std::sync::Arc::new(move |progress_id, sequence| {{\n\
                             operation_progress_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).publish_progress(progress_id, sequence);\n\
                         }});\n\
                         let mut progress = flowrt::OperationProgressPublisher::<{feedback_ty}>::with_hook(id, progress_hook);\n\
                         let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {{\n\
                             {operation_handler_call}\n\
                         }}));\n\
                         let terminal_state = match result {{\n\
                             Ok(flowrt::OperationHandlerResult::Succeeded(_)) => flowrt::OperationState::Succeeded,\n\
                             Ok(flowrt::OperationHandlerResult::Failed) | Err(_) => flowrt::OperationState::Failed,\n\
                             Ok(flowrt::OperationHandlerResult::Canceled) => flowrt::OperationState::Cancelled,\n\
                         }};\n\
                         let _ = operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).complete(id, terminal_state);\n\
                     }});\n\
                 if spawn_result.is_err() {{\n\
                     let _ = {start_handler}_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).complete(id, flowrt::OperationState::Failed);\n\
                     return flowrt::ServiceResult::err(flowrt::ServiceError::HandlerError);\n\
                 }}\n\
                 flowrt::ServiceResult::ok(ack)\n\
             }};\n"
            ,
            index = plan.index,
        ));

        registration.push_str(&format!(
            "        let {cancel_handler}_control = {control_var}.clone();\n\
             let {cancel_handler} = move |id: flowrt::OperationId| -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {{\n\
                 let mut control = {cancel_handler}_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n\
                 let snapshot = control.snapshot();\n\
                 match control.request_cancel(id, snapshot.owner) {{\n\
                     Ok(snapshot) => flowrt::ServiceResult::ok(snapshot),\n\
                     Err(error) => flowrt_operation_control_error(error),\n\
                 }}\n\
             }};\n\
             let {status_handler}_control = {control_var}.clone();\n\
             let {status_handler} = move |id: flowrt::OperationId| -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {{\n\
                 match {status_handler}_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).status(id) {{\n\
                     Ok(snapshot) => flowrt::ServiceResult::ok(snapshot),\n\
                     Err(error) => flowrt_operation_control_error(error),\n\
                 }}\n\
             }};\n"
        ));

        registration.push_str(&format!(
            "        let {start_reg} = operation_registry.register_result_with_config::<flowrt::OperationStartRequest<{goal_ty}>, flowrt::OperationStartAck, _>(\n\
                 {start_name},\n\
                 flowrt::LaneId({lane_id}),\n\
                 flowrt::InprocServiceConfig {{ queue_depth: {queue_depth}, max_in_flight: {max_in_flight}, ..Default::default() }},\n\
                 {start_handler},\n\
             );\n\
             let {cancel_reg} = operation_registry.register_result_with_config::<flowrt::OperationId, flowrt::OperationStatusSnapshot, _>(\n\
                 {cancel_name},\n\
                 flowrt::LaneId({lane_id}),\n\
                 flowrt::InprocServiceConfig {{ queue_depth: {queue_depth}, max_in_flight: {max_in_flight}, ..Default::default() }},\n\
                 {cancel_handler},\n\
             );\n\
             let {status_reg} = operation_registry.register_result_with_config::<flowrt::OperationId, flowrt::OperationStatusSnapshot, _>(\n\
                 {status_name},\n\
                 flowrt::LaneId({lane_id}),\n\
                 flowrt::InprocServiceConfig {{ queue_depth: {queue_depth}, max_in_flight: {max_in_flight}, ..Default::default() }},\n\
                 {status_handler},\n\
             );\n"
        ));

        initializers.push_str(&format!(
            "            {client_field}: {handle_name} {{ start_client: {start_reg}.0, cancel_client: {cancel_reg}.0, status_client: {status_reg}.0 }},\n\
             {}: {start_reg}.1,\n\
             {}: {cancel_reg}.1,\n\
             {}: {status_reg}.1,\n\
             {control_var}: {control_var}.clone(),\n",
            operation_start_server_field_name(plan),
            operation_cancel_server_field_name(plan),
            operation_status_server_field_name(plan),
        ));
    }

    (registration, initializers)
}

pub(crate) fn emit_rust_operation_step_functions(contract: &ContractIr, graph: &GraphIr) -> String {
    let plans = operation_runtime_plans(contract, graph);
    if plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    output.push_str("// ── Operation step functions ───────────────────────────────────────\n\n");
    for plan in &plans {
        let operation_name = rust_string_literal(&plan.operation_name);
        let owner_name =
            rust_string_literal(&format!("{}.{}", plan.client_instance, plan.client_port));
        let control_var = format!("operation_control_{}", plan.index);
        let pending_block = match plan.backend.0.as_str() {
            "zenoh" => String::new(),
            "iox2" => rust_iox2_operation_pending_drain(
                contract,
                graph,
                plan,
                RustIox2OperationPendingDrain {
                    start_server_expr: &format!("self.{}", operation_start_server_field_name(plan)),
                    cancel_server_expr: &format!(
                        "self.{}",
                        operation_cancel_server_field_name(plan)
                    ),
                    status_server_expr: &format!(
                        "self.{}",
                        operation_status_server_field_name(plan)
                    ),
                    control_expr: &format!("self.{control_var}"),
                    server_expr: &format!("self.{}", plan.server_instance),
                    indent: "        ",
                    error_return: "return flowrt::Status::Error;",
                },
            ),
            _ => {
                format!(
                    "        self.{start_server}.process_pending_requests();\n        self.{cancel_server}.process_pending_requests();\n        self.{status_server}.process_pending_requests();\n",
                    start_server = operation_start_server_field_name(plan),
                    cancel_server = operation_cancel_server_field_name(plan),
                    status_server = operation_status_server_field_name(plan),
                )
            }
        };
        output.push_str(&format!(
            "    fn {fn_name}(&self, introspection_state: &flowrt::IntrospectionState, _health_map: &mut std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth>) -> flowrt::Status {{\n\
                 let operation_cancel_control = self.{control_var}.clone();\n\
                 introspection_state.register_operation_cancel_handler({operation_name}, move |operation_id| {{\n\
                     let mut control = operation_cancel_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n\
                     let snapshot = control.snapshot();\n\
                     if flowrt_operation_id_string(snapshot.id) != operation_id {{\n\
                         return Err(format!(\"stale operation invocation `{{}}`; current is `{{}}`\", operation_id, flowrt_operation_id_string(snapshot.id)));\n\
                     }}\n\
                     control.request_cancel(snapshot.id, snapshot.owner).map_err(|error| error.to_string())?;\n\
                     Ok(flowrt_operation_status_from_snapshot({operation_name}, {owner_name}, control.snapshot()))\n\
                 }});\n\
                 {pending_block}\
                 let mut operation_control = self.{control_var}.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n\
                 let _ = operation_control.check_deadline(flowrt::monotonic_time_ms());\n\
                 let snapshot = operation_control.snapshot();\n\
                 let events = operation_control.drain_events();\n\
                 drop(operation_control);\n\
                 for event in events {{\n\
                     let operation_id = flowrt_operation_id_string(event.id);\n\
                     match event.kind {{\n\
                         flowrt::OperationRuntimeEventKind::StateChanged => {{\n\
                             if let Some(state) = event.state {{\n\
                                 introspection_state.record_operation_transition({operation_name}, &operation_id, state.as_str(), Some({owner_name}), if state.is_terminal() {{ None }} else {{ Some(snapshot.deadline_ms) }});\n\
                             }}\n\
                         }}\n\
                         flowrt::OperationRuntimeEventKind::Progress => {{\n\
                             introspection_state.record_operation_progress({operation_name}, &operation_id, event.sequence.unwrap_or(0));\n\
                         }}\n\
                         flowrt::OperationRuntimeEventKind::Result => {{\n\
                             let result = event.state.map(flowrt::OperationState::as_str).unwrap_or(\"succeeded\");\n\
                             introspection_state.record_operation_result({operation_name}, &operation_id, result, None);\n\
                         }}\n\
                         flowrt::OperationRuntimeEventKind::Error => {{\n\
                             let result = event.state.map(flowrt::OperationState::as_str).unwrap_or(\"failed\");\n\
                             introspection_state.record_operation_result({operation_name}, &operation_id, result, Some(\"handler error\"));\n\
                         }}\n\
                     }}\n\
                 }}\n\
                 introspection_state.record_operation_health(flowrt_operation_status_from_snapshot({operation_name}, {owner_name}, snapshot));\n\
                 flowrt::Status::Ok\n\
             }}\n\n",
            fn_name = operation_step_fn_name(plan),
        ));
    }

    output
}

pub(crate) fn emit_rust_zenoh_operation_endpoints(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&flowrt_ir::InstanceIr],
) -> String {
    let plans = operation_runtime_plans(contract, graph);
    let active: std::collections::BTreeSet<&str> = order
        .iter()
        .map(|instance| instance.name.as_str())
        .collect();
    let zenoh_plans = plans
        .iter()
        .filter(|plan| plan.backend.0 == "zenoh")
        .filter(|plan| {
            active.contains(plan.client_instance.as_str())
                || active.contains(plan.server_instance.as_str())
        })
        .collect::<Vec<_>>();
    if zenoh_plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    output.push_str(
        "        let zenoh_operation_session = match flowrt::zenoh::open_session_from_environment() {\n\
         \x20           Ok(session) => Some(session),\n\
         \x20           Err(error) => {\n\
         \x20               eprintln!(\"FlowRT: failed to open zenoh operation session: {error}\");\n\
         \x20               status = flowrt::Status::Error;\n\
         \x20               None\n\
         \x20           }\n\
         \x20       };\n",
    );

    for plan in zenoh_plans {
        let goal_ty = rust_type(&plan.goal_type);
        let feedback_ty = rust_type(&plan.feedback_type);
        let start_name = rust_string_literal(&operation_start_endpoint_name(plan));
        let cancel_name = rust_string_literal(&operation_cancel_endpoint_name(plan));
        let status_name = rust_string_literal(&operation_status_endpoint_name(plan));
        let feedback_name = rust_string_literal(&operation_feedback_endpoint_name(plan));
        let result_name = rust_string_literal(&operation_result_endpoint_name(plan));

        if active.contains(plan.client_instance.as_str()) {
            let client_field = operation_client_field_name(plan);
            output.push_str(&format!(
                "        if let Some(session) = zenoh_operation_session.as_ref() {{\n\
                 \x20           let _ = app.{client_field}.start_client.set(flowrt::zenoh::ZenohServiceClient::open({start_name}, session.clone()));\n\
                 \x20           let _ = app.{client_field}.cancel_client.set(flowrt::zenoh::ZenohServiceClient::open({cancel_name}, session.clone()));\n\
                 \x20           let _ = app.{client_field}.status_client.set(flowrt::zenoh::ZenohServiceClient::open({status_name}, session.clone()));\n\
                 \x20       }}\n",
            ));
        }

        if !active.contains(plan.server_instance.as_str()) {
            continue;
        }

        let server_instance = &plan.server_instance;
        let method_name = operation_handler_method_name(&plan.server_port);
        let server_instance_ir = graph
            .instances
            .iter()
            .find(|instance| instance.name == *server_instance)
            .expect("validated operation server instance must exist");
        let server_component =
            crate::component_by_name(contract, &server_instance_ir.component.name);
        let operation_handler_call = if super::rust_component_is_parallel(server_component) {
            format!(
                "operation_worker_server.as_ref().as_ref().{method_name}(&goal_for_worker, cancel.clone(), &mut progress)"
            )
        } else {
            format!(
                "operation_worker_server\n\
                                     .lock()\n\
                                     .unwrap_or_else(|poisoned| poisoned.into_inner())\n\
                                     .{method_name}(&goal_for_worker, cancel.clone(), &mut progress)"
            )
        };
        let control_field = format!("operation_control_{}", plan.index);
        let start_handler = format!("operation_start_handler_{}", plan.index);
        let cancel_handler = format!("operation_cancel_handler_{}", plan.index);
        let status_handler = format!("operation_status_handler_{}", plan.index);
        let start_server_var = format!("_zenoh_operation_start_server_{}", plan.index);
        let cancel_server_var = format!("_zenoh_operation_cancel_server_{}", plan.index);
        let status_server_var = format!("_zenoh_operation_status_server_{}", plan.index);

        output.push_str(&format!(
            "        let _operation_feedback_endpoint_{index} = {feedback_name};\n\
             let _operation_result_endpoint_{index} = {result_name};\n\
             let {start_server_var} = if let Some(session) = zenoh_operation_session.as_ref() {{\n\
                 let {start_handler}_control = app.{control_field}.clone();\n\
                 let operation_server_{index} = app.{server_instance}.clone();\n\
                 let {start_handler} = move |request: flowrt::OperationStartRequest<{goal_ty}>| -> flowrt::ServiceResult<flowrt::OperationStartAck> {{\n\
                     let ack = match {start_handler}_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).start_with_timeout(request.owner, flowrt::monotonic_time_ms(), request.timeout()) {{\n\
                         Ok(ack) => ack,\n\
                         Err(error) => return flowrt_operation_control_error(error),\n\
                     }};\n\
                     let id = ack.id;\n\
                     let operation_worker_server = operation_server_{index}.clone();\n\
                     let operation_worker_control = {start_handler}_control.clone();\n\
                     let goal_for_worker = request.goal;\n\
                     let spawn_result = std::thread::Builder::new()\n\
                         .name(\"flowrt-operation-{index}\".to_string())\n\
                         .spawn(move || {{\n\
                             loop {{\n\
                                 let should_start = {{\n\
                                     let control = operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n\
                                     let status = match control.status(id) {{\n\
                                         Ok(status) => status,\n\
                                         Err(_) => return,\n\
                                     }};\n\
                                     if status.state.is_terminal() {{\n\
                                         return;\n\
                                     }}\n\
                                     control.ready_to_run(id)\n\
                                 }};\n\
                                 if should_start {{\n\
                                     break;\n\
                                 }}\n\
                                 std::thread::sleep(std::time::Duration::from_millis(1));\n\
                             }}\n\
                             let cancel = match operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).cancel_token_for(id) {{\n\
                                 Some(cancel) => cancel,\n\
                                 None => return,\n\
                             }};\n\
                             if operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).mark_running(id).is_err() {{\n\
                                 return;\n\
                             }}\n\
                             let operation_progress_control = operation_worker_control.clone();\n\
                             let progress_hook: std::sync::Arc<dyn Fn(flowrt::OperationId, u64) + Send + Sync> = std::sync::Arc::new(move |progress_id, sequence| {{\n\
                                 operation_progress_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).publish_progress(progress_id, sequence);\n\
                             }});\n\
                             let mut progress = flowrt::OperationProgressPublisher::<{feedback_ty}>::with_hook(id, progress_hook);\n\
                             let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {{\n\
                                 {operation_handler_call}\n\
                             }}));\n\
                             let terminal_state = match result {{\n\
                                 Ok(flowrt::OperationHandlerResult::Succeeded(_)) => flowrt::OperationState::Succeeded,\n\
                                 Ok(flowrt::OperationHandlerResult::Failed) | Err(_) => flowrt::OperationState::Failed,\n\
                                 Ok(flowrt::OperationHandlerResult::Canceled) => flowrt::OperationState::Cancelled,\n\
                             }};\n\
                             let _ = operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).complete(id, terminal_state);\n\
                         }});\n\
                     if spawn_result.is_err() {{\n\
                         let _ = {start_handler}_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).complete(id, flowrt::OperationState::Failed);\n\
                         return flowrt::ServiceResult::err(flowrt::ServiceError::HandlerError);\n\
                     }}\n\
                     flowrt::ServiceResult::ok(ack)\n\
                 }};\n\
                 match flowrt::zenoh::ZenohServiceServer::open({start_name}, session.clone(), {start_handler}) {{\n\
                     Ok(server) => Some(server),\n\
                     Err(error) => {{\n\
                         eprintln!(\"FlowRT: failed to open zenoh operation start server: {{error}}\");\n\
                         status = flowrt::Status::Error;\n\
                         None\n\
                     }}\n\
                 }}\n\
             }} else {{\n\
                 None\n\
             }};\n",
            index = plan.index,
        ));

        output.push_str(&format!(
            "        let {cancel_server_var} = if let Some(session) = zenoh_operation_session.as_ref() {{\n\
                 let {cancel_handler}_control = app.{control_field}.clone();\n\
                 let {cancel_handler} = move |id: flowrt::OperationId| -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {{\n\
                     let mut control = {cancel_handler}_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n\
                     let snapshot = control.snapshot();\n\
                     match control.request_cancel(id, snapshot.owner) {{\n\
                         Ok(snapshot) => flowrt::ServiceResult::ok(snapshot),\n\
                         Err(error) => flowrt_operation_control_error(error),\n\
                     }}\n\
                 }};\n\
                 match flowrt::zenoh::ZenohServiceServer::open({cancel_name}, session.clone(), {cancel_handler}) {{\n\
                     Ok(server) => Some(server),\n\
                     Err(error) => {{\n\
                         eprintln!(\"FlowRT: failed to open zenoh operation cancel server: {{error}}\");\n\
                         status = flowrt::Status::Error;\n\
                         None\n\
                     }}\n\
                 }}\n\
             }} else {{\n\
                 None\n\
             }};\n\
             let {status_server_var} = if let Some(session) = zenoh_operation_session.as_ref() {{\n\
                 let {status_handler}_control = app.{control_field}.clone();\n\
                 let {status_handler} = move |id: flowrt::OperationId| -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {{\n\
                     match {status_handler}_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).status(id) {{\n\
                         Ok(snapshot) => flowrt::ServiceResult::ok(snapshot),\n\
                         Err(error) => flowrt_operation_control_error(error),\n\
                     }}\n\
                 }};\n\
                 match flowrt::zenoh::ZenohServiceServer::open({status_name}, session.clone(), {status_handler}) {{\n\
                     Ok(server) => Some(server),\n\
                     Err(error) => {{\n\
                         eprintln!(\"FlowRT: failed to open zenoh operation status server: {{error}}\");\n\
                         status = flowrt::Status::Error;\n\
                         None\n\
                     }}\n\
                 }}\n\
             }} else {{\n\
                 None\n\
             }};\n",
        ));
    }

    output
}

/// 生成进程级 iox2 Operation endpoint 构造代码。
///
/// Operation over iox2 复用 start/cancel/status 三条内部 Service；
/// request drain 由 hidden operation task 通过 `poll_requests` 驱动。
pub(crate) fn emit_rust_iox2_operation_endpoints(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&flowrt_ir::InstanceIr],
) -> String {
    let plans = operation_runtime_plans(contract, graph);
    let active: std::collections::BTreeSet<&str> = order
        .iter()
        .map(|instance| instance.name.as_str())
        .collect();
    let iox2_plans = plans
        .iter()
        .filter(|plan| plan.backend.0 == "iox2")
        .filter(|plan| {
            active.contains(plan.client_instance.as_str())
                || active.contains(plan.server_instance.as_str())
        })
        .collect::<Vec<_>>();
    if iox2_plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    for plan in iox2_plans {
        let goal_ty = rust_type(&plan.goal_type);
        let start_name = rust_string_literal(
            plan.start_endpoint
                .service_name()
                .expect("iox2 operation start endpoint must have service name"),
        );
        let cancel_name = rust_string_literal(
            plan.cancel_endpoint
                .service_name()
                .expect("iox2 operation cancel endpoint must have service name"),
        );
        let status_name = rust_string_literal(
            plan.status_endpoint
                .service_name()
                .expect("iox2 operation status endpoint must have service name"),
        );
        if active.contains(plan.client_instance.as_str()) {
            let client_field = operation_client_field_name(plan);
            output.push_str(&format!(
                "        let _ = app.{client_field}.start_client.set(match flowrt::iox2::Iox2ServiceClient::open({start_name}) {{\n\
                 \x20           Ok(client) => client,\n\
                 \x20           Err(error) => {{\n\
                 \x20               eprintln!(\"FlowRT: failed to open iox2 operation start client {{}}: {{error}}\", {start_name});\n\
                 \x20               status = flowrt::Status::Error;\n\
                 \x20               flowrt::iox2::Iox2ServiceClient::unavailable({start_name}, error.to_string())\n\
                 \x20           }}\n\
                 \x20       }});\n\
                 \x20       let _ = app.{client_field}.cancel_client.set(match flowrt::iox2::Iox2ServiceClient::open({cancel_name}) {{\n\
                 \x20           Ok(client) => client,\n\
                 \x20           Err(error) => {{\n\
                 \x20               eprintln!(\"FlowRT: failed to open iox2 operation cancel client {{}}: {{error}}\", {cancel_name});\n\
                 \x20               status = flowrt::Status::Error;\n\
                 \x20               flowrt::iox2::Iox2ServiceClient::unavailable({cancel_name}, error.to_string())\n\
                 \x20           }}\n\
                 \x20       }});\n\
                 \x20       let _ = app.{client_field}.status_client.set(match flowrt::iox2::Iox2ServiceClient::open({status_name}) {{\n\
                 \x20           Ok(client) => client,\n\
                 \x20           Err(error) => {{\n\
                 \x20               eprintln!(\"FlowRT: failed to open iox2 operation status client {{}}: {{error}}\", {status_name});\n\
                 \x20               status = flowrt::Status::Error;\n\
                 \x20               flowrt::iox2::Iox2ServiceClient::unavailable({status_name}, error.to_string())\n\
                 \x20           }}\n\
                 \x20       }});\n",
            ));
        }

        if active.contains(plan.server_instance.as_str()) {
            let start_server = operation_start_server_field_name(plan);
            let cancel_server = operation_cancel_server_field_name(plan);
            let status_server = operation_status_server_field_name(plan);
            let max_in_flight = plan.max_in_flight.max(1);
            output.push_str(&format!(
                "        match flowrt::iox2::Iox2ServiceServer::<flowrt::OperationStartRequest<{goal_ty}>, flowrt::OperationStartAck>::open({start_name}, {max_in_flight}usize) {{\n\
                 \x20           Ok(mut server) => {{\n\
                 \x20               server.set_schedule_waiter(scheduler_events.clone());\n\
                 \x20               let _ = app.{start_server}.set(std::sync::Mutex::new(server));\n\
                 \x20           }}\n\
                 \x20           Err(error) => {{\n\
                 \x20               eprintln!(\"FlowRT: failed to open iox2 operation start server {{}}: {{error}}\", {start_name});\n\
                 \x20               status = flowrt::Status::Error;\n\
                 \x20           }}\n\
                 \x20       }}\n\
                 \x20       match flowrt::iox2::Iox2ServiceServer::<flowrt::OperationId, flowrt::OperationStatusSnapshot>::open({cancel_name}, {max_in_flight}usize) {{\n\
                 \x20           Ok(mut server) => {{\n\
                 \x20               server.set_schedule_waiter(scheduler_events.clone());\n\
                 \x20               let _ = app.{cancel_server}.set(std::sync::Mutex::new(server));\n\
                 \x20           }}\n\
                 \x20           Err(error) => {{\n\
                 \x20               eprintln!(\"FlowRT: failed to open iox2 operation cancel server {{}}: {{error}}\", {cancel_name});\n\
                 \x20               status = flowrt::Status::Error;\n\
                 \x20           }}\n\
                 \x20       }}\n\
                 \x20       match flowrt::iox2::Iox2ServiceServer::<flowrt::OperationId, flowrt::OperationStatusSnapshot>::open({status_name}, {max_in_flight}usize) {{\n\
                 \x20           Ok(mut server) => {{\n\
                 \x20               server.set_schedule_waiter(scheduler_events.clone());\n\
                 \x20               let _ = app.{status_server}.set(std::sync::Mutex::new(server));\n\
                 \x20           }}\n\
                 \x20           Err(error) => {{\n\
                 \x20               eprintln!(\"FlowRT: failed to open iox2 operation status server {{}}: {{error}}\", {status_name});\n\
                 \x20               status = flowrt::Status::Error;\n\
                 \x20           }}\n\
                 \x20       }}\n",
            ));
        }
    }
    output
}

pub(crate) struct RustIox2OperationPendingDrain<'a> {
    pub(crate) start_server_expr: &'a str,
    pub(crate) cancel_server_expr: &'a str,
    pub(crate) status_server_expr: &'a str,
    pub(crate) control_expr: &'a str,
    pub(crate) server_expr: &'a str,
    pub(crate) indent: &'a str,
    pub(crate) error_return: &'a str,
}

pub(crate) fn rust_iox2_operation_pending_drain(
    contract: &ContractIr,
    graph: &GraphIr,
    plan: &OperationRuntimePlan,
    emit: RustIox2OperationPendingDrain<'_>,
) -> String {
    let goal_ty = rust_type(&plan.goal_type);
    let feedback_ty = rust_type(&plan.feedback_type);
    let method_name = operation_handler_method_name(&plan.server_port);
    let server_instance_ir = graph
        .instances
        .iter()
        .find(|instance| instance.name == plan.server_instance)
        .expect("validated operation server instance must exist");
    let server_component = crate::component_by_name(contract, &server_instance_ir.component.name);
    let operation_handler_call = if super::rust_component_is_parallel(server_component) {
        format!(
            "operation_worker_server.as_ref().as_ref().{method_name}(&goal_for_worker, cancel.clone(), &mut progress)"
        )
    } else {
        format!(
            "operation_worker_server\n\
                         .lock()\n\
                         .unwrap_or_else(|poisoned| poisoned.into_inner())\n\
                         .{method_name}(&goal_for_worker, cancel.clone(), &mut progress)"
        )
    };
    let index = plan.index;
    let start_server_expr = emit.start_server_expr;
    let cancel_server_expr = emit.cancel_server_expr;
    let status_server_expr = emit.status_server_expr;
    let control_expr = emit.control_expr;
    let server_expr = emit.server_expr;
    let indent = emit.indent;
    let error_return = emit.error_return;
    format!(
        "{indent}if let Some(start_server) = {start_server_expr}.get() {{\n\
         {indent}    let mut start_server = start_server.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n\
         {indent}    let operation_start_control_{index} = {control_expr}.clone();\n\
         {indent}    let operation_server_{index} = {server_expr}.clone();\n\
         {indent}    if start_server.poll_requests(move |request: flowrt::OperationStartRequest<{goal_ty}>| -> flowrt::ServiceResult<flowrt::OperationStartAck> {{\n\
         {indent}        let ack = match operation_start_control_{index}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).start_with_timeout(request.owner, flowrt::monotonic_time_ms(), request.timeout()) {{\n\
         {indent}            Ok(ack) => ack,\n\
         {indent}            Err(error) => return flowrt_operation_control_error(error),\n\
         {indent}        }};\n\
         {indent}        let id = ack.id;\n\
         {indent}        let operation_worker_server = operation_server_{index}.clone();\n\
         {indent}        let operation_worker_control = operation_start_control_{index}.clone();\n\
         {indent}        let goal_for_worker = request.goal;\n\
         {indent}        let spawn_result = std::thread::Builder::new()\n\
         {indent}            .name(\"flowrt-operation-{index}\".to_string())\n\
         {indent}            .spawn(move || {{\n\
         {indent}                loop {{\n\
         {indent}                    let should_start = {{\n\
         {indent}                        let control = operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n\
         {indent}                        let status = match control.status(id) {{\n\
         {indent}                            Ok(status) => status,\n\
         {indent}                            Err(_) => return,\n\
         {indent}                        }};\n\
         {indent}                        if status.state.is_terminal() {{\n\
         {indent}                            return;\n\
         {indent}                        }}\n\
         {indent}                        control.ready_to_run(id)\n\
         {indent}                    }};\n\
         {indent}                    if should_start {{\n\
         {indent}                        break;\n\
         {indent}                    }}\n\
         {indent}                    std::thread::sleep(std::time::Duration::from_millis(1));\n\
         {indent}                }}\n\
         {indent}                let cancel = match operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).cancel_token_for(id) {{\n\
         {indent}                    Some(cancel) => cancel,\n\
         {indent}                    None => return,\n\
         {indent}                }};\n\
         {indent}                if operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).mark_running(id).is_err() {{\n\
         {indent}                    return;\n\
         {indent}                }}\n\
         {indent}                let operation_progress_control = operation_worker_control.clone();\n\
         {indent}                let progress_hook: std::sync::Arc<dyn Fn(flowrt::OperationId, u64) + Send + Sync> = std::sync::Arc::new(move |progress_id, sequence| {{\n\
         {indent}                    operation_progress_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).publish_progress(progress_id, sequence);\n\
         {indent}                }});\n\
         {indent}                let mut progress = flowrt::OperationProgressPublisher::<{feedback_ty}>::with_hook(id, progress_hook);\n\
         {indent}                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {{\n\
         {indent}                    {operation_handler_call}\n\
         {indent}                }}));\n\
         {indent}                let terminal_state = match result {{\n\
         {indent}                    Ok(flowrt::OperationHandlerResult::Succeeded(_)) => flowrt::OperationState::Succeeded,\n\
         {indent}                    Ok(flowrt::OperationHandlerResult::Failed) | Err(_) => flowrt::OperationState::Failed,\n\
         {indent}                    Ok(flowrt::OperationHandlerResult::Canceled) => flowrt::OperationState::Cancelled,\n\
         {indent}                }};\n\
         {indent}                let _ = operation_worker_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).complete(id, terminal_state);\n\
         {indent}            }});\n\
         {indent}        if spawn_result.is_err() {{\n\
         {indent}            let _ = operation_start_control_{index}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).complete(id, flowrt::OperationState::Failed);\n\
         {indent}            return flowrt::ServiceResult::err(flowrt::ServiceError::HandlerError);\n\
         {indent}        }}\n\
         {indent}        flowrt::ServiceResult::ok(ack)\n\
         {indent}    }}).is_err() {{\n\
         {indent}        {error_return}\n\
         {indent}    }}\n\
         {indent}}}\n\
         {indent}if let Some(cancel_server) = {cancel_server_expr}.get() {{\n\
         {indent}    let mut cancel_server = cancel_server.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n\
         {indent}    let operation_cancel_control_{index} = {control_expr}.clone();\n\
         {indent}    if cancel_server.poll_requests(move |id: flowrt::OperationId| -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {{\n\
         {indent}        let mut control = operation_cancel_control_{index}.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n\
         {indent}        let snapshot = control.snapshot();\n\
         {indent}        match control.request_cancel(id, snapshot.owner) {{\n\
         {indent}            Ok(snapshot) => flowrt::ServiceResult::ok(snapshot),\n\
         {indent}            Err(error) => flowrt_operation_control_error(error),\n\
         {indent}        }}\n\
         {indent}    }}).is_err() {{\n\
         {indent}        {error_return}\n\
         {indent}    }}\n\
         {indent}}}\n\
         {indent}if let Some(status_server) = {status_server_expr}.get() {{\n\
         {indent}    let mut status_server = status_server.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n\
         {indent}    let operation_status_control_{index} = {control_expr}.clone();\n\
         {indent}    if status_server.poll_requests(move |id: flowrt::OperationId| -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {{\n\
         {indent}        match operation_status_control_{index}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).status(id) {{\n\
         {indent}            Ok(snapshot) => flowrt::ServiceResult::ok(snapshot),\n\
         {indent}            Err(error) => flowrt_operation_control_error(error),\n\
         {indent}        }}\n\
         {indent}    }}).is_err() {{\n\
         {indent}        {error_return}\n\
         {indent}    }}\n\
         {indent}}}\n",
    )
}

pub(crate) fn emit_rust_operation_scheduler_registration(
    operation_tasks: &[&SchedulerHiddenTaskPlan],
) -> String {
    let mut task_output = String::new();
    for task in operation_tasks {
        let task_id = task.id;
        let lane_id = task.lane_id;
        let priority = task.priority;
        let operation = &task.source_name;
        task_output.push_str(&format!(
            "        // Operation task {task_id}: {operation}\n\
             scheduler.add_task(flowrt::TaskSpec {{ id: flowrt::TaskId({task_id}), lane: flowrt::LaneId({lane_id}), priority: {priority} }});\n",
        ));
    }

    task_output
}

pub(crate) fn emit_rust_operation_tick_driver_state(
    operation_tasks: &[&SchedulerHiddenTaskPlan],
) -> String {
    if operation_tasks.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    for task in operation_tasks {
        output.push_str(&format!(
            "            let mut flowrt_operation_tick_driven_{} = false;\n",
            task.source_index
        ));
    }
    output
}

pub(crate) fn emit_rust_operation_wake_checks(
    contract: &ContractIr,
    graph: &GraphIr,
    operation_tasks: &[&SchedulerHiddenTaskPlan],
) -> String {
    let plans = operation_runtime_plans(contract, graph);
    if plans.is_empty() || operation_tasks.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    for task in operation_tasks {
        let plan = plans
            .iter()
            .find(|plan| plan.index == task.source_index)
            .expect("scheduler operation task must reference an operation plan");
        let task_id = task.id;
        let tick_driven_flag = format!("flowrt_operation_tick_driven_{}", plan.index);
        let control_var = format!("operation_control_{}", plan.index);
        let pending_condition = match plan.backend.0.as_str() {
            "zenoh" => String::new(),
            "iox2" => format!(
                "(self.{start_server}.get().is_some()\n                     || self.{cancel_server}.get().is_some()\n                     || self.{status_server}.get().is_some()) && !{tick_driven_flag}\n                     || ",
                start_server = operation_start_server_field_name(plan),
                cancel_server = operation_cancel_server_field_name(plan),
                status_server = operation_status_server_field_name(plan),
                tick_driven_flag = tick_driven_flag,
            ),
            _ => format!(
                "self.{start_server}.pending_count() > 0\n                     || self.{cancel_server}.pending_count() > 0\n                     || self.{status_server}.pending_count() > 0\n                     || ",
                start_server = operation_start_server_field_name(plan),
                cancel_server = operation_cancel_server_field_name(plan),
                status_server = operation_status_server_field_name(plan),
            ),
        };
        output.push_str(&format!(
            "                let flowrt_operation_snapshot_{index} = self.{control_var}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).snapshot();\n\
                 let flowrt_operation_active_{index} = !flowrt_operation_snapshot_{index}.state.is_terminal()\n\
                     && flowrt_operation_snapshot_{index}.state != flowrt::OperationState::Idle;\n\
                 if {pending_condition}(flowrt_operation_active_{index} && !{tick_driven_flag}) {{\n\
                     scheduler.wake(flowrt::TaskId({task_id}));\n\
                     {tick_driven_flag} = true;\n\
                     woke_on_message = true;\n\
                 }}\n",
            index = plan.index,
            control_var = control_var,
        ));
    }

    output
}

pub(crate) fn operation_client_handle_name(plan: &OperationRuntimePlan) -> String {
    format!(
        "OperationClient_{}_{}",
        crate::snake_identifier(&plan.client_component),
        crate::snake_identifier(&plan.client_port)
    )
}

pub(crate) fn operation_client_field_name(plan: &OperationRuntimePlan) -> String {
    format!(
        "operation_client_{}_{}",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

pub(crate) fn operation_start_server_field_name(plan: &OperationRuntimePlan) -> String {
    format!(
        "operation_start_server_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

pub(crate) fn operation_cancel_server_field_name(plan: &OperationRuntimePlan) -> String {
    format!(
        "operation_cancel_server_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

pub(crate) fn operation_status_server_field_name(plan: &OperationRuntimePlan) -> String {
    format!(
        "operation_status_server_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

fn operation_step_fn_name(plan: &OperationRuntimePlan) -> String {
    format!(
        "step_operation_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

pub(crate) fn operation_handler_method_name(port_name: &str) -> String {
    format!("on_{}_operation", crate::snake_identifier(port_name))
}

pub(crate) fn operation_start_endpoint_name(plan: &OperationRuntimePlan) -> String {
    format!(
        "__flowrt_operation_{}_{}_start",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

pub(crate) fn operation_cancel_endpoint_name(plan: &OperationRuntimePlan) -> String {
    format!(
        "__flowrt_operation_{}_{}_cancel",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

pub(crate) fn operation_status_endpoint_name(plan: &OperationRuntimePlan) -> String {
    format!(
        "__flowrt_operation_{}_{}_status",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

pub(crate) fn operation_feedback_endpoint_name(plan: &OperationRuntimePlan) -> String {
    format!(
        "__flowrt_operation_{}_{}_feedback",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

pub(crate) fn operation_result_endpoint_name(plan: &OperationRuntimePlan) -> String {
    format!(
        "__flowrt_operation_{}_{}_result",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

fn rust_operation_concurrency(policy: OperationConcurrencyPolicy) -> &'static str {
    match policy {
        OperationConcurrencyPolicy::Reject => "flowrt::OperationConcurrencyPolicy::Reject",
        OperationConcurrencyPolicy::Queue => "flowrt::OperationConcurrencyPolicy::Queue",
    }
}

fn rust_operation_preempt(policy: OperationPreemptPolicy) -> &'static str {
    match policy {
        OperationPreemptPolicy::Reject => "flowrt::OperationPreemptPolicy::Reject",
        OperationPreemptPolicy::CancelRunning => "flowrt::OperationPreemptPolicy::CancelRunning",
    }
}

#[allow(dead_code)]
fn rust_operation_feedback(policy: OperationFeedbackPolicy) -> &'static str {
    match policy {
        OperationFeedbackPolicy::Latest => "latest",
        OperationFeedbackPolicy::Fifo => "fifo",
    }
}
