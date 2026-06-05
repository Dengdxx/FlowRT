use flowrt_ir::{
    CapabilityAtom, ContractIr, OverflowPolicy, ParamIr, ParamType, ParamUpdatePolicy, ParamValue,
    ParamValueIr, StalePolicy, backend_capabilities, deployment_capability_decision, hash_source,
    normalize_document,
};
use flowrt_rsdl::parse_str;

use super::*;

mod capability_tests;
mod component_tests;
mod contract_tests;
mod graph_tests;
mod message_tests;

fn bounded_variable_contract(backend: &str) -> ContractIr {
    let source = format!(
        r#"
[package]
name = "variable_demo"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes<max=262144>"
label = "string<max=64>"
samples = "sequence<u32,max=16>"

[component.producer]
language = "rust"
output = ["packet:Packet"]

[component.consumer]
language = "rust"
input = ["packet:Packet"]

[instance.producer]
component = "producer"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["packet"]

[instance.consumer]
component = "consumer"
target = "linux"

[instance.consumer.task]
trigger = "on_message"
input = ["packet"]

[[bind.dataflow]]
from = "producer.packet"
to = "consumer.packet"
channel = "latest"

[profile.default]
backend = "{backend}"

[target.linux]
runtime = ["rust"]
backends = ["{backend}"]
"#
    );
    let raw = parse_str(&source).unwrap();
    normalize_document(&raw, hash_source(&source)).unwrap()
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

fn test_param(name: &str, default: ParamValue) -> ParamIr {
    ParamIr {
        name: name.to_string(),
        ty: match default {
            ParamValue::Bool(_) => ParamType::Bool,
            ParamValue::Integer(_) => ParamType::I64,
            ParamValue::Float(_) => ParamType::F64,
            ParamValue::String(_) => ParamType::String,
            ParamValue::Array(_) => ParamType::Array,
            ParamValue::Table(_) => ParamType::Table,
        },
        default,
        update: ParamUpdatePolicy::Startup,
        min: None,
        max: None,
        choices: Vec::new(),
    }
}
