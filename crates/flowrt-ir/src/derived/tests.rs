use flowrt_rsdl::parse_str;

use super::derive_contract_facts;
use crate::{
    BackendName, BackendThreadAffinity, CapabilityAtom, ChannelBackendSource, hash_source,
    normalize_document,
};

fn normalize_source(source: &str) -> crate::ContractIr {
    let raw = parse_str(source).unwrap();
    normalize_document(&raw, hash_source(source)).unwrap()
}

#[test]
fn derive_contract_facts_recomputes_route_target_deployment_and_resources() {
    let source = r#"
[package]
name = "derived_fact_demo"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes"

[component.producer]
language = "rust"
output = ["packet:Packet"]

[component.consumer]
language = "rust"
input = ["packet:Packet"]

[component.consumer.resource.frames]
capability = "perception.camera.frames"
access = "read"
readiness = "before_init"

[instance.producer]
component = "producer"
target = "edge"

[instance.consumer]
component = "consumer"
target = "edge"

[instance.consumer.task]
trigger = "on_message"
input = ["packet"]

[[resource.provider]]
name = "camera_provider"
capabilities = ["perception.camera.frames"]
scope = "target"
target = "edge"
health_source = "target_health"
readiness_source = "target_ready"

[[bind.dataflow]]
from = "producer.packet"
to = "consumer.packet"
channel = "latest"

[profile.default]
backend = "iox2"

[target.edge]
runtime = ["rust"]
backends = ["iox2", "zenoh"]
"#;
    let mut contract = normalize_source(source);

    let bind = &mut contract.graphs[0].binds[0];
    bind.backend = BackendName("iox2".to_string());
    bind.backend_source = ChannelBackendSource::Explicit;
    bind.thread_affinity = Some(BackendThreadAffinity::SchedulerLocalCommit);
    bind.capability_requirements = vec![CapabilityAtom("tampered:route".to_string())];

    contract.targets[0].capabilities = vec![CapabilityAtom("tampered:target".to_string())];

    let deployment = &mut contract.deployments[0];
    deployment.backend = BackendName("zenoh".to_string());
    deployment.required_capabilities = vec![CapabilityAtom("tampered:deployment".to_string())];
    deployment.satisfied = false;

    contract.graphs[0].resource_satisfactions.clear();

    let facts = derive_contract_facts(&contract).unwrap();

    assert!(
        facts
            .message_abi_capabilities
            .contains(&CapabilityAtom("abi:variable_payload_frame".to_string()))
    );
    assert!(
        !facts
            .message_abi_capabilities
            .contains(&CapabilityAtom("tampered:route".to_string()))
    );

    let graph = facts
        .graphs
        .iter()
        .find(|graph| graph.graph.name == "default")
        .unwrap();
    let route = graph.routes.first().unwrap();
    assert_eq!(route.backend.0, "zenoh");
    assert_eq!(route.backend_source, ChannelBackendSource::AutoFallback);
    assert_eq!(route.thread_affinity, Some(BackendThreadAffinity::SendSafe));
    assert!(!route.topology.crosses_process);
    assert!(
        route
            .capability_requirements
            .contains(&CapabilityAtom("abi:variable_payload_frame".to_string()))
    );
    assert!(
        !route
            .capability_requirements
            .contains(&CapabilityAtom("tampered:route".to_string()))
    );

    assert_eq!(graph.resources.satisfactions.len(), 1);
    assert_eq!(graph.resources.satisfied_count, 1);
    assert_eq!(graph.resources.required_unsatisfied_count, 0);
    assert_eq!(
        graph.resources.satisfactions[0]
            .provider
            .as_ref()
            .map(|provider| provider.name.as_str()),
        Some("camera_provider")
    );

    let target = facts
        .targets
        .iter()
        .find(|target| target.target.name == "edge")
        .unwrap();
    assert!(
        target
            .capabilities
            .contains(&CapabilityAtom("abi:variable_payload_frame".to_string()))
    );
    assert!(
        !target
            .capabilities
            .contains(&CapabilityAtom("tampered:target".to_string()))
    );

    let deployment = facts.deployments.first().unwrap();
    assert_eq!(deployment.backend.0, "iox2");
    assert!(
        deployment
            .required_capabilities
            .contains(&CapabilityAtom("trigger:on_message".to_string()))
    );
    assert!(
        !deployment
            .required_capabilities
            .contains(&CapabilityAtom("tampered:deployment".to_string()))
    );
    assert!(deployment.decision.satisfied);
}

#[test]
fn derive_contract_facts_orders_target_capabilities_by_catalog_not_backend_order() {
    let source = r#"
[package]
name = "derived_order_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"

[instance.worker.task]
trigger = "periodic"
period_ms = 10

[profile.default]
backend = "inproc"

[target.edge]
runtime = ["rust"]
backends = ["zenoh", "iox2", "inproc"]
"#;
    let mut contract = normalize_source(source);
    contract.targets[0].backends = vec![
        BackendName("inproc".to_string()),
        BackendName("iox2".to_string()),
        BackendName("zenoh".to_string()),
    ];
    contract.targets[0].capabilities = vec![CapabilityAtom("tampered:target".to_string())];

    let facts = derive_contract_facts(&contract).unwrap();
    let capabilities = &facts.targets[0].capabilities;

    assert_eq!(
        capabilities.iter().take(5).cloned().collect::<Vec<_>>(),
        vec![
            CapabilityAtom("abi:fixed_size_plain_data".to_string()),
            CapabilityAtom("abi:variable_payload_frame".to_string()),
            CapabilityAtom("layout:native_layout".to_string()),
            CapabilityAtom("allocation:bounded".to_string()),
            CapabilityAtom("allocation:unbounded_dynamic".to_string()),
        ]
    );
    assert!(
        !capabilities.contains(&CapabilityAtom("tampered:target".to_string())),
        "derived target capabilities must ignore stored target capability metadata"
    );
}
