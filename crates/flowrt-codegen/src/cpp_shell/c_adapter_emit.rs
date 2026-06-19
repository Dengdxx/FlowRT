use super::*;

pub(super) fn emit_c_adapter_helpers(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&InstanceIr],
) -> String {
    let mut output = String::new();
    output.push_str(
        r#"using namespace flowrt_app;

flowrt_string_view_t c_string_view(std::string_view value) {
    return flowrt_string_view_t{.data = value.data(), .len = value.size()};
}

flowrt_bytes_view_t c_bytes_view(const void* data, std::size_t size) {
    return flowrt_bytes_view_t{.data = reinterpret_cast<const std::uint8_t*>(data), .len = size};
}

std::string c_param_json(bool value) {
    return value ? "true" : "false";
}

template <typename T>
std::string c_param_json(T value) {
    return std::to_string(value);
}

std::string c_param_json(const std::string& value) {
    return value;
}

flowrt_c_param_snapshot_v0_t make_c_param_snapshot(const flowrt_param_view_t* params,
                                                   std::size_t param_count) {
    return flowrt_c_param_snapshot_v0_t{
        .abi_version = 0U,
        .param_count = static_cast<std::uint32_t>(param_count),
        .params = params,
        .reserved = {},
    };
}

flowrt_c_clock_source_t c_clock_source(flowrt::ClockSource source) {
    switch (source) {
        case flowrt::ClockSource::Runtime:
            return FLOWRT_C_CLOCK_SOURCE_RUNTIME;
        case flowrt::ClockSource::Replay:
            return FLOWRT_C_CLOCK_SOURCE_REPLAY;
    }
    return FLOWRT_C_CLOCK_SOURCE_RUNTIME;
}

flowrt_c_task_timing_t make_c_task_timing(const flowrt::TaskTiming& timing) {
    return flowrt_c_task_timing_t{
        .step = timing.step,
        .task_name = c_string_view(timing.task_name),
        .trigger = c_string_view(timing.trigger),
        .clock_source = c_clock_source(timing.clock_source),
        .reserved0 = 0U,
        .scheduled_time_ms = timing.scheduled_time_ms,
        .observed_time_ms = timing.observed_time_ms,
        .scheduled_delta_ms = timing.scheduled_delta_ms,
        .observed_delta_ms = timing.observed_delta_ms,
        .period_ms = timing.period_ms.value_or(0U),
        .deadline_ms = timing.deadline_ms.value_or(0U),
        .lateness_ms = timing.lateness_ms,
        .missed_periods = timing.missed_periods,
        .has_period_ms = timing.period_ms.has_value() ? std::uint8_t{1} : std::uint8_t{0},
        .has_deadline_ms = timing.deadline_ms.has_value() ? std::uint8_t{1} : std::uint8_t{0},
        .deadline_missed = timing.deadline_missed ? std::uint8_t{1} : std::uint8_t{0},
        .overrun = timing.overrun ? std::uint8_t{1} : std::uint8_t{0},
        .reserved = {},
    };
}

flowrt::Status status_from_c(flowrt_status_t status) {
    switch (status) {
        case FLOWRT_STATUS_OK:
            return flowrt::Status::Ok;
        case FLOWRT_STATUS_RETRY:
            return flowrt::Status::Retry;
        case FLOWRT_STATUS_ERROR:
            return flowrt::Status::Error;
        default:
            return flowrt::Status::Error;
    }
}

const char* callback_table_validation_error(const flowrt_c_component_callback_table_t* callbacks,
                                            bool needs_periodic,
                                            bool needs_on_message,
                                            bool needs_startup,
                                            bool needs_shutdown) {
    if (callbacks == nullptr) {
        return "FlowRT C component callback table is null";
    }
    if (callbacks->size < sizeof(flowrt_c_component_callback_table_t)) {
        return "FlowRT C component callback table size is too small";
    }
    if (callbacks->version_major != FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MAJOR) {
        return "FlowRT C component callback table major version mismatch";
    }
    if (callbacks->version_minor != FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MINOR) {
        return "FlowRT C component callback table minor version mismatch";
    }
    constexpr std::uint64_t kRequiredFeatures =
        FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0 |
        FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1;
    constexpr std::uint64_t kKnownFeatures = kRequiredFeatures;
    if ((callbacks->feature_flags & FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0) !=
        FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0) {
        return "FlowRT C component callback table missing v0 feature bit";
    }
    if ((callbacks->feature_flags & FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1) !=
        FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1) {
        return "FlowRT C component callback table missing task timing feature bit";
    }
    if ((callbacks->feature_flags & ~kKnownFeatures) != 0U) {
        return "FlowRT C component callback table has unknown feature bit";
    }
    if (needs_periodic && callbacks->run_periodic == nullptr) {
        return "FlowRT C component callback table missing run_periodic";
    }
    if (needs_on_message && callbacks->run_on_message == nullptr) {
        return "FlowRT C component callback table missing run_on_message";
    }
    if (needs_startup && callbacks->run_startup == nullptr) {
        return "FlowRT C component callback table missing run_startup";
    }
    if (needs_shutdown && callbacks->run_shutdown == nullptr) {
        return "FlowRT C component callback table missing run_shutdown";
    }
    return nullptr;
}

flowrt_c_component_context_t make_c_component_context(std::string_view component_name,
                                                      std::string_view instance_name,
                                                      std::string_view task_name,
                                                      std::string_view lane_name,
                                                      const flowrt::Context& runtime_context,
                                                      std::uint64_t step,
                                                      std::uint64_t tick_time_ms,
                                                      std::uint64_t deadline_ms,
                                                      bool has_deadline_ms,
                                                      flowrt_c_param_snapshot_v0_t param_snapshot) {
    const auto* timing = runtime_context.timing();
    return flowrt_c_component_context_t{
        .component_name = c_string_view(component_name),
        .instance_name = c_string_view(instance_name),
        .task_name = c_string_view(task_name),
        .lane_name = c_string_view(lane_name),
        .step = timing != nullptr ? timing->step : step,
        .tick_time_ms = timing != nullptr ? timing->observed_time_ms : tick_time_ms,
        .deadline_ms =
            timing != nullptr ? timing->deadline_ms.value_or(0U) : deadline_ms,
        .has_deadline_ms =
            timing != nullptr ? (timing->deadline_ms.has_value() ? std::uint8_t{1} : std::uint8_t{0})
                              : (has_deadline_ms ? std::uint8_t{1} : std::uint8_t{0}),
        .has_timing = timing != nullptr ? std::uint8_t{1} : std::uint8_t{0},
        .reserved = {},
        .timing = timing != nullptr ? make_c_task_timing(*timing) : flowrt_c_task_timing_t{},
        .params = param_snapshot,
    };
}

"#,
    );

    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        if component.language != LanguageKind::C {
            continue;
        }
        output.push_str(&emit_c_adapter_class(graph, instance, component));
    }

    output
}

pub(super) fn emit_c_adapter_class(
    graph: &GraphIr,
    instance: &InstanceIr,
    component: &ComponentIr,
) -> String {
    let class_name = c_adapter_class_name(instance);
    let factory_name = c_adapter_factory_name(instance);
    let symbol = c_callback_factory_symbol(instance);
    let needs_periodic = c_bool_literal(
        tasks_for_instance(graph, instance)
            .any(|task| task.trigger == flowrt_ir::TriggerKind::Periodic),
    );
    let needs_on_message = c_bool_literal(
        tasks_for_instance(graph, instance)
            .any(|task| task.trigger == flowrt_ir::TriggerKind::OnMessage),
    );
    let needs_startup = c_bool_literal(
        tasks_for_instance(graph, instance)
            .any(|task| task.trigger == flowrt_ir::TriggerKind::Startup),
    );
    let needs_shutdown = c_bool_literal(
        tasks_for_instance(graph, instance)
            .any(|task| task.trigger == flowrt_ir::TriggerKind::Shutdown),
    );

    let mut output = String::new();
    output.push_str(&format!(
        "class {class_name} final : public flowrt_app::{component}Interface {{\npublic:\n    explicit {class_name}(const flowrt_c_component_callback_table_t* callbacks) : callbacks_(callbacks) {{}}\n\n",
        component = component_cpp_name(component),
    ));
    output.push_str(&emit_c_adapter_lifecycle_method(
        "on_init",
        &component.name,
        &instance.name,
    ));
    output.push_str(&emit_c_adapter_lifecycle_method(
        "on_start",
        &component.name,
        &instance.name,
    ));
    output.push_str(&emit_c_adapter_lifecycle_method(
        "on_stop",
        &component.name,
        &instance.name,
    ));
    output.push_str(&emit_c_adapter_lifecycle_method(
        "on_shutdown",
        &component.name,
        &instance.name,
    ));
    output.push_str(&emit_c_adapter_on_tick_override(component));
    for task in tasks_for_instance(graph, instance) {
        output.push_str(&emit_c_adapter_task_method(component, instance, task));
    }
    output.push_str(&format!(
        "private:\n    bool callbacks_valid(std::string_view instance_name) const {{\n        const char* error = callback_table_validation_error(callbacks_, {needs_periodic}, {needs_on_message}, {needs_startup}, {needs_shutdown});\n        if (error == nullptr) {{\n            return true;\n        }}\n        std::cerr << \"FlowRT C component callback table invalid for instance `\" << instance_name << \"`: \" << error << '\\n';\n        return false;\n    }}\n\n    flowrt::Status call_lifecycle(flowrt_c_lifecycle_callback_t callback, std::string_view hook_name, std::string_view component_name, std::string_view instance_name, const flowrt::Context& runtime_context) {{\n        if (!callbacks_valid(instance_name)) {{\n            return flowrt::Status::Error;\n        }}\n        if (callback == nullptr) {{\n            return flowrt::Status::Ok;\n        }}\n        const auto context = make_c_component_context(component_name, instance_name, hook_name, std::string_view{{}}, runtime_context, 0U, 0U, 0U, false, flowrt_c_param_snapshot_v0_t{{}});\n        return status_from_c(callback(callbacks_->user_data, &context));\n    }}\n\n    const flowrt_c_component_callback_table_t* callbacks_ = nullptr;\n}};\n\nstd::unique_ptr<flowrt_app::{component}Interface> {factory_name}() {{\n    return std::make_unique<{class_name}>({symbol}());\n}}\n\n",
        component = component_cpp_name(component),
    ));
    output
}

pub(super) fn emit_c_adapter_lifecycle_method(
    name: &str,
    component_name: &str,
    instance_name: &str,
) -> String {
    format!(
        "    flowrt::Status {name}(flowrt::Context& context) override {{\n        return call_lifecycle(callbacks_ != nullptr ? callbacks_->{name} : nullptr, {hook}, {component}, {instance}, context);\n    }}\n\n",
        hook = cpp_string_literal(name),
        component = cpp_string_literal(component_name),
        instance = cpp_string_literal(instance_name),
    )
}

pub(super) fn emit_c_adapter_on_tick_override(component: &ComponentIr) -> String {
    let args = cpp_callback_args(component, &[], &[]);
    if args.is_empty() {
        return "    flowrt::Status on_tick() override {\n        return flowrt::Status::Error;\n    }\n\n"
            .to_string();
    }
    let joined = args
        .iter()
        .map(|arg| format!("        {arg}"))
        .collect::<Vec<_>>()
        .join(",\n");
    format!(
        "    flowrt::Status on_tick(\n{joined}) override {{\n        return flowrt::Status::Error;\n    }}\n\n"
    )
}

pub(super) fn emit_c_adapter_task_method(
    component: &ComponentIr,
    instance: &InstanceIr,
    task: &flowrt_ir::TaskIr,
) -> String {
    let mut args = vec![
        "std::uint64_t step".to_string(),
        "std::uint64_t tick_time_ms".to_string(),
        "flowrt::Context& tick_context".to_string(),
    ];
    for input in &component.inputs {
        args.push(format!("std::uint64_t {}_revision", input.name));
        args.push(format!(
            "const flowrt::Latest<{}>& {}",
            cpp_type(&input.ty),
            input.name
        ));
    }
    if !component.params.is_empty() {
        args.push(format!(
            "const {}Params& params",
            component_cpp_name(component)
        ));
    }
    for output in &component.outputs {
        args.push(format!(
            "flowrt::Output<{}>& {}",
            cpp_type(&output.ty),
            output.name
        ));
    }
    let joined = args
        .iter()
        .map(|arg| format!("        {arg}"))
        .collect::<Vec<_>>()
        .join(",\n");
    let callback_field = c_task_callback_field(task);
    let mut output = format!(
        "    flowrt::Status {method}(\n{joined}) {{\n        if (!callbacks_valid({instance_name}) || callbacks_->{callback_field} == nullptr) {{\n            return flowrt::Status::Error;\n        }}\n",
        method = c_adapter_task_method_name(task),
        instance_name = cpp_string_literal(&instance.name),
        callback_field = callback_field,
    );
    output.push_str(&emit_c_param_snapshot_vars(component, instance));
    output.push_str(&format!(
        "        const auto context = make_c_component_context({component_name}, {instance_name}, {task_name}, {lane_name}, tick_context, step, tick_time_ms, {deadline}, {has_deadline}, param_snapshot);\n",
        component_name = cpp_string_literal(&component.name),
        instance_name = cpp_string_literal(&instance.name),
        task_name = cpp_string_literal(&task.name),
        lane_name = cpp_string_literal(&cpp_task_lane_name(task)),
        deadline = task.deadline_ms.unwrap_or(0),
        has_deadline = c_bool_literal(task.deadline_ms.is_some()),
    ));

    let task_inputs = task
        .inputs
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let task_outputs = task
        .outputs
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();

    for input in component
        .inputs
        .iter()
        .filter(|input| task_inputs.contains(input.name.as_str()))
    {
        let input_name = crate::snake_identifier(&input.name);
        output.push_str(&format!(
            "        const auto* {input_name}_payload = {port}.get();\n        flowrt_c_input_view_t {instance}_{input_name}_input{{\n            .name = c_string_view({port_name}),\n            .type_name = c_string_view({type_name}),\n            .schema_hash = flowrt::fnv1a64({type_name}),\n            .size_bytes = sizeof({ty}),\n            .payload = {input_name}_payload != nullptr ? c_bytes_view({input_name}_payload, sizeof({ty})) : flowrt_bytes_view_t{{}},\n            .source_time_ms = tick_time_ms,\n            .revision = {port}_revision,\n            .present = {port}.present() ? std::uint8_t{{1}} : std::uint8_t{{0}},\n            .stale = {port}.stale() ? std::uint8_t{{1}} : std::uint8_t{{0}},\n            .reserved = {{}},\n        }};\n",
            instance = crate::snake_identifier(&instance.name),
            port = input.name,
            port_name = cpp_string_literal(&input.name),
            type_name = cpp_string_literal(&input.ty.canonical_syntax()),
            ty = cpp_type(&input.ty),
        ));
    }

    for output_port in component
        .outputs
        .iter()
        .filter(|output_port| task_outputs.contains(output_port.name.as_str()))
    {
        let output_name = crate::snake_identifier(&output_port.name);
        output.push_str(&format!(
            "        std::array<std::uint8_t, sizeof({ty})> {instance}_{output_name}_storage{{}};\n        flowrt_c_output_slot_t {instance}_{output_name}_output{{\n            .name = c_string_view({port_name}),\n            .type_name = c_string_view({type_name}),\n            .schema_hash = flowrt::fnv1a64({type_name}),\n            .size_bytes = sizeof({ty}),\n            .data = {instance}_{output_name}_storage.data(),\n            .capacity = {instance}_{output_name}_storage.size(),\n            .written_len = 0U,\n            .status = FLOWRT_C_OUTPUT_UNWRITTEN,\n            .reserved = {{}},\n        }};\n",
            instance = crate::snake_identifier(&instance.name),
            port_name = cpp_string_literal(&output_port.name),
            type_name = cpp_string_literal(&output_port.ty.canonical_syntax()),
            ty = cpp_type(&output_port.ty),
        ));
    }

    output.push_str(&emit_c_input_array(instance, component, &task_inputs));
    output.push_str(&emit_c_output_array(instance, component, &task_outputs));
    output.push_str(&format!(
        "        const auto callback_status = status_from_c(callbacks_->{callback_field}(callbacks_->user_data, &context, &inputs, &outputs));\n        if (callback_status != flowrt::Status::Ok) {{\n            return callback_status;\n        }}\n"
    ));
    for (index, output_port) in component
        .outputs
        .iter()
        .filter(|output_port| task_outputs.contains(output_port.name.as_str()))
        .enumerate()
    {
        let output_name = crate::snake_identifier(&output_port.name);
        output.push_str(&format!(
            "        {instance}_{output_name}_output = output_slots[{index}];\n",
            instance = crate::snake_identifier(&instance.name),
        ));
    }
    for output_port in component
        .outputs
        .iter()
        .filter(|output_port| task_outputs.contains(output_port.name.as_str()))
    {
        let output_name = crate::snake_identifier(&output_port.name);
        output.push_str(&format!(
            "        if ({instance}_{output_name}_output.status == FLOWRT_C_OUTPUT_WRITTEN && {instance}_{output_name}_output.written_len == sizeof({ty})) {{\n            {ty} {instance}_{output_name}_value{{}};\n            std::memcpy(&{instance}_{output_name}_value, {instance}_{output_name}_storage.data(), sizeof({ty}));\n            {port}.write({instance}_{output_name}_value);\n        }} else if ({instance}_{output_name}_output.status == FLOWRT_C_OUTPUT_UNWRITTEN) {{\n        }} else {{\n            std::cerr << \"FlowRT C component output invalid for instance `{instance_raw}`, port `{port}`: status=\"\n                      << {instance}_{output_name}_output.status << \" written_len=\"\n                      << {instance}_{output_name}_output.written_len << \" expected=\" << sizeof({ty}) << '\\n';\n            return flowrt::Status::Error;\n        }}\n",
            instance = crate::snake_identifier(&instance.name),
            instance_raw = instance.name,
            port = output_port.name,
            ty = cpp_type(&output_port.ty),
        ));
    }
    output.push_str("        return flowrt::Status::Ok;\n    }\n\n");

    output
}

pub(super) fn emit_c_param_snapshot_vars(component: &ComponentIr, instance: &InstanceIr) -> String {
    if component.params.is_empty() {
        return "        const auto param_snapshot = make_c_param_snapshot(nullptr, 0U);\n"
            .to_string();
    }

    let instance_name = crate::snake_identifier(&instance.name);
    let json_values = component
        .params
        .iter()
        .map(|param| format!("            c_param_json(params.{}),", param.name))
        .collect::<Vec<_>>()
        .join("\n");
    let views = component
        .params
        .iter()
        .enumerate()
        .map(|(index, param)| {
            format!(
                "            flowrt_param_view_t{{\n                .instance_name = c_string_view({instance_literal}),\n                .param_name = c_string_view({param_literal}),\n                .type_name = c_string_view({type_literal}),\n                .update_policy = c_string_view({update_literal}),\n                .current_json = c_string_view({instance_name}_param_json[{index}]),\n                .pending_json = flowrt_string_view_t{{}},\n                .min_json = {min_json},\n                .max_json = {max_json},\n                .choices_json = {choices_json},\n                .schema_hash = flowrt::fnv1a64({runtime_literal}),\n                .revision = 0U,\n                .mutable_at_runtime = {mutable_at_runtime},\n                .has_pending = 0U,\n                .has_min = {has_min},\n                .has_max = {has_max},\n                .reserved = {{}},\n            }},",
                instance_literal = cpp_string_literal(&instance.name),
                param_literal = cpp_string_literal(&param.name),
                type_literal = cpp_string_literal(crate::param_type_name(param.ty)),
                update_literal = cpp_string_literal(crate::param_update_name(param.update)),
                min_json = c_param_json_view_literal(param.min.as_ref()),
                max_json = c_param_json_view_literal(param.max.as_ref()),
                choices_json = c_param_choices_view_literal(&param.choices),
                runtime_literal =
                    cpp_string_literal(&crate::runtime_plan::runtime_param_name(instance, param)),
                mutable_at_runtime = if param.update == ParamUpdatePolicy::OnTick {
                    "1U"
                } else {
                    "0U"
                },
                has_min = if param.min.is_some() { "1U" } else { "0U" },
                has_max = if param.max.is_some() { "1U" } else { "0U" },
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "        std::array<std::string, {len}> {instance_name}_param_json{{{{\n{json_values}\n        }}}};\n        std::array<flowrt_param_view_t, {len}> {instance_name}_param_views{{{{\n{views}\n        }}}};\n        const auto param_snapshot = make_c_param_snapshot({instance_name}_param_views.data(), {instance_name}_param_views.size());\n",
        len = component.params.len(),
    )
}

fn c_param_json_view_literal(value: Option<&ParamValue>) -> String {
    value
        .map(|value| {
            format!(
                "c_string_view({})",
                cpp_string_literal(&param_json_literal(value))
            )
        })
        .unwrap_or_else(|| "flowrt_string_view_t{}".to_string())
}

fn c_param_choices_view_literal(values: &[ParamValue]) -> String {
    if values.is_empty() {
        return "flowrt_string_view_t{}".to_string();
    }
    let json = format!(
        "[{}]",
        values
            .iter()
            .map(param_json_literal)
            .collect::<Vec<_>>()
            .join(",")
    );
    format!("c_string_view({})", cpp_string_literal(&json))
}

pub(super) fn emit_c_input_array(
    instance: &InstanceIr,
    component: &ComponentIr,
    task_inputs: &BTreeSet<&str>,
) -> String {
    let inputs = component
        .inputs
        .iter()
        .filter(|input| task_inputs.contains(input.name.as_str()))
        .map(|input| {
            format!(
                "{}_{}_input",
                crate::snake_identifier(&instance.name),
                crate::snake_identifier(&input.name)
            )
        })
        .collect::<Vec<_>>();
    if inputs.is_empty() {
        return "        std::array<flowrt_c_input_view_t, 0> input_views{};\n        flowrt_c_input_array_view_t inputs{.data = nullptr, .len = 0U};\n"
            .to_string();
    }
    let body = inputs
        .iter()
        .map(|input| format!("            {input},"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "        std::array<flowrt_c_input_view_t, {len}> input_views{{{{\n{body}\n        }}}};\n        flowrt_c_input_array_view_t inputs{{.data = input_views.data(), .len = input_views.size()}};\n",
        len = inputs.len(),
    )
}

pub(super) fn emit_c_output_array(
    instance: &InstanceIr,
    component: &ComponentIr,
    task_outputs: &BTreeSet<&str>,
) -> String {
    let outputs = component
        .outputs
        .iter()
        .filter(|output| task_outputs.contains(output.name.as_str()))
        .map(|output| {
            format!(
                "{}_{}_output",
                crate::snake_identifier(&instance.name),
                crate::snake_identifier(&output.name)
            )
        })
        .collect::<Vec<_>>();
    if outputs.is_empty() {
        return "        std::array<flowrt_c_output_slot_t, 0> output_slots{};\n        flowrt_c_output_array_view_t outputs{.data = nullptr, .len = 0U};\n"
            .to_string();
    }
    let body = outputs
        .iter()
        .map(|output| format!("            {output},"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "        std::array<flowrt_c_output_slot_t, {len}> output_slots{{{{\n{body}\n        }}}};\n        flowrt_c_output_array_view_t outputs{{.data = output_slots.data(), .len = output_slots.size()}};\n",
        len = outputs.len(),
    )
}

pub(super) fn emit_c_adapter_task_step_call(
    emission: &CppStepEmission<'_>,
    instance: &InstanceIr,
    component: &ComponentIr,
    task: &flowrt_ir::TaskIr,
    collect_outputs: bool,
    body_indent: &str,
    body_inner_indent: &str,
) -> String {
    let mut args = vec![
        "static_cast<std::uint64_t>(tick)".to_string(),
        "tick_time_ms".to_string(),
        "tick_context".to_string(),
    ];
    for input in &component.inputs {
        args.push(c_input_revision_expr(emission, instance, input));
        args.push(cpp_step_local_name(&instance.name, &input.name));
    }
    if !component.params.is_empty() {
        args.push(format!("{}_params_", instance.name));
    }
    for output in &component.outputs {
        args.push(cpp_step_local_name(&instance.name, &output.name));
    }
    let call = format!(
        "static_cast<{}*>({}_.get())->{}({})",
        c_adapter_class_name(instance),
        instance.name,
        c_adapter_task_method_name(task),
        args.join(", ")
    );

    if collect_outputs {
        format!(
            "{body_indent}if ({instance}_) {{\n{body_inner_indent}switch ({call}) {{\n{body_inner_indent}    case flowrt::Status::Ok:\n{body_inner_indent}        break;\n{body_inner_indent}    case flowrt::Status::Retry:\n{body_inner_indent}        return FlowrtTaskOutcome::retry(std::vector<FlowrtOutputCommit>{{}});\n{body_inner_indent}    case flowrt::Status::Error:\n{body_inner_indent}        return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{{}});\n{body_inner_indent}}}\n{body_indent}}}\n",
            instance = instance.name,
        )
    } else {
        format!(
            "{body_indent}if ({instance}_) {{\n{body_inner_indent}switch ({call}) {{\n{body_inner_indent}    case flowrt::Status::Ok:\n{body_inner_indent}        break;\n{body_inner_indent}    case flowrt::Status::Retry:\n{body_inner_indent}        return flowrt::Status::Retry;\n{body_inner_indent}    case flowrt::Status::Error:\n{body_inner_indent}        return flowrt::Status::Error;\n{body_inner_indent}}}\n{body_indent}}}\n",
            instance = instance.name,
        )
    }
}

pub(super) fn c_input_revision_expr(
    emission: &CppStepEmission<'_>,
    instance: &InstanceIr,
    input: &PortIr,
) -> String {
    if let Some(bind_index) = emission
        .incoming_bind_index
        .get(&(instance.name.clone(), input.name.clone()))
    {
        return format!("{}_.revision()", emission.binds[*bind_index].field_name);
    }
    if let Some(bridge_index) = emission
        .incoming_bridge_index
        .get(&(instance.name.clone(), input.name.clone()))
    {
        return format!("{}_.revision()", emission.bridges[*bridge_index].field_name);
    }
    if let Some(boundary_index) = emission
        .incoming_boundary_index
        .get(&(instance.name.clone(), input.name.clone()))
    {
        return format!(
            "{}_.revision()",
            emission.boundaries[*boundary_index].field_name
        );
    }
    "0U".to_string()
}

pub(super) fn c_task_callback_field(task: &flowrt_ir::TaskIr) -> &'static str {
    match task.trigger {
        flowrt_ir::TriggerKind::Periodic => "run_periodic",
        flowrt_ir::TriggerKind::OnMessage => "run_on_message",
        flowrt_ir::TriggerKind::Startup => "run_startup",
        flowrt_ir::TriggerKind::Shutdown => "run_shutdown",
        flowrt_ir::TriggerKind::OnSynchronized => "run_on_synchronized",
    }
}

pub(super) fn c_adapter_class_name(instance: &InstanceIr) -> String {
    format!(
        "C{}Adapter",
        crate::pascal_case(&crate::snake_identifier(&instance.name))
    )
}

pub(super) fn c_adapter_factory_name(instance: &InstanceIr) -> String {
    format!("make_c_{}_adapter", crate::snake_identifier(&instance.name))
}

pub(super) fn c_adapter_task_method_name(task: &flowrt_ir::TaskIr) -> String {
    format!("run_{}", cpp_task_local_name(task))
}

pub(super) fn c_callback_factory_symbol(instance: &InstanceIr) -> String {
    format!(
        "flowrt_app_{}_callbacks",
        crate::snake_identifier(&instance.name)
    )
}

pub(super) fn c_bool_literal(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}
