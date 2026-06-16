use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    BoundaryDirection, BoundaryEndpointIr, CONTRACT_IR_VERSION, CONTRACT_SCHEMA_VERSION,
    ChannelEdgeIr, ClockSourceKind, ContractIr, EntityId, EntityRef, GraphMode, LanguageKind,
    OperationEdgeIr, RSDL_VERSION, ResourceSatisfactionIr,
};

use crate::ValidationError;

pub(crate) fn validate_contract_versions(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
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

pub(crate) fn validate_contract_shape(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    if ir.graphs.len() != 1 {
        errors.push(ValidationError::new(format!(
            "Contract IR v0.1 must contain exactly one graph; found {}",
            ir.graphs.len()
        )));
    }
    if ir.profiles.is_empty() {
        errors.push(ValidationError::new(
            "Contract IR v0.1 must contain at least one profile; found 0",
        ));
    }
}

/// 返回 clock source 的 canonical 名称，用于诊断文案。
fn clock_source_label(source: ClockSourceKind) -> &'static str {
    source.label()
}

pub(crate) fn validate_contract_artifact(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    let artifact = &ir.artifact;
    if artifact.temporary_island && !artifact.test_only {
        errors.push(ValidationError::new(
            "temporary island artifact must also be test_only",
        ));
    }
    if artifact.test_only && artifact.temporary_overlay.is_none() {
        errors.push(ValidationError::new(
            "test_only artifact must identify its temporary_overlay source",
        ));
    }

    // clock source 是派生事实：按 temporary_overlay 重新推导，拒绝不一致或暂不支持的取值，
    // 不能信任落盘 IR 中可被手工改写的来源。
    match artifact.clock_source {
        ClockSourceKind::ExternalStepped => {
            errors.push(ValidationError::new(
                "external_stepped clock source is not supported yet; \
                 use a realtime or simulated_replay contract",
            ));
        }
        clock_source => {
            let expected = if artifact.temporary_overlay.is_some() {
                ClockSourceKind::SimulatedReplay
            } else {
                ClockSourceKind::Realtime
            };
            if clock_source != expected {
                errors.push(ValidationError::new(format!(
                    "artifact clock source `{}` is inconsistent with derived `{}`",
                    clock_source_label(clock_source),
                    clock_source_label(expected),
                )));
            }
        }
    }

    let Some(overlay) = artifact.temporary_overlay.as_ref() else {
        return;
    };

    if artifact.mode != GraphMode::Island {
        errors.push(ValidationError::new(
            "temporary overlay artifact must use island artifact mode",
        ));
    }
    if !artifact.temporary_island || !artifact.test_only {
        errors.push(ValidationError::new(
            "temporary overlay artifact must be temporary_island and test_only",
        ));
    }
    if overlay.kind != "temporary_island" {
        errors.push(ValidationError::new(format!(
            "temporary overlay kind `{}` is not supported",
            overlay.kind
        )));
    }
    if overlay.generated_by.command.trim().is_empty() {
        errors.push(ValidationError::new(
            "temporary overlay generated_by.command must not be empty",
        ));
    }
    if overlay.generated_by.source.trim().is_empty() {
        errors.push(ValidationError::new(
            "temporary overlay generated_by.source must not be empty",
        ));
    }
    if overlay.boundary_mappings.is_empty() {
        errors.push(ValidationError::new(
            "temporary overlay must record at least one boundary mapping",
        ));
    }

    for mapping in &overlay.boundary_mappings {
        if mapping.name.trim().is_empty() {
            errors.push(ValidationError::new(
                "temporary overlay boundary mapping name must not be empty",
            ));
        }
        if mapping.endpoint.trim().is_empty() {
            errors.push(ValidationError::new(
                "temporary overlay boundary mapping endpoint must not be empty",
            ));
        }
        let expected_source = match mapping.direction {
            BoundaryDirection::Input => "--boundary-input",
            BoundaryDirection::Output => "--boundary-output",
        };
        if mapping.source != expected_source {
            errors.push(ValidationError::new(format!(
                "temporary overlay boundary mapping `{}` source `{}` does not match direction `{}`",
                mapping.name, mapping.source, expected_source
            )));
        }
        if !ir.graphs.iter().any(|graph| {
            graph.boundary_endpoints.iter().any(|endpoint| {
                endpoint.direction == mapping.direction
                    && endpoint.name == mapping.name
                    && format!("{}.{}", endpoint.port.instance.name, endpoint.port.port)
                        == mapping.endpoint
            })
        }) {
            errors.push(ValidationError::new(format!(
                "temporary overlay boundary mapping `{}` -> `{}` does not match graph boundary endpoints",
                mapping.name, mapping.endpoint
            )));
        }
    }
}

pub(crate) fn validate_contract_canonical_fields(
    ir: &ContractIr,
    errors: &mut Vec<ValidationError>,
) {
    if !is_canonical_hex_digest(&ir.source_hash, 64) {
        errors.push(ValidationError::new(format!(
            "source_hash `{}` must be a 64-character lowercase hex digest",
            ir.source_hash
        )));
    }

    validate_entity_id_shape("package id", "package", &ir.package_id, errors);
    for ty in &ir.types {
        validate_entity_id_shape("type id", "type", &ty.id, errors);
    }
    for component in &ir.components {
        validate_entity_id_shape("component id", "component", &component.id, errors);
        for resource in &component.resources {
            validate_entity_id_shape(
                "resource requirement id",
                "resource_requirement",
                &resource.id,
                errors,
            );
        }
    }
    for graph in &ir.graphs {
        validate_entity_id_shape("graph id", "graph", &graph.id, errors);
        for instance in &graph.instances {
            validate_entity_id_shape("instance id", "instance", &instance.id, errors);
        }
        for task in &graph.tasks {
            validate_entity_id_shape("task id", "task", &task.id, errors);
        }
        for bind in &graph.binds {
            validate_entity_id_shape("bind id", "bind", &bind.id, errors);
        }
        for service in &graph.services {
            validate_entity_id_shape("service id", "service", &service.id, errors);
        }
        for operation in &graph.operations {
            validate_entity_id_shape("operation id", "operation", &operation.id, errors);
        }
        for endpoint in &graph.boundary_endpoints {
            validate_entity_id_shape("boundary endpoint id", "boundary", &endpoint.id, errors);
        }
        for bridge in &graph.ros2_bridges {
            validate_entity_id_shape("ROS2 bridge id", "bridge", &bridge.id, errors);
        }
        for provider in &graph.resource_providers {
            validate_entity_id_shape(
                "resource provider id",
                "resource_provider",
                &provider.id,
                errors,
            );
        }
        for satisfaction in &graph.resource_satisfactions {
            validate_entity_id_shape(
                "resource satisfaction id",
                "resource_satisfaction",
                &satisfaction.id,
                errors,
            );
        }
        for group in &graph.sync_groups {
            validate_entity_id_shape("sync group id", "sync", &group.id, errors);
        }
    }
    for profile in &ir.profiles {
        validate_entity_id_shape("profile id", "profile", &profile.id, errors);
    }
    for target in &ir.targets {
        validate_entity_id_shape("target id", "target", &target.id, errors);
    }
    for deployment in &ir.deployments {
        validate_entity_id_shape("deployment id", "deployment", &deployment.id, errors);
    }
}

pub(crate) fn validate_contract_canonical_ordering(
    ir: &ContractIr,
    errors: &mut Vec<ValidationError>,
) {
    let mut import_kinds = BTreeSet::new();
    for import in &ir.package.imports {
        if !is_supported_import_kind(&import.kind) {
            errors.push(ValidationError::new(format!(
                "package import kind `{}` is not supported",
                import.kind
            )));
        }

        if !import_kinds.insert(import.kind.as_str()) {
            errors.push(ValidationError::new(format!(
                "package imports have duplicate kind `{}`",
                import.kind
            )));
        }

        let mut import_patterns = BTreeSet::new();
        for pattern in &import.patterns {
            if !import_patterns.insert(pattern.as_str()) {
                errors.push(ValidationError::new(format!(
                    "package import `{}` has duplicate pattern `{pattern}`",
                    import.kind
                )));
            }
        }
    }

    if !ir
        .package
        .imports
        .windows(2)
        .all(|pair| pair[0].kind <= pair[1].kind)
    {
        errors.push(ValidationError::new(
            "package imports must use canonical kind order",
        ));
    }
    for import in &ir.package.imports {
        if !import.patterns.windows(2).all(|pair| pair[0] <= pair[1]) {
            errors.push(ValidationError::new(format!(
                "package import `{}` patterns must use canonical sorted order",
                import.kind
            )));
        }
    }

    if !ir.types.windows(2).all(|pair| pair[0].name <= pair[1].name) {
        errors.push(ValidationError::new(
            "contract types must use canonical name order",
        ));
    }
    if !ir
        .components
        .windows(2)
        .all(|pair| pair[0].name <= pair[1].name)
    {
        errors.push(ValidationError::new(
            "contract components must use canonical name order",
        ));
    }
    for component in &ir.components {
        if !component
            .params
            .windows(2)
            .all(|pair| pair[0].name <= pair[1].name)
        {
            errors.push(ValidationError::new(format!(
                "component `{}` params must use canonical name order",
                component.name
            )));
        }
        if !component
            .resources
            .windows(2)
            .all(|pair| pair[0].name <= pair[1].name)
        {
            errors.push(ValidationError::new(format!(
                "component `{}` resources must use canonical name order",
                component.name
            )));
        }
    }

    if !ir
        .graphs
        .windows(2)
        .all(|pair| pair[0].name <= pair[1].name)
    {
        errors.push(ValidationError::new(
            "contract graphs must use canonical name order",
        ));
    }
    for graph in &ir.graphs {
        if !graph
            .instances
            .windows(2)
            .all(|pair| pair[0].name <= pair[1].name)
        {
            errors.push(ValidationError::new(format!(
                "graph `{}` instances must use canonical name order",
                graph.name
            )));
        }
        for instance in &graph.instances {
            if !instance
                .params
                .windows(2)
                .all(|pair| pair[0].name <= pair[1].name)
            {
                errors.push(ValidationError::new(format!(
                    "instance `{}` params must use canonical name order",
                    instance.name
                )));
            }
        }
        if !graph
            .tasks
            .windows(2)
            .all(|pair| task_canonical_key(&pair[0]) <= task_canonical_key(&pair[1]))
        {
            errors.push(ValidationError::new(format!(
                "graph `{}` tasks must use canonical instance/name order",
                graph.name
            )));
        }
        if !graph
            .processes
            .windows(2)
            .all(|pair| pair[0].name <= pair[1].name)
        {
            errors.push(ValidationError::new(format!(
                "graph `{}` processes must use canonical name order",
                graph.name
            )));
        }
        for process in &graph.processes {
            if !process.depends_on.windows(2).all(|pair| pair[0] <= pair[1]) {
                errors.push(ValidationError::new(format!(
                    "process `{}` dependencies must use canonical sorted order",
                    process.name
                )));
            }
        }
        if !graph
            .binds
            .windows(2)
            .all(|pair| bind_canonical_key(&pair[0]) <= bind_canonical_key(&pair[1]))
        {
            errors.push(ValidationError::new(format!(
                "graph `{}` binds must use canonical endpoint order",
                graph.name
            )));
        }
        if !graph
            .services
            .windows(2)
            .all(|pair| service_canonical_key(&pair[0]) <= service_canonical_key(&pair[1]))
        {
            errors.push(ValidationError::new(format!(
                "graph `{}` services must use canonical endpoint order",
                graph.name
            )));
        }
        if !graph
            .operations
            .windows(2)
            .all(|pair| operation_canonical_key(&pair[0]) <= operation_canonical_key(&pair[1]))
        {
            errors.push(ValidationError::new(format!(
                "graph `{}` operations must use canonical endpoint order",
                graph.name
            )));
        }
        if !graph.boundary_endpoints.windows(2).all(|pair| {
            boundary_endpoint_canonical_key(&pair[0]) <= boundary_endpoint_canonical_key(&pair[1])
        }) {
            errors.push(ValidationError::new(format!(
                "graph `{}` boundary endpoints must use canonical direction/name order",
                graph.name
            )));
        }
        if !graph
            .ros2_bridges
            .windows(2)
            .all(|pair| pair[0].name <= pair[1].name)
        {
            errors.push(ValidationError::new(format!(
                "graph `{}` ROS2 bridges must use canonical name order",
                graph.name
            )));
        }
        if !graph
            .resource_providers
            .windows(2)
            .all(|pair| pair[0].name <= pair[1].name)
        {
            errors.push(ValidationError::new(format!(
                "graph `{}` resource providers must use canonical name order",
                graph.name
            )));
        }
        if !graph.resource_satisfactions.windows(2).all(|pair| {
            resource_satisfaction_canonical_key(&pair[0])
                <= resource_satisfaction_canonical_key(&pair[1])
        }) {
            errors.push(ValidationError::new(format!(
                "graph `{}` resource satisfactions must use canonical instance/resource order",
                graph.name
            )));
        }
        if !graph
            .sync_groups
            .windows(2)
            .all(|pair| pair[0].name <= pair[1].name)
        {
            errors.push(ValidationError::new(format!(
                "graph `{}` sync groups must use canonical name order",
                graph.name
            )));
        }
    }

    if !ir
        .profiles
        .windows(2)
        .all(|pair| pair[0].name <= pair[1].name)
    {
        errors.push(ValidationError::new(
            "contract profiles must use canonical name order",
        ));
    }
    if !ir
        .targets
        .windows(2)
        .all(|pair| pair[0].name <= pair[1].name)
    {
        errors.push(ValidationError::new(
            "contract targets must use canonical name order",
        ));
    }
    for target in &ir.targets {
        if !target
            .runtime
            .windows(2)
            .all(|pair| target_runtime_rank(pair[0]) <= target_runtime_rank(pair[1]))
        {
            errors.push(ValidationError::new(format!(
                "target `{}` runtime must use canonical sorted order",
                target.name
            )));
        }
        if !target.backends.windows(2).all(|pair| pair[0] <= pair[1]) {
            errors.push(ValidationError::new(format!(
                "target `{}` backends must use canonical sorted order",
                target.name
            )));
        }
    }

    if !ir
        .deployments
        .windows(2)
        .all(|pair| deployment_canonical_key(&pair[0]) <= deployment_canonical_key(&pair[1]))
    {
        errors.push(ValidationError::new(
            "contract deployments must use canonical graph/profile/target order",
        ));
    }
}

pub(crate) fn validate_entity_name_uniqueness(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    validate_unique_names(
        "contract",
        "type",
        ir.types.iter().map(|ty| ty.qualified_name.as_str()),
        errors,
    );
    validate_unique_names(
        "contract",
        "component",
        ir.components
            .iter()
            .map(|component| component.qualified_name.as_str()),
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
    for component in &ir.components {
        validate_unique_names(
            &format!("component `{}`", component.name),
            "resource",
            component
                .resources
                .iter()
                .map(|resource| resource.name.as_str()),
            errors,
        );
    }

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
        validate_unique_names(
            &format!("graph `{}`", graph.name),
            "process",
            graph.processes.iter().map(|process| process.name.as_str()),
            errors,
        );
        validate_unique_names(
            &format!("graph `{}`", graph.name),
            "ROS2 bridge",
            graph.ros2_bridges.iter().map(|bridge| bridge.name.as_str()),
            errors,
        );
        validate_unique_names(
            &format!("graph `{}`", graph.name),
            "resource provider",
            graph
                .resource_providers
                .iter()
                .map(|provider| provider.name.as_str()),
            errors,
        );
        validate_unique_names(
            &format!("graph `{}`", graph.name),
            "sync group",
            graph.sync_groups.iter().map(|group| group.name.as_str()),
            errors,
        );
        for direction in [BoundaryDirection::Input, BoundaryDirection::Output] {
            let direction_name = match direction {
                BoundaryDirection::Input => "boundary input",
                BoundaryDirection::Output => "boundary output",
            };
            validate_unique_names(
                &format!("graph `{}`", graph.name),
                direction_name,
                graph
                    .boundary_endpoints
                    .iter()
                    .filter(|endpoint| endpoint.direction == direction)
                    .map(|endpoint| endpoint.name.as_str()),
                errors,
            );
        }
    }
}

pub(crate) fn validate_entity_id_uniqueness(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
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
        for resource in &component.resources {
            record_entity_id(
                &mut seen,
                &resource.id,
                format!(
                    "resource requirement `{}.{}`",
                    component.name, resource.name
                ),
                errors,
            );
        }
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
                format!("task `{}` on instance `{}`", task.name, task.instance.name),
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
        for service in &graph.services {
            record_entity_id(
                &mut seen,
                &service.id,
                format!(
                    "service `{}.{}` -> `{}.{}`",
                    service.client.instance.name,
                    service.client.port,
                    service.server.instance.name,
                    service.server.port
                ),
                errors,
            );
        }
        for operation in &graph.operations {
            record_entity_id(
                &mut seen,
                &operation.id,
                format!(
                    "operation `{}.{}` -> `{}.{}`",
                    operation.client.instance.name,
                    operation.client.port,
                    operation.server.instance.name,
                    operation.server.port
                ),
                errors,
            );
        }
        for endpoint in &graph.boundary_endpoints {
            record_entity_id(
                &mut seen,
                &endpoint.id,
                format!("boundary endpoint `{}`", endpoint.name),
                errors,
            );
        }
        for bridge in &graph.ros2_bridges {
            record_entity_id(
                &mut seen,
                &bridge.id,
                format!("ROS2 bridge `{}`", bridge.name),
                errors,
            );
        }
        for provider in &graph.resource_providers {
            record_entity_id(
                &mut seen,
                &provider.id,
                format!("resource provider `{}`", provider.name),
                errors,
            );
        }
        for satisfaction in &graph.resource_satisfactions {
            record_entity_id(
                &mut seen,
                &satisfaction.id,
                format!(
                    "resource satisfaction `{}.{}`",
                    satisfaction.instance.name, satisfaction.resource
                ),
                errors,
            );
        }
        for group in &graph.sync_groups {
            record_entity_id(
                &mut seen,
                &group.id,
                format!("sync group `{}`", group.name),
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

pub(crate) fn validate_entity_references(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    let component_ids = ir
        .components
        .iter()
        .map(|component| (component.qualified_name.as_str(), &component.id))
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
    let resource_requirement_ids = ir
        .components
        .iter()
        .flat_map(|component| {
            component.resources.iter().map(|resource| {
                (
                    format!("{}.{}", component.qualified_name, resource.name),
                    &resource.id,
                )
            })
        })
        .collect::<BTreeMap<_, _>>();
    let resource_requirement_ref_ids = resource_requirement_ids
        .iter()
        .map(|(name, id)| (name.as_str(), *id))
        .collect::<BTreeMap<_, _>>();

    for graph in &ir.graphs {
        let instance_ids = graph
            .instances
            .iter()
            .map(|instance| (instance.name.as_str(), &instance.id))
            .collect::<BTreeMap<_, _>>();
        let boundary_ids = graph
            .boundary_endpoints
            .iter()
            .map(|endpoint| (endpoint.name.as_str(), &endpoint.id))
            .collect::<BTreeMap<_, _>>();
        let provider_ids = graph
            .resource_providers
            .iter()
            .map(|provider| (provider.name.as_str(), &provider.id))
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
                    "task `{}` on instance `{}` instance reference",
                    task.name, task.instance.name
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

        for service in &graph.services {
            validate_named_entity_ref(
                "service client instance reference",
                "instance",
                &service.client.instance,
                &instance_ids,
                errors,
            );
            validate_named_entity_ref(
                "service server instance reference",
                "instance",
                &service.server.instance,
                &instance_ids,
                errors,
            );
        }

        for operation in &graph.operations {
            validate_named_entity_ref(
                "operation client instance reference",
                "instance",
                &operation.client.instance,
                &instance_ids,
                errors,
            );
            validate_named_entity_ref(
                "operation server instance reference",
                "instance",
                &operation.server.instance,
                &instance_ids,
                errors,
            );
        }

        for bridge in &graph.ros2_bridges {
            validate_named_entity_ref(
                "ROS2 bridge FlowRT instance reference",
                "instance",
                &bridge.flowrt.instance,
                &instance_ids,
                errors,
            );
            if let Some(endpoint) = &bridge.boundary_endpoint {
                validate_named_entity_ref(
                    &format!("ROS2 bridge `{}` boundary endpoint reference", bridge.name),
                    "boundary endpoint",
                    endpoint,
                    &boundary_ids,
                    errors,
                );
            }
        }

        for endpoint in &graph.boundary_endpoints {
            validate_named_entity_ref(
                &format!("boundary endpoint `{}` instance reference", endpoint.name),
                "instance",
                &endpoint.port.instance,
                &instance_ids,
                errors,
            );
        }

        for provider in &graph.resource_providers {
            if let Some(target) = &provider.target {
                validate_named_entity_ref(
                    &format!("resource provider `{}` target reference", provider.name),
                    "target",
                    target,
                    &target_ids,
                    errors,
                );
            }
        }

        for satisfaction in &graph.resource_satisfactions {
            validate_named_entity_ref(
                &format!(
                    "resource satisfaction `{}.{}` instance reference",
                    satisfaction.instance.name, satisfaction.resource
                ),
                "instance",
                &satisfaction.instance,
                &instance_ids,
                errors,
            );
            validate_named_entity_ref(
                &format!(
                    "resource satisfaction `{}.{}` component reference",
                    satisfaction.instance.name, satisfaction.resource
                ),
                "component",
                &satisfaction.component,
                &component_ids,
                errors,
            );
            validate_named_entity_ref(
                &format!(
                    "resource satisfaction `{}.{}` requirement reference",
                    satisfaction.instance.name, satisfaction.resource
                ),
                "resource requirement",
                &satisfaction.requirement,
                &resource_requirement_ref_ids,
                errors,
            );
            if let Some(provider) = &satisfaction.provider {
                validate_named_entity_ref(
                    &format!(
                        "resource satisfaction `{}.{}` provider reference",
                        satisfaction.instance.name, satisfaction.resource
                    ),
                    "resource provider",
                    provider,
                    &provider_ids,
                    errors,
                );
            }
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

fn is_supported_import_kind(kind: &str) -> bool {
    matches!(
        kind,
        "types" | "components" | "graphs" | "profiles" | "targets" | "modules" | "compositions"
    )
}

fn validate_entity_id_shape(
    label: &str,
    kind: &str,
    id: &EntityId,
    errors: &mut Vec<ValidationError>,
) {
    let Some(hex) = id.0.strip_prefix(&format!("{kind}_")) else {
        errors.push(ValidationError::new(format!(
            "{label} `{}` must use the `{kind}_<hex>` canonical format",
            id.0
        )));
        return;
    };
    if !is_canonical_hex_digest(hex, 16) {
        errors.push(ValidationError::new(format!(
            "{label} `{}` must use the `{kind}_<hex>` canonical format",
            id.0
        )));
    }
}

fn is_canonical_hex_digest(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len && value.chars().all(|ch| matches!(ch, '0'..='9' | 'a'..='f'))
}

fn bind_canonical_key(bind: &ChannelEdgeIr) -> (&str, &str, &str, &str) {
    (
        bind.from.instance.name.as_str(),
        bind.from.port.as_str(),
        bind.to.instance.name.as_str(),
        bind.to.port.as_str(),
    )
}

fn service_canonical_key(service: &flowrt_ir::ServiceEdgeIr) -> (&str, &str, &str, &str) {
    (
        service.client.instance.name.as_str(),
        service.client.port.as_str(),
        service.server.instance.name.as_str(),
        service.server.port.as_str(),
    )
}

fn operation_canonical_key(operation: &OperationEdgeIr) -> (&str, &str, &str, &str) {
    (
        operation.client.instance.name.as_str(),
        operation.client.port.as_str(),
        operation.server.instance.name.as_str(),
        operation.server.port.as_str(),
    )
}

fn boundary_endpoint_canonical_key(endpoint: &BoundaryEndpointIr) -> (BoundaryDirection, &str) {
    (endpoint.direction, endpoint.name.as_str())
}

fn task_canonical_key(task: &flowrt_ir::TaskIr) -> (&str, &str) {
    (task.instance.name.as_str(), task.name.as_str())
}

fn resource_satisfaction_canonical_key(satisfaction: &ResourceSatisfactionIr) -> (&str, &str) {
    (
        satisfaction.instance.name.as_str(),
        satisfaction.resource.as_str(),
    )
}

fn target_runtime_rank(language: LanguageKind) -> u8 {
    match language {
        LanguageKind::C => 0,
        LanguageKind::Cpp => 1,
        LanguageKind::Rust => 2,
        LanguageKind::External => 3,
    }
}

fn deployment_canonical_key(deployment: &flowrt_ir::DeploymentIr) -> (&str, &str, &str) {
    (
        deployment.graph.name.as_str(),
        deployment.profile.name.as_str(),
        deployment.target.name.as_str(),
    )
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
