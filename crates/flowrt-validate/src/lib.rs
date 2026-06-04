//! normalized Contract IR 的校验 passes。
//!
//! 本 crate 只校验已经归一化后的 IR，不直接读取 RSDL 源文本。校验失败时会聚合多个错误，
//! 便于 CLI 一次性报告 contract 中的结构问题。

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{Display, Formatter};

use flowrt_conformance::message_abi_expectations;
use flowrt_ir::{
    CONTRACT_IR_VERSION, CONTRACT_SCHEMA_VERSION, ChannelKind, ComponentIr, ComponentKind,
    ContractIr, EntityId, EntityRef, GraphIr, InstanceIr, LanguageKind, PortIr, PortRef,
    RSDL_VERSION, TaskIr, TriggerKind, TypeExpr, backend_capabilities, is_known_backend,
};

/// validation passes 返回的结果类型。
pub type Result<T> = std::result::Result<T, ValidationReport>;

/// validation report，可同时包含多个 contract 错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationReport {
    pub errors: Vec<ValidationError>,
}

impl ValidationReport {
    /// 判断报告是否不包含任何错误。
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }
}

impl Display for ValidationReport {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(
            formatter,
            "validation failed with {} error(s)",
            self.errors.len()
        )?;
        for error in &self.errors {
            writeln!(formatter, "- {}", error.message)?;
        }
        Ok(())
    }
}

impl Error for ValidationReport {}

/// 单个 contract 校验错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub message: String,
}

impl ValidationError {
    /// 构造一个校验错误。
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// 校验一个 normalized Contract IR 文档。
pub fn validate_contract(ir: &ContractIr) -> Result<()> {
    let mut errors = Vec::new();

    let type_names = ir
        .types
        .iter()
        .map(|ty| ty.name.as_str())
        .collect::<BTreeSet<_>>();
    validate_contract_versions(ir, &mut errors);
    validate_contract_shape(ir, &mut errors);
    validate_entity_name_uniqueness(ir, &mut errors);
    validate_entity_id_uniqueness(ir, &mut errors);
    validate_entity_references(ir, &mut errors);
    validate_names(ir, &mut errors);
    validate_message_types(ir, &type_names, &mut errors);
    validate_message_abi(ir, &mut errors);
    validate_components(ir, &type_names, &mut errors);
    validate_graphs(ir, &mut errors);
    validate_declared_backends(ir, &mut errors);
    validate_deployments(ir, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(ValidationReport { errors })
    }
}

fn validate_contract_versions(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    if ir.ir_version != CONTRACT_IR_VERSION {
        errors.push(ValidationError::new(format!(
            "unsupported Contract IR version `{}`; expected `{CONTRACT_IR_VERSION}`",
            ir.ir_version
        )));
    }
    if ir.schema_version != CONTRACT_SCHEMA_VERSION {
        errors.push(ValidationError::new(format!(
            "unsupported Contract IR schema version `{}`; expected `{CONTRACT_SCHEMA_VERSION}`",
            ir.schema_version
        )));
    }
    if ir.package.rsdl_version != RSDL_VERSION {
        errors.push(ValidationError::new(format!(
            "unsupported RSDL version `{}`; expected `{RSDL_VERSION}`",
            ir.package.rsdl_version
        )));
    }
}

fn validate_contract_shape(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    if ir.graphs.len() != 1 {
        errors.push(ValidationError::new(format!(
            "Contract IR v0.1 must contain exactly one graph; found {}",
            ir.graphs.len()
        )));
    }
}

fn validate_entity_name_uniqueness(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    validate_unique_names(
        "contract",
        "type",
        ir.types.iter().map(|ty| ty.name.as_str()),
        errors,
    );
    validate_unique_names(
        "contract",
        "component",
        ir.components
            .iter()
            .map(|component| component.name.as_str()),
        errors,
    );
    validate_unique_names(
        "contract",
        "profile",
        ir.profiles.iter().map(|profile| profile.name.as_str()),
        errors,
    );
    validate_unique_names(
        "contract",
        "target",
        ir.targets.iter().map(|target| target.name.as_str()),
        errors,
    );
    validate_unique_names(
        "contract",
        "graph",
        ir.graphs.iter().map(|graph| graph.name.as_str()),
        errors,
    );

    for graph in &ir.graphs {
        validate_unique_names(
            &format!("graph `{}`", graph.name),
            "instance",
            graph
                .instances
                .iter()
                .map(|instance| instance.name.as_str()),
            errors,
        );
    }
}

fn validate_entity_id_uniqueness(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    let mut seen = BTreeMap::<&EntityId, String>::new();
    record_entity_id(
        &mut seen,
        &ir.package_id,
        format!("package `{}`", ir.package.name),
        errors,
    );
    for ty in &ir.types {
        record_entity_id(&mut seen, &ty.id, format!("type `{}`", ty.name), errors);
    }
    for component in &ir.components {
        record_entity_id(
            &mut seen,
            &component.id,
            format!("component `{}`", component.name),
            errors,
        );
    }
    for graph in &ir.graphs {
        record_entity_id(
            &mut seen,
            &graph.id,
            format!("graph `{}`", graph.name),
            errors,
        );
        for instance in &graph.instances {
            record_entity_id(
                &mut seen,
                &instance.id,
                format!("instance `{}`", instance.name),
                errors,
            );
        }
        for task in &graph.tasks {
            record_entity_id(
                &mut seen,
                &task.id,
                format!("task on instance `{}`", task.instance.name),
                errors,
            );
        }
        for bind in &graph.binds {
            record_entity_id(
                &mut seen,
                &bind.id,
                format!(
                    "bind `{}.{}` -> `{}.{}`",
                    bind.from.instance.name, bind.from.port, bind.to.instance.name, bind.to.port
                ),
                errors,
            );
        }
    }
    for profile in &ir.profiles {
        record_entity_id(
            &mut seen,
            &profile.id,
            format!("profile `{}`", profile.name),
            errors,
        );
    }
    for target in &ir.targets {
        record_entity_id(
            &mut seen,
            &target.id,
            format!("target `{}`", target.name),
            errors,
        );
    }
    for deployment in &ir.deployments {
        record_entity_id(
            &mut seen,
            &deployment.id,
            format!(
                "deployment `{}` / `{}` / `{}`",
                deployment.graph.name, deployment.profile.name, deployment.target.name
            ),
            errors,
        );
    }
}

fn record_entity_id<'a>(
    seen: &mut BTreeMap<&'a EntityId, String>,
    id: &'a EntityId,
    description: String,
    errors: &mut Vec<ValidationError>,
) {
    if let Some(previous) = seen.insert(id, description.clone()) {
        errors.push(ValidationError::new(format!(
            "contract has duplicate entity ID `{}` shared by {previous} and {description}",
            id.0
        )));
    }
}

fn validate_entity_references(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    let component_ids = ir
        .components
        .iter()
        .map(|component| (component.name.as_str(), &component.id))
        .collect::<BTreeMap<_, _>>();
    let graph_ids = ir
        .graphs
        .iter()
        .map(|graph| (graph.name.as_str(), &graph.id))
        .collect::<BTreeMap<_, _>>();
    let profile_ids = ir
        .profiles
        .iter()
        .map(|profile| (profile.name.as_str(), &profile.id))
        .collect::<BTreeMap<_, _>>();
    let target_ids = ir
        .targets
        .iter()
        .map(|target| (target.name.as_str(), &target.id))
        .collect::<BTreeMap<_, _>>();

    for graph in &ir.graphs {
        let instance_ids = graph
            .instances
            .iter()
            .map(|instance| (instance.name.as_str(), &instance.id))
            .collect::<BTreeMap<_, _>>();

        for instance in &graph.instances {
            validate_named_entity_ref(
                &format!("instance `{}` component reference", instance.name),
                "component",
                &instance.component,
                &component_ids,
                errors,
            );
            if let Some(target) = &instance.target {
                validate_named_entity_ref(
                    &format!("instance `{}` target reference", instance.name),
                    "target",
                    target,
                    &target_ids,
                    errors,
                );
            }
        }

        for task in &graph.tasks {
            validate_named_entity_ref(
                &format!(
                    "task on instance `{}` instance reference",
                    task.instance.name
                ),
                "instance",
                &task.instance,
                &instance_ids,
                errors,
            );
        }

        for bind in &graph.binds {
            validate_named_entity_ref(
                "bind source instance reference",
                "instance",
                &bind.from.instance,
                &instance_ids,
                errors,
            );
            validate_named_entity_ref(
                "bind target instance reference",
                "instance",
                &bind.to.instance,
                &instance_ids,
                errors,
            );
        }
    }

    for deployment in &ir.deployments {
        validate_named_entity_ref(
            "deployment graph reference",
            "graph",
            &deployment.graph,
            &graph_ids,
            errors,
        );
        validate_named_entity_ref(
            "deployment profile reference",
            "profile",
            &deployment.profile,
            &profile_ids,
            errors,
        );
        validate_named_entity_ref(
            "deployment target reference",
            "target",
            &deployment.target,
            &target_ids,
            errors,
        );
    }
}

fn validate_named_entity_ref(
    context: &str,
    entity_kind: &str,
    reference: &EntityRef,
    known: &BTreeMap<&str, &EntityId>,
    errors: &mut Vec<ValidationError>,
) {
    let Some(expected_id) = known.get(reference.name.as_str()) else {
        errors.push(ValidationError::new(format!(
            "{context} references unknown {entity_kind} `{}`",
            reference.name
        )));
        return;
    };

    if *expected_id != &reference.id {
        errors.push(ValidationError::new(format!(
            "{context} points to {entity_kind} `{}` with ID `{}`, expected ID `{}`",
            reference.name, reference.id.0, expected_id.0
        )));
    }
}

fn validate_unique_names<'a>(
    scope: &str,
    entity_kind: &str,
    names: impl IntoIterator<Item = &'a str>,
    errors: &mut Vec<ValidationError>,
) {
    let mut seen = BTreeSet::new();
    for name in names {
        if !seen.insert(name) {
            errors.push(ValidationError::new(format!(
                "{scope} has duplicate {entity_kind} name `{name}`"
            )));
        }
    }
}

fn validate_message_abi(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    if let Err(error) = message_abi_expectations(ir) {
        errors.push(ValidationError::new(format!(
            "message ABI v0.1 violation: {error}"
        )));
    }
}

fn validate_names(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    validate_name(
        "package",
        "package name",
        &ir.package.name,
        NameStyle::SnakeCase,
        errors,
    );

    for ty in &ir.types {
        validate_name("type", "type name", &ty.name, NameStyle::PascalCase, errors);
        for field in &ty.fields {
            validate_name(
                "field",
                "field name",
                &field.name,
                NameStyle::SnakeCase,
                errors,
            );
        }
    }

    for component in &ir.components {
        validate_name(
            "component",
            "component name",
            &component.name,
            NameStyle::SnakeCase,
            errors,
        );
        for port in component.inputs.iter().chain(component.outputs.iter()) {
            validate_name(
                "port",
                "port name",
                &port.name,
                NameStyle::SnakeCase,
                errors,
            );
        }
    }

    for profile in &ir.profiles {
        validate_name(
            "profile",
            "profile name",
            &profile.name,
            NameStyle::SnakeCase,
            errors,
        );
    }

    for target in &ir.targets {
        validate_name(
            "target",
            "target name",
            &target.name,
            NameStyle::SnakeCase,
            errors,
        );
    }

    for graph in &ir.graphs {
        validate_name(
            "graph",
            "graph name",
            &graph.name,
            NameStyle::SnakeCase,
            errors,
        );
        for instance in &graph.instances {
            validate_name(
                "instance",
                "instance name",
                &instance.name,
                NameStyle::SnakeCase,
                errors,
            );
            if let Some(process) = &instance.process {
                validate_name(
                    "process",
                    "process name",
                    process,
                    NameStyle::SnakeCase,
                    errors,
                );
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum NameStyle {
    SnakeCase,
    PascalCase,
}

impl NameStyle {
    fn label(self) -> &'static str {
        match self {
            NameStyle::SnakeCase => "snake_case",
            NameStyle::PascalCase => "PascalCase",
        }
    }

    fn accepts(self, name: &str) -> bool {
        match self {
            NameStyle::SnakeCase => is_snake_case(name),
            NameStyle::PascalCase => is_pascal_case(name),
        }
    }
}

fn validate_name(
    entity_kind: &'static str,
    label: &'static str,
    name: &str,
    style: NameStyle,
    errors: &mut Vec<ValidationError>,
) {
    if !style.accepts(name) {
        errors.push(ValidationError::new(format!(
            "{label} `{name}` must be {}",
            style.label()
        )));
    }
    if name.starts_with("flowrt") {
        errors.push(ValidationError::new(format!(
            "{entity_kind} name `{name}` uses reserved `flowrt` prefix"
        )));
    }
}

fn is_snake_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }

    let mut previous_underscore = false;
    for ch in chars {
        match ch {
            '_' if !previous_underscore => previous_underscore = true,
            '_' => return false,
            'a'..='z' | '0'..='9' => previous_underscore = false,
            _ => return false,
        }
    }
    !previous_underscore
}

fn is_pascal_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_uppercase() {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric()) && name.chars().any(|ch| ch.is_ascii_lowercase())
}

fn validate_message_types(
    ir: &ContractIr,
    type_names: &BTreeSet<&str>,
    errors: &mut Vec<ValidationError>,
) {
    for ty in &ir.types {
        if ty.fields.is_empty() {
            errors.push(ValidationError::new(format!(
                "type `{}` must declare at least one field",
                ty.name
            )));
        }

        let mut fields = BTreeSet::new();
        for field in &ty.fields {
            if !fields.insert(field.name.as_str()) {
                errors.push(ValidationError::new(format!(
                    "type `{}` has duplicate field `{}`",
                    ty.name, field.name
                )));
            }
            validate_type_expr(
                &field.ty,
                type_names,
                &format!("type `{}` field `{}`", ty.name, field.name),
                errors,
            );
        }
    }
}

fn validate_components(
    ir: &ContractIr,
    type_names: &BTreeSet<&str>,
    errors: &mut Vec<ValidationError>,
) {
    for component in &ir.components {
        if component.kind == ComponentKind::External {
            errors.push(ValidationError::new(format!(
                "component `{}` uses external process kind, which is not supported by Contract IR v0.1 runtime shell",
                component.name
            )));
        }

        let mut ports = BTreeSet::new();
        for port in component.inputs.iter().chain(component.outputs.iter()) {
            if !ports.insert(port.name.as_str()) {
                errors.push(ValidationError::new(format!(
                    "component `{}` has duplicate port `{}`",
                    component.name, port.name
                )));
            }
            validate_type_expr(
                &port.ty,
                type_names,
                &format!("component `{}` port `{}`", component.name, port.name),
                errors,
            );
        }
    }
}

fn validate_graphs(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    let components = ir
        .components
        .iter()
        .map(|component| (component.name.as_str(), component))
        .collect::<BTreeMap<_, _>>();
    let targets = ir
        .targets
        .iter()
        .map(|target| (target.id.clone(), target))
        .collect::<BTreeMap<_, _>>();

    for graph in &ir.graphs {
        let instances = graph
            .instances
            .iter()
            .map(|instance| (instance.name.as_str(), instance))
            .collect::<BTreeMap<_, _>>();

        validate_instance_targets(&components, &targets, graph, errors);
        validate_process_targets(graph, errors);
        validate_tasks(&components, &instances, graph, errors);
        validate_binds(&components, &instances, graph, errors);
        validate_graph_is_acyclic(&instances, graph, errors);
    }
}

fn validate_instance_targets(
    components: &BTreeMap<&str, &ComponentIr>,
    targets: &BTreeMap<EntityId, &flowrt_ir::TargetIr>,
    graph: &GraphIr,
    errors: &mut Vec<ValidationError>,
) {
    for instance in &graph.instances {
        let Some(component) = components.get(instance.component.name.as_str()) else {
            errors.push(ValidationError::new(format!(
                "instance `{}` references unknown component `{}`",
                instance.name, instance.component.name
            )));
            continue;
        };

        let Some(target) = &instance.target else {
            continue;
        };
        let Some(target) = targets.get(&target.id) else {
            errors.push(ValidationError::new(format!(
                "instance `{}` references unknown target `{}`",
                instance.name, target.name
            )));
            continue;
        };
        if !target.runtime.contains(&component.language) {
            errors.push(ValidationError::new(format!(
                "target `{}` does not support {:?} runtime required by instance `{}`",
                target.name, component.language, instance.name
            )));
        }
    }
}

fn validate_process_targets(graph: &GraphIr, errors: &mut Vec<ValidationError>) {
    let mut process_targets = BTreeMap::<String, BTreeSet<String>>::new();

    for instance in &graph.instances {
        let process = instance.process.as_deref().unwrap_or("main");
        let target = instance
            .target
            .as_ref()
            .map(|target| target.name.as_str())
            .unwrap_or("default");
        process_targets
            .entry(process.to_string())
            .or_default()
            .insert(target.to_string());
    }

    for (process, targets) in process_targets {
        if targets.len() > 1 {
            errors.push(ValidationError::new(format!(
                "process `{}` spans multiple targets: {}",
                process,
                targets.into_iter().collect::<Vec<_>>().join(", ")
            )));
        }
    }
}

fn validate_tasks(
    components: &BTreeMap<&str, &ComponentIr>,
    instances: &BTreeMap<&str, &InstanceIr>,
    graph: &GraphIr,
    errors: &mut Vec<ValidationError>,
) {
    let mut task_instances = BTreeSet::new();
    let incoming_binds = graph
        .binds
        .iter()
        .map(|bind| (bind.to.instance.id.clone(), bind.to.port.as_str()))
        .collect::<BTreeSet<_>>();

    for task in &graph.tasks {
        if !task_instances.insert(task.instance.id.clone()) {
            errors.push(ValidationError::new(format!(
                "instance `{}` has multiple tasks in Contract IR v0.1",
                task.instance.name
            )));
        }

        let Some(instance) = instances.get(task.instance.name.as_str()) else {
            errors.push(ValidationError::new(format!(
                "task references unknown instance `{}`",
                task.instance.name
            )));
            continue;
        };
        let Some(component) = components.get(instance.component.name.as_str()) else {
            continue;
        };

        if task.trigger == TriggerKind::Periodic && task.period_ms.is_none() {
            errors.push(ValidationError::new(format!(
                "periodic task on instance `{}` must set period_ms",
                instance.name
            )));
        }
        if task.trigger == TriggerKind::Periodic && task.period_ms == Some(0) {
            errors.push(ValidationError::new(format!(
                "periodic task on instance `{}` must set period_ms greater than zero",
                instance.name
            )));
        }
        if task.trigger != TriggerKind::Periodic && task.period_ms.is_some() {
            errors.push(ValidationError::new(format!(
                "task on instance `{}` must not set period_ms unless trigger is periodic",
                instance.name
            )));
        }
        if task.trigger == TriggerKind::OnMessage && task.inputs.is_empty() {
            errors.push(ValidationError::new(format!(
                "on_message task on instance `{}` must list at least one input",
                instance.name
            )));
        }

        validate_task_ports(task, component, errors);
        validate_task_input_binds(task, component, &incoming_binds, errors);
    }
}

fn validate_task_ports(task: &TaskIr, component: &ComponentIr, errors: &mut Vec<ValidationError>) {
    let input_ports = component
        .inputs
        .iter()
        .map(|port| port.name.as_str())
        .collect::<BTreeSet<_>>();
    let output_ports = component
        .outputs
        .iter()
        .map(|port| port.name.as_str())
        .collect::<BTreeSet<_>>();

    let mut listed_inputs = BTreeSet::new();
    for input in &task.inputs {
        if !listed_inputs.insert(input.as_str()) {
            errors.push(ValidationError::new(format!(
                "task on instance `{}` lists input port `{}` more than once",
                task.instance.name, input
            )));
        }
        if !input_ports.contains(input.as_str()) {
            errors.push(ValidationError::new(format!(
                "task on instance `{}` references undeclared input port `{}`",
                task.instance.name, input
            )));
        }
    }
    let mut listed_outputs = BTreeSet::new();
    for output in &task.outputs {
        if !listed_outputs.insert(output.as_str()) {
            errors.push(ValidationError::new(format!(
                "task on instance `{}` lists output port `{}` more than once",
                task.instance.name, output
            )));
        }
        if !output_ports.contains(output.as_str()) {
            errors.push(ValidationError::new(format!(
                "task on instance `{}` references undeclared output port `{}`",
                task.instance.name, output
            )));
        }
    }
}

fn validate_task_input_binds(
    task: &TaskIr,
    component: &ComponentIr,
    incoming_binds: &BTreeSet<(EntityId, &str)>,
    errors: &mut Vec<ValidationError>,
) {
    let input_ports = component
        .inputs
        .iter()
        .map(|port| port.name.as_str())
        .collect::<BTreeSet<_>>();

    for input in &task.inputs {
        if !input_ports.contains(input.as_str()) {
            continue;
        }
        if !incoming_binds.contains(&(task.instance.id.clone(), input.as_str())) {
            errors.push(ValidationError::new(format!(
                "task input `{}.{}` has no incoming bind",
                task.instance.name, input
            )));
        }
    }
}

fn validate_binds(
    components: &BTreeMap<&str, &ComponentIr>,
    instances: &BTreeMap<&str, &InstanceIr>,
    graph: &GraphIr,
    errors: &mut Vec<ValidationError>,
) {
    let mut incoming = BTreeSet::new();
    for bind in &graph.binds {
        validate_channel_depth(bind.channel, bind.depth, &bind.to, errors);

        let from_port = resolve_port(components, instances, &bind.from, PortDirection::Output);
        let to_port = resolve_port(components, instances, &bind.to, PortDirection::Input);

        match (&from_port, &to_port) {
            (Ok(from_port), Ok(to_port)) if from_port.ty != to_port.ty => {
                errors.push(ValidationError::new(format!(
                    "bind `{}.{}` -> `{}.{}` has mismatched types",
                    bind.from.instance.name, bind.from.port, bind.to.instance.name, bind.to.port
                )));
            }
            (Err(message), _) => errors.push(ValidationError::new(message.clone())),
            (_, Err(message)) => errors.push(ValidationError::new(message.clone())),
            _ => {}
        }

        let key = (bind.to.instance.id.clone(), bind.to.port.clone());
        if !incoming.insert(key) {
            errors.push(ValidationError::new(format!(
                "input port `{}.{}` has multiple incoming binds",
                bind.to.instance.name, bind.to.port
            )));
        }
    }
}

fn validate_graph_is_acyclic(
    instances: &BTreeMap<&str, &InstanceIr>,
    graph: &GraphIr,
    errors: &mut Vec<ValidationError>,
) {
    let mut indegree = instances
        .keys()
        .map(|name| ((*name).to_string(), 0usize))
        .collect::<BTreeMap<_, _>>();
    let mut edges = BTreeMap::<String, BTreeSet<String>>::new();
    let mut self_loops = BTreeSet::<String>::new();

    for bind in &graph.binds {
        let source = bind.from.instance.name.as_str();
        let target = bind.to.instance.name.as_str();
        if !instances.contains_key(source) || !instances.contains_key(target) {
            continue;
        }

        if source == target {
            self_loops.insert(source.to_string());
            continue;
        }

        let inserted = edges
            .entry(source.to_string())
            .or_default()
            .insert(target.to_string());
        if inserted {
            *indegree
                .get_mut(target)
                .expect("known instance must have an indegree entry") += 1;
        }
    }

    for instance in self_loops {
        errors.push(ValidationError::new(format!(
            "graph `{}` has a dataflow self-loop on instance `{}`",
            graph.name, instance
        )));
    }

    let mut ready = indegree
        .iter()
        .filter_map(|(name, degree)| (*degree == 0).then_some(name.clone()))
        .collect::<BTreeSet<_>>();
    let mut visited = 0usize;

    while let Some(name) = ready.iter().next().cloned() {
        ready.remove(&name);
        visited += 1;

        if let Some(next) = edges.get(&name) {
            for target in next {
                let degree = indegree
                    .get_mut(target)
                    .expect("known instance must have an indegree entry");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(target.clone());
                }
            }
        }
    }

    if visited != instances.len() {
        let cycle_instances = indegree
            .into_iter()
            .filter_map(|(name, degree)| (degree > 0).then_some(format!("`{name}`")))
            .collect::<Vec<_>>()
            .join(", ");
        errors.push(ValidationError::new(format!(
            "graph `{}` has a dataflow cycle involving {}",
            graph.name, cycle_instances
        )));
    }
}

fn validate_channel_depth(
    channel: ChannelKind,
    depth: Option<u32>,
    to: &PortRef,
    errors: &mut Vec<ValidationError>,
) {
    match (channel, depth) {
        (ChannelKind::Latest, Some(depth)) if depth != 1 => {
            errors.push(ValidationError::new(format!(
                "latest channel to `{}.{}` must omit depth or set depth = 1",
                to.instance.name, to.port
            )));
        }
        (ChannelKind::Fifo, Some(0)) => {
            errors.push(ValidationError::new(format!(
                "channel to `{}.{}` has zero depth",
                to.instance.name, to.port
            )));
        }
        (ChannelKind::Fifo, None) => {
            errors.push(ValidationError::new(format!(
                "fifo channel to `{}.{}` must set depth",
                to.instance.name, to.port
            )));
        }
        _ => {}
    }
}

fn resolve_port<'a>(
    components: &'a BTreeMap<&str, &'a ComponentIr>,
    instances: &BTreeMap<&str, &InstanceIr>,
    endpoint: &PortRef,
    direction: PortDirection,
) -> std::result::Result<&'a PortIr, String> {
    let instance = instances
        .get(endpoint.instance.name.as_str())
        .ok_or_else(|| format!("unknown instance `{}`", endpoint.instance.name))?;
    let component = components
        .get(instance.component.name.as_str())
        .ok_or_else(|| format!("unknown component `{}`", instance.component.name))?;
    let ports = match direction {
        PortDirection::Input => &component.inputs,
        PortDirection::Output => &component.outputs,
    };
    ports
        .iter()
        .find(|port| port.name == endpoint.port)
        .ok_or_else(|| {
            format!(
                "instance `{}` component `{}` has no {:?} port `{}`",
                instance.name, component.name, direction, endpoint.port
            )
        })
}

#[derive(Debug, Clone, Copy)]
enum PortDirection {
    Input,
    Output,
}

fn validate_deployments(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    for deployment in &ir.deployments {
        if !is_known_backend(&deployment.backend.0) {
            errors.push(ValidationError::new(format!(
                "deployment for graph `{}` selects unknown backend `{}`",
                deployment.graph.name, deployment.backend.0
            )));
            continue;
        }

        let Some(backend_caps) = backend_capabilities(&deployment.backend.0) else {
            continue;
        };
        let missing_caps = deployment
            .required_capabilities
            .iter()
            .filter(|capability| !backend_caps.contains(capability))
            .collect::<Vec<_>>();

        if !deployment.satisfied || !missing_caps.is_empty() {
            errors.push(ValidationError::new(format!(
                "target `{}` does not support backend `{}` selected by profile `{}`",
                deployment.target.name, deployment.backend.0, deployment.profile.name
            )));
        }
    }
}

fn validate_declared_backends(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    for profile in &ir.profiles {
        if !is_known_backend(&profile.backend.0) {
            errors.push(ValidationError::new(format!(
                "profile `{}` selects unknown backend `{}`",
                profile.name, profile.backend.0
            )));
        }
    }

    for target in &ir.targets {
        for backend in &target.backends {
            if !is_known_backend(&backend.0) {
                errors.push(ValidationError::new(format!(
                    "target `{}` declares unknown backend `{}`",
                    target.name, backend.0
                )));
            }
        }
    }
}

fn validate_type_expr(
    expr: &TypeExpr,
    type_names: &BTreeSet<&str>,
    context: &str,
    errors: &mut Vec<ValidationError>,
) {
    match expr {
        TypeExpr::Primitive { .. } => {}
        TypeExpr::Named { name } => {
            if !type_names.contains(name.as_str()) {
                errors.push(ValidationError::new(format!(
                    "{context} references unknown type `{name}`"
                )));
            }
        }
        TypeExpr::Array { element, .. } => {
            validate_type_expr(element, type_names, context, errors);
        }
    }
}

#[allow(dead_code)]
fn _language_name(language: LanguageKind) -> &'static str {
    match language {
        LanguageKind::Cpp => "cpp",
        LanguageKind::Rust => "rust",
    }
}

#[cfg(test)]
mod tests {
    use flowrt_ir::{ContractIr, hash_source, normalize_document};
    use flowrt_rsdl::parse_str;

    use super::*;

    #[test]
    fn accepts_valid_minimal_contract() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"

[component.producer]
language = "rust"
output = ["imu:Imu"]

[component.consumer]
language = "rust"
input = ["imu:Imu"]

[instance.producer]
component = "producer"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.consumer]
component = "consumer"

[instance.consumer.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "producer.imu"
to = "consumer.imu"
channel = "latest"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        validate_contract(&ir).unwrap();
    }

    #[test]
    fn rejects_contract_without_graphs() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"
"#;
        let raw = parse_str(source).unwrap();
        let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
        ir.graphs.clear();

        let report = validate_contract(&ir).expect_err("v0.1 contract without graphs should fail");

        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("Contract IR v0.1 must contain exactly one graph; found 0")
        }));
    }

    #[test]
    fn rejects_contract_with_multiple_graphs() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"
"#;
        let raw = parse_str(source).unwrap();
        let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
        let mut second_graph = ir.graphs[0].clone();
        second_graph.name = "secondary".to_string();
        ir.graphs.push(second_graph);

        let report =
            validate_contract(&ir).expect_err("v0.1 contract with multiple graphs should fail");

        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("Contract IR v0.1 must contain exactly one graph; found 2")
        }));
    }

    #[test]
    fn rejects_unsupported_contract_versions() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"
"#;
        let raw = parse_str(source).unwrap();
        let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
        ir.ir_version = "9.9".to_string();
        ir.schema_version = "8.8".to_string();
        ir.package.rsdl_version = "7.7".to_string();

        let report = validate_contract(&ir).expect_err("unsupported versions should fail");

        for expected in [
            "unsupported Contract IR version `9.9`; expected `0.1`",
            "unsupported Contract IR schema version `8.8`; expected `0.1`",
            "unsupported RSDL version `7.7`; expected `0.1`",
        ] {
            assert!(
                report
                    .errors
                    .iter()
                    .any(|error| error.message.contains(expected)),
                "missing validation error: {expected}"
            );
        }
    }

    #[test]
    fn rejects_duplicate_entity_names_in_contract_ir_scopes() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.worker]
language = "rust"
output = ["sample:Sample"]

[instance.worker]
component = "worker"
target = "linux"

[instance.worker.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
        ir.types.push(ir.types[0].clone());
        ir.components.push(ir.components[0].clone());
        ir.profiles.push(ir.profiles[0].clone());
        ir.targets.push(ir.targets[0].clone());
        let duplicate_graph = ir.graphs[0].clone();
        let duplicate_instance = ir.graphs[0].instances[0].clone();
        ir.graphs[0].instances.push(duplicate_instance);
        ir.graphs.push(duplicate_graph);

        let report = validate_contract(&ir).expect_err("duplicate entity names should fail");

        for expected in [
            "contract has duplicate type name `Sample`",
            "contract has duplicate component name `worker`",
            "contract has duplicate profile name `default`",
            "contract has duplicate target name `linux`",
            "contract has duplicate graph name `default`",
            "graph `default` has duplicate instance name `worker`",
        ] {
            assert!(
                report
                    .errors
                    .iter()
                    .any(|error| error.message.contains(expected)),
                "missing validation error: {expected}"
            );
        }
    }

    #[test]
    fn rejects_wrong_bind_direction() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"

[component.producer]
language = "rust"
input = ["imu:Imu"]

[component.consumer]
language = "rust"
input = ["imu:Imu"]

[instance.producer]
component = "producer"

[instance.consumer]
component = "consumer"

[[bind.dataflow]]
from = "producer.imu"
to = "consumer.imu"
channel = "latest"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report = validate_contract(&ir).expect_err("wrong direction should fail validation");
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.message.contains("has no Output port"))
        );
    }

    #[test]
    fn rejects_task_input_without_incoming_bind() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.consumer]
language = "rust"
input = ["sample:Sample"]

[instance.consumer]
component = "consumer"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report = validate_contract(&ir).expect_err("missing incoming bind should fail");

        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("task input `consumer.sample` has no incoming bind")
        }));
    }

    #[test]
    fn rejects_duplicate_task_inputs() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["sample", "sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report = validate_contract(&ir).expect_err("duplicate task inputs should fail");

        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("task on instance `sink` lists input port `sample` more than once")
        }));
    }

    #[test]
    fn rejects_duplicate_task_outputs() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["sample", "sample"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report = validate_contract(&ir).expect_err("duplicate task outputs should fail");

        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("task on instance `source` lists output port `sample` more than once")
        }));
    }

    #[test]
    fn rejects_period_ms_on_non_periodic_task() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["sample:Sample"]

[component.consumer]
language = "rust"
input = ["sample:Sample"]

[instance.producer]
component = "producer"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.consumer]
component = "consumer"

[instance.consumer.task]
trigger = "on_message"
period_ms = 10
input = ["sample"]

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report = validate_contract(&ir).expect_err("non-periodic period_ms should fail");

        assert!(report.errors.iter().any(|error| {
            error.message.contains(
                "task on instance `consumer` must not set period_ms unless trigger is periodic",
            )
        }));
    }

    #[test]
    fn rejects_zero_period_ms_on_periodic_task() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"

[instance.worker.task]
trigger = "periodic"
period_ms = 0
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report = validate_contract(&ir).expect_err("zero period_ms should fail");

        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("periodic task on instance `worker` must set period_ms greater than zero")
        }));
    }

    #[test]
    fn rejects_multiple_tasks_for_one_instance_in_v0_1() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.worker]
language = "rust"
output = ["sample:Sample"]

[instance.worker]
component = "worker"

[instance.worker.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]
"#;
        let raw = parse_str(source).unwrap();
        let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
        let mut second_task = ir.graphs[0].tasks[0].clone();
        second_task.id.0 = "task:default.worker.second".to_string();
        second_task.period_ms = Some(10);
        ir.graphs[0].tasks.push(second_task);

        let report = validate_contract(&ir).expect_err("duplicate instance task should fail");

        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("instance `worker` has multiple tasks in Contract IR v0.1")
        }));
    }

    #[test]
    fn rejects_external_component_kind_until_process_adapter_semantics_exist() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.external_source]
language = "rust"
kind = "external"
output = ["sample:Sample"]

[instance.external_source]
component = "external_source"

[instance.external_source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report = validate_contract(&ir).expect_err("external components should fail");

        assert!(report.errors.iter().any(|error| {
            error.message.contains(
                "component `external_source` uses external process kind, which is not supported by Contract IR v0.1 runtime shell",
            )
        }));
    }

    #[test]
    fn rejects_unknown_backend_names_declared_in_profiles() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[profile.default]
backend = "typo_backend"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report = validate_contract(&ir).expect_err("unknown profile backend should fail");

        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("profile `default` selects unknown backend `typo_backend`")
        }));
    }

    #[test]
    fn rejects_unknown_backend_names_declared_in_targets() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "typo_backend"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report = validate_contract(&ir).expect_err("unknown target backend should fail");

        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("target `linux` declares unknown backend `typo_backend`")
        }));
    }

    #[test]
    fn rejects_implicit_default_backend_when_target_does_not_support_it() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
target = "linux"

[instance.worker.task]
trigger = "periodic"
period_ms = 5

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report = validate_contract(&ir)
            .expect_err("implicit default backend unsupported by target should fail");

        assert!(report.errors.iter().any(|error| {
            error.message.contains(
                "target `linux` does not support backend `inproc` selected by profile `default`",
            )
        }));
    }

    #[test]
    fn rejects_recursive_message_type() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Node]
next = "Node"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report = validate_contract(&ir).expect_err("recursive type should fail validation");
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.message.contains("recursive message type"))
        );
    }

    #[test]
    fn rejects_empty_message_types() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Empty]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report = validate_contract(&ir).expect_err("empty message type should fail");

        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("type `Empty` must declare at least one field")
        }));
    }

    #[test]
    fn rejects_process_spanning_multiple_targets() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "main"
target = "linux_a"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "main"
target = "linux_b"

[instance.sink.task]
trigger = "on_message"
input = ["value"]

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[target.linux_a]
runtime = ["rust"]
backends = ["inproc"]

[target.linux_b]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report = validate_contract(&ir).expect_err("process target mismatch should fail");

        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("process `main` spans multiple targets")
        }));
    }

    #[test]
    fn rejects_invalid_process_names() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
process = "Control-Loop"

[instance.worker.task]
trigger = "periodic"
period_ms = 5
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report = validate_contract(&ir).expect_err("invalid process names should fail");

        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("process name `Control-Loop` must be snake_case")
        }));
    }

    #[test]
    fn rejects_dataflow_cycle_between_instances() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.alpha]
language = "rust"
input = ["feedback:Sample"]
output = ["forward:Sample"]

[component.beta]
language = "rust"
input = ["forward:Sample"]
output = ["feedback:Sample"]

[instance.alpha]
component = "alpha"

[instance.alpha.task]
trigger = "on_message"
input = ["feedback"]
output = ["forward"]

[instance.beta]
component = "beta"

[instance.beta.task]
trigger = "on_message"
input = ["forward"]
output = ["feedback"]

[[bind.dataflow]]
from = "alpha.forward"
to = "beta.forward"
channel = "latest"

[[bind.dataflow]]
from = "beta.feedback"
to = "alpha.feedback"
channel = "latest"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report = validate_contract(&ir).expect_err("dataflow cycle should fail");

        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("graph `default` has a dataflow cycle involving `alpha`")
        }));
    }

    #[test]
    fn rejects_dataflow_self_loop() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.echo]
language = "rust"
input = ["in_value:Sample"]
output = ["out_value:Sample"]

[instance.echo]
component = "echo"

[instance.echo.task]
trigger = "on_message"
input = ["in_value"]
output = ["out_value"]

[[bind.dataflow]]
from = "echo.out_value"
to = "echo.in_value"
channel = "latest"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report = validate_contract(&ir).expect_err("dataflow self-loop should fail");

        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("graph `default` has a dataflow self-loop on instance `echo`")
        }));
    }

    #[test]
    fn rejects_latest_channel_depth_greater_than_one() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"
depth = 2
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report =
            validate_contract(&ir).expect_err("latest channel depth greater than one should fail");

        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("latest channel to `sink.sample` must omit depth or set depth = 1")
        }));
    }

    #[test]
    fn rejects_invalid_rsdl_names() {
        let source = r#"
[package]
name = "RobotDemo"
rsdl_version = "0.1"

[type.imu_sample]
timestamp = "u64"

[component.BadComponent]
language = "rust"
output = ["ImuOut:imu_sample"]

[instance.BadInstance]
component = "BadComponent"
target = "Linux"

[instance.BadInstance.task]
trigger = "periodic"
period_ms = 5
output = ["ImuOut"]

[profile.Default]
backend = "inproc"

[target.Linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let report = validate_contract(&ir).expect_err("invalid RSDL names should fail");

        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("package name `RobotDemo` must be snake_case")
        }));
        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("type name `imu_sample` must be PascalCase")
        }));
        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("component name `BadComponent` must be snake_case")
        }));
        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("port name `ImuOut` must be snake_case")
        }));
        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("profile name `Default` must be snake_case")
        }));
        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("target name `Linux` must be snake_case")
        }));
    }

    #[test]
    fn rejects_duplicate_entity_ids_in_contract_ir_scopes() {
        let mut ir = valid_reference_contract();
        let duplicate_id = ir.types[0].id.clone();
        ir.components[0].id = duplicate_id.clone();
        ir.graphs[0].id = duplicate_id.clone();
        ir.graphs[0].tasks[0].id = duplicate_id.clone();
        ir.graphs[0].binds[0].id = duplicate_id.clone();
        ir.deployments[0].id = duplicate_id.clone();

        let report = validate_contract(&ir).expect_err("duplicate entity IDs should fail");

        assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains(&format!("duplicate entity ID `{}`", duplicate_id.0))
        }));
    }

    #[test]
    fn rejects_inconsistent_entity_references_in_contract_ir() {
        let mut ir = valid_reference_contract();
        let consumer_component_id = ir
            .components
            .iter()
            .find(|component| component.name == "consumer")
            .expect("consumer component must exist")
            .id
            .clone();
        let consumer_instance_id = ir.graphs[0]
            .instances
            .iter()
            .find(|instance| instance.name == "consumer")
            .expect("consumer instance must exist")
            .id
            .clone();
        let consumer_target_id = ir.targets[0].id.clone();

        ir.graphs[0]
            .instances
            .iter_mut()
            .find(|instance| instance.name == "producer")
            .expect("producer instance must exist")
            .component
            .id = consumer_component_id;
        ir.graphs[0]
            .tasks
            .iter_mut()
            .find(|task| task.instance.name == "producer")
            .expect("producer task must exist")
            .instance
            .id = consumer_instance_id.clone();
        ir.graphs[0]
            .binds
            .iter_mut()
            .find(|bind| bind.from.instance.name == "producer")
            .expect("producer bind must exist")
            .from
            .instance
            .id = consumer_instance_id;
        ir.deployments[0].profile.id = consumer_target_id;

        let report =
            validate_contract(&ir).expect_err("inconsistent entity references should fail");

        for expected in [
            "instance `producer` component reference",
            "task on instance `producer` instance reference",
            "bind source instance reference",
            "deployment profile reference",
        ] {
            assert!(
                report
                    .errors
                    .iter()
                    .any(|error| error.message.contains(expected)),
                "missing validation error: {expected}"
            );
        }
    }

    fn valid_reference_contract() -> ContractIr {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["sample:Sample"]

[component.consumer]
language = "rust"
input = ["sample:Sample"]

[instance.producer]
component = "producer"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.consumer]
component = "consumer"
target = "linux"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        normalize_document(&raw, hash_source(source)).unwrap()
    }
}
