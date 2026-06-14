//! Rust Operation codegen：typed client handle、server handler、内部 lowering task。

use std::collections::BTreeMap;

use flowrt_ir::{
    ContractIr, GraphIr, OperationConcurrencyPolicy, OperationFeedbackPolicy,
    OperationPreemptPolicy,
};

use crate::messages::rust_type;
use crate::runtime_plan::{OperationRuntimePlan, operation_runtime_plans, operation_server_lane};
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
            "    /// 处理 `{port}` Operation goal。\n\
             ///\n\
             /// runtime shell 在 hidden operation task 中调用该方法。用户业务逻辑\n\
             /// 负责长任务执行，在安全边界检查 cancel token，并通过 progress 发布 typed feedback。\n",
            port = port_name,
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

        output.push_str("#[allow(non_camel_case_types)]\n#[derive(Clone)]\n");
        output.push_str(&format!("pub struct {handle_name} {{\n"));
        if is_zenoh {
            output.push_str("    pub(crate) _marker: std::marker::PhantomData<()>,\n");
        } else {
            output.push_str(&format!(
                "    pub(crate) start_client: flowrt::InprocServiceClient<flowrt::OperationStartRequest<{goal_ty}>, flowrt::OperationStartAck>,\n\
                 pub(crate) cancel_client: flowrt::InprocServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>,\n\
                 pub(crate) status_client: flowrt::InprocServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>,\n",
            ));
        }
        output.push_str("}\n\n");

        output.push_str(&format!("impl {handle_name} {{\n"));
        if is_zenoh {
            output.push_str(&format!(
                "    pub fn start(&self, _goal: {goal_ty}, _timeout: std::time::Duration) -> Result<flowrt::OperationStartAck, flowrt::OperationClientError> {{\n        Err(flowrt::OperationClientError::Backend)\n    }}\n\n",
            ));
            output.push_str(
                "    pub fn cancel(&self, _id: flowrt::OperationId, _timeout: std::time::Duration) -> Result<flowrt::OperationStatusSnapshot, flowrt::OperationClientError> {\n        Err(flowrt::OperationClientError::Backend)\n    }\n\n",
            );
            output.push_str(
                "    pub fn status(&self, _id: flowrt::OperationId, _timeout: std::time::Duration) -> Result<flowrt::OperationStatusSnapshot, flowrt::OperationClientError> {\n        Err(flowrt::OperationClientError::Backend)\n    }\n",
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
        if plan.backend.0 == "zenoh" {
            continue;
        }
        let goal_ty = rust_type(&plan.goal_type);
        output.push_str(&format!(
            "    {}: flowrt::InprocServiceServer<flowrt::OperationStartRequest<{goal_ty}>, flowrt::OperationStartAck>,\n\
             {}: flowrt::InprocServiceServer<flowrt::OperationId, flowrt::OperationStatusSnapshot>,\n\
             {}: flowrt::InprocServiceServer<flowrt::OperationId, flowrt::OperationStatusSnapshot>,\n",
            operation_start_server_field_name(plan),
            operation_cancel_server_field_name(plan),
            operation_status_server_field_name(plan),
        ));
        output.push_str(&format!(
            "    operation_control_{}: std::sync::Arc<std::sync::Mutex<flowrt::OperationControl>>,\n",
            plan.index
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
    let has_inproc_operation = plans.iter().any(|plan| plan.backend.0 != "zenoh");

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
        if plan.backend.0 == "zenoh" {
            initializers.push_str(&format!(
                "            {client_field}: {handle_name} {{ _marker: std::marker::PhantomData }},\n"
            ));
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
             ) {{\n\
                 Ok(policy) => policy,\n\
                 Err(error) => panic!(\"validated operation policy rejected at runtime: {{error}}\"),\n\
             }};\n\
             let {control_var} = std::sync::Arc::new(std::sync::Mutex::new(flowrt::OperationControl::new({operation_key}, operation_policy_{index})));\n\
             let {server_component} = {server_instance}.clone();\n\
             let {start_handler}_control = {control_var}.clone();\n\
             let {start_handler} = move |request: flowrt::OperationStartRequest<{goal_ty}>| -> flowrt::ServiceResult<flowrt::OperationStartAck> {{\n\
                 let ack = match {start_handler}_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).start_with_timeout(request.owner, flowrt::monotonic_time_ms(), request.timeout) {{\n\
                     Ok(ack) => ack,\n\
                     Err(error) => return flowrt_operation_control_error(error),\n\
                 }};\n\
                 let id = ack.id;\n\
                 let cancel = match {start_handler}_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).cancel_token() {{\n\
                     Some(cancel) => cancel,\n\
                     None => return flowrt::ServiceResult::err(flowrt::ServiceError::HandlerError),\n\
                 }};\n\
                 if let Err(error) = {start_handler}_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).mark_running(id) {{\n\
                     return flowrt_operation_control_error(error);\n\
                 }}\n\
                 let operation_worker_server = {server_component}.clone();\n\
                 let operation_worker_control = {start_handler}_control.clone();\n\
                 let goal_for_worker = request.goal;\n\
                 let spawn_result = std::thread::Builder::new()\n\
                     .name(\"flowrt-operation-{index}\".to_string())\n\
                     .spawn(move || {{\n\
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
             let {status_handler} = move |_id: flowrt::OperationId| -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {{\n\
                 flowrt::ServiceResult::ok({status_handler}_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).snapshot())\n\
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
        if plan.backend.0 == "zenoh" {
            continue;
        }
        let operation_name = rust_string_literal(&plan.operation_name);
        let owner_name =
            rust_string_literal(&format!("{}.{}", plan.client_instance, plan.client_port));
        let control_var = format!("operation_control_{}", plan.index);
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
                 self.{start_server}.process_pending_requests();\n\
                 self.{cancel_server}.process_pending_requests();\n\
                 self.{status_server}.process_pending_requests();\n\
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
            start_server = operation_start_server_field_name(plan),
            cancel_server = operation_cancel_server_field_name(plan),
            status_server = operation_status_server_field_name(plan),
        ));
    }

    output
}

pub(crate) fn emit_rust_operation_scheduler_registration(
    contract: &ContractIr,
    graph: &GraphIr,
    next_task_id: usize,
    lane_ids: &mut BTreeMap<String, usize>,
) -> (String, String, usize) {
    let plans = operation_runtime_plans(contract, graph);
    if plans.is_empty() {
        return (String::new(), String::new(), next_task_id);
    }

    let mut lane_output = String::new();
    let mut task_output = String::new();
    let mut task_id = next_task_id;
    for plan in &plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        let server_lane = operation_server_lane(plan);
        if !lane_ids.contains_key(&server_lane) {
            let lane_id = lane_ids.len() + 1;
            lane_ids.insert(server_lane.clone(), lane_id);
            lane_output.push_str(&format!(
                "        scheduler.add_lane(flowrt::LaneId({lane_id}), flowrt::LaneKind::Serial);\n        let _ = {server_lane:?};\n",
            ));
        }
        let lane_id = lane_ids[&server_lane];
        task_id += 1;
        task_output.push_str(&format!(
            "        // Operation task {task_id}: {operation}\n\
             scheduler.add_task(flowrt::TaskSpec {{ id: flowrt::TaskId({task_id}), lane: flowrt::LaneId({lane_id}), priority: 0 }});\n",
            operation = plan.operation_name,
        ));
    }

    (lane_output, task_output, task_id)
}

pub(crate) fn emit_rust_operation_tick_driver_state(
    contract: &ContractIr,
    graph: &GraphIr,
) -> String {
    let plans = operation_runtime_plans(contract, graph);
    if plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    for plan in &plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        output.push_str(&format!(
            "            let mut flowrt_operation_tick_driven_{} = false;\n",
            plan.index
        ));
    }
    output
}

pub(crate) fn emit_rust_operation_wake_checks(
    contract: &ContractIr,
    graph: &GraphIr,
    task_id_offset: usize,
) -> String {
    let plans = operation_runtime_plans(contract, graph);
    if plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    let mut task_id = task_id_offset;
    for plan in &plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        task_id += 1;
        let tick_driven_flag = format!("flowrt_operation_tick_driven_{}", plan.index);
        let control_var = format!("operation_control_{}", plan.index);
        output.push_str(&format!(
            "                let flowrt_operation_snapshot_{index} = self.{control_var}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).snapshot();\n\
                 let flowrt_operation_active_{index} = !flowrt_operation_snapshot_{index}.state.is_terminal()\n\
                     && flowrt_operation_snapshot_{index}.state != flowrt::OperationState::Idle;\n\
                 if self.{start_server}.pending_count() > 0\n\
                     || self.{cancel_server}.pending_count() > 0\n\
                     || self.{status_server}.pending_count() > 0\n\
                     || (flowrt_operation_active_{index} && !{tick_driven_flag}) {{\n\
                     scheduler.wake(flowrt::TaskId({task_id}));\n\
                     if flowrt_operation_active_{index} {{\n\
                         {tick_driven_flag} = true;\n\
                     }}\n\
                     woke_on_message = true;\n\
                 }}\n",
            index = plan.index,
            control_var = control_var,
            start_server = operation_start_server_field_name(plan),
            cancel_server = operation_cancel_server_field_name(plan),
            status_server = operation_status_server_field_name(plan),
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
