use clap::CommandFactory;
use flowrt_ir::normalize_document;
use flowrt_rsdl::parse_str;
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

fn contract_from_source(source: &str) -> ContractIr {
    let raw = parse_str(source).unwrap();
    let contract = normalize_document(&raw, hash_source(source)).unwrap();
    validate_contract(&contract).unwrap();
    contract
}

fn unchecked_contract_from_source(source: &str) -> ContractIr {
    let raw = parse_str(source).unwrap();
    normalize_document(&raw, hash_source(source)).unwrap()
}

fn temp_test_dir(test_name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("flowrt-{test_name}-{}-{nonce}", std::process::id()))
}

mod build_runtime_tests;
mod command_tests;
mod echo_params_tests;
mod record_tests;
mod selfdesc_status_hz_tests;
mod workspace_tests;
