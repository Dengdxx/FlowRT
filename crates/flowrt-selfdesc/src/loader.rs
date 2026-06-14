//! 从 JSON 文件或二进制 section 读取 self-description。

use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use object::{Object, ObjectSection};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::schema::{
    RESOURCE_CONTRACT_SCHEMA_VERSION, SELF_DESCRIPTION_SCHEMA_VERSION, SELF_DESCRIPTION_SECTION,
    SelfDescription,
};

/// 加载错误。
#[derive(Debug, Error)]
pub enum LoadError {
    #[error("failed to read FlowRT image `{path}`: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("FlowRT image is not a supported object file")]
    ParseObject(#[from] object::Error),
    #[error("FlowRT image does not contain `{section}` section")]
    MissingSection { section: String },
    #[error("failed to decode FlowRT self-description section data")]
    SectionData,
    #[error("failed to parse FlowRT self-description: {0}")]
    Json(#[from] serde_json::Error),
    #[error(
        "unsupported FlowRT self-description version `{actual}`; supported version is `{supported}`"
    )]
    UnsupportedVersion {
        actual: String,
        supported: &'static str,
    },
    #[error(
        "unsupported FlowRT resource contract version `{actual}`; supported version is `{supported}`"
    )]
    UnsupportedResourceContractVersion {
        actual: String,
        supported: &'static str,
    },
}

/// 从 `selfdesc.json` 或二进制 `.flowrt.selfdesc` section 读取 self-description JSON bytes。
pub fn load_self_description_json_bytes(path: &Path) -> Result<Vec<u8>, LoadError> {
    let bytes = fs::read(path).map_err(|source| LoadError::Io {
        path: path.display().to_string(),
        source,
    })?;
    if path
        .file_name()
        .is_some_and(|name| name == OsStr::new("selfdesc.json"))
    {
        Ok(bytes)
    } else {
        self_description_section_bytes(&bytes)
    }
}

/// 从 `selfdesc.json` 或二进制 `.flowrt.selfdesc` section 读取 self-description。
pub fn load_self_description(path: &Path) -> Result<SelfDescription, LoadError> {
    let json = load_self_description_json_bytes(path)?;
    parse_self_description_json(&json)
}

/// 读取 self-description，并返回与 runtime handshake 一致的 JSON SHA-256。
pub fn load_self_description_with_hash(
    path: &Path,
) -> Result<(SelfDescription, String), LoadError> {
    let json = load_self_description_json_bytes(path)?;
    let hash = self_description_hash(&json);
    let self_description = parse_self_description_json(&json)?;
    Ok((self_description, hash))
}

fn parse_self_description_json(json: &[u8]) -> Result<SelfDescription, LoadError> {
    let self_description: SelfDescription =
        serde_json::from_slice(json).map_err(LoadError::Json)?;
    if self_description.self_description_version != SELF_DESCRIPTION_SCHEMA_VERSION {
        return Err(LoadError::UnsupportedVersion {
            actual: self_description.self_description_version,
            supported: SELF_DESCRIPTION_SCHEMA_VERSION,
        });
    }
    for graph in &self_description.graphs {
        if self_description_resource_contract_present(graph)
            && graph.resource_contract.resource_contract_version != RESOURCE_CONTRACT_SCHEMA_VERSION
        {
            return Err(LoadError::UnsupportedResourceContractVersion {
                actual: graph.resource_contract.resource_contract_version.clone(),
                supported: RESOURCE_CONTRACT_SCHEMA_VERSION,
            });
        }
    }
    Ok(self_description)
}

fn self_description_resource_contract_present(graph: &crate::schema::SelfDescriptionGraph) -> bool {
    !graph.resource_contract.requirements.is_empty()
        || !graph.resource_contract.providers.is_empty()
        || !graph.resource_contract.satisfactions.is_empty()
        || graph.resource_contract.resource_contract_version != RESOURCE_CONTRACT_SCHEMA_VERSION
}

/// 计算 self-description JSON 的 SHA-256 哈希（hex 小写）。
pub fn self_description_hash(json: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(json);
    format!("{:x}", hasher.finalize())
}

fn self_description_section_bytes(image: &[u8]) -> Result<Vec<u8>, LoadError> {
    let object = object::File::parse(image)?;
    let section = object
        .section_by_name(SELF_DESCRIPTION_SECTION)
        .ok_or_else(|| LoadError::MissingSection {
            section: SELF_DESCRIPTION_SECTION.to_string(),
        })?;
    let data = section.data().map_err(|_| LoadError::SectionData)?;
    let mut data = data.to_vec();
    while data.last() == Some(&0) {
        data.pop();
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_selfdesc_json() -> &'static str {
        r#"{
  "self_description_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "test_pkg" },
  "graphs": [],
  "message_abi": []
}"#
    }

    #[test]
    fn load_from_selfdesc_json_file() {
        let dir =
            std::env::temp_dir().join(format!("flowrt-selfdesc-load-json-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("selfdesc.json");
        std::fs::write(&path, minimal_selfdesc_json()).unwrap();

        let sd = load_self_description(&path).unwrap();
        assert_eq!(sd.self_description_version, "0.1");
        assert_eq!(sd.package.name, "test_pkg");
        assert!(sd.graphs.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_from_binary_section() {
        let dir = std::env::temp_dir().join(format!(
            "flowrt-selfdesc-load-section-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"selfdesc-section-test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
        )
        .unwrap();
        let json = minimal_selfdesc_json();
        let len = json.len();
        std::fs::write(
            dir.join("src/main.rs"),
            format!(
                r##"#[used]
#[unsafe(link_section = ".flowrt.selfdesc")]
static FLOWRT_SELF_DESCRIPTION: [u8; {len}] = *br#"{json}"#;

fn main() {{}}
"##,
            ),
        )
        .unwrap();

        let status = std::process::Command::new("cargo")
            .arg("build")
            .arg("--quiet")
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(status.success());

        let binary_name = if cfg!(windows) {
            "selfdesc-section-test.exe"
        } else {
            "selfdesc-section-test"
        };
        let binary = dir.join("target/debug").join(binary_name);
        let sd = load_self_description(&binary).unwrap();
        assert_eq!(sd.package.name, "test_pkg");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hash_is_deterministic() {
        let json = minimal_selfdesc_json();
        let h1 = self_description_hash(json.as_bytes());
        let h2 = self_description_hash(json.as_bytes());
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let json = r#"{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "x" },
  "graphs": [],
  "message_abi": [],
  "future_field": 42,
  "nested_unknown": { "a": "b" }
}"#;
        let dir =
            std::env::temp_dir().join(format!("flowrt-selfdesc-unknown-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("selfdesc.json");
        std::fs::write(&path, json).unwrap();

        let sd = load_self_description(&path).unwrap();
        assert_eq!(sd.self_description_version, "0.1");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_self_description_defaults_operation_fields() {
        let json = r#"{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "old_operation_free" },
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": []
  }],
  "component_types": [{
    "name": "controller",
    "language": "rust",
    "kind": "native"
  }],
  "message_abi": []
}"#;
        let dir = std::env::temp_dir().join(format!(
            "flowrt-selfdesc-old-operation-defaults-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("selfdesc.json");
        std::fs::write(&path, json).unwrap();

        let sd = load_self_description(&path).unwrap();
        assert!(sd.graphs[0].operations.is_empty());
        assert!(sd.component_types[0].operation_clients.is_empty());
        assert!(sd.component_types[0].operation_servers.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_json_with_island_boundary_fields_loads_correctly() {
        let json = r#"{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "island_demo" },
  "profiles": [{ "name": "dev", "backend": "inproc", "mode": "island" }],
  "graphs": [{
    "name": "default",
    "mode": "island",
    "instances": [],
    "tasks": [],
    "channels": [],
    "boundary_endpoints": [{
      "name": "sample_in",
      "direction": "input",
      "endpoint": "consumer.sample",
      "instance": "consumer",
      "port": "sample",
      "message_type": "Sample"
    }]
  }],
  "message_abi": []
}"#;
        let dir = std::env::temp_dir().join(format!(
            "flowrt-selfdesc-island-boundary-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("selfdesc.json");
        std::fs::write(&path, json).unwrap();

        let sd = load_self_description(&path).unwrap();
        assert_eq!(sd.profiles[0].mode, "island");
        assert_eq!(sd.graphs[0].mode, "island");
        assert_eq!(sd.graphs[0].boundary_endpoints[0].direction, "input");
        assert_eq!(
            sd.graphs[0].boundary_endpoints[0].endpoint,
            "consumer.sample"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unsupported_version_reports_clear_error() {
        let json = r#"{
  "self_description_version": "99.0",
  "source_hash": "abc",
  "package": { "name": "x" },
  "graphs": [],
  "message_abi": []
}"#;
        let dir = std::env::temp_dir().join(format!("flowrt-selfdesc-ver-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("selfdesc.json");
        std::fs::write(&path, json).unwrap();

        let err = load_self_description(&path).unwrap_err();
        assert!(matches!(
            &err,
            LoadError::UnsupportedVersion { actual, supported }
                if actual == "99.0" && *supported == SELF_DESCRIPTION_SCHEMA_VERSION
        ));
        assert!(
            err.to_string()
                .contains("unsupported FlowRT self-description version"),
            "error: {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_json_reports_clear_error() {
        let dir =
            std::env::temp_dir().join(format!("flowrt-selfdesc-badjson-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("selfdesc.json");
        std::fs::write(&path, "not json").unwrap();

        let err = load_self_description(&path).unwrap_err();
        assert!(err.to_string().contains("failed to parse"), "error: {err}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_reports_clear_error() {
        let path = Path::new("/nonexistent/selfdesc.json");
        let err = load_self_description(path).unwrap_err();
        assert!(err.to_string().contains("failed to read"), "error: {err}");
    }

    #[test]
    fn old_json_without_services_field_loads_without_error() {
        let json = r#"{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "x" },
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": []
  }],
  "message_abi": []
}"#;
        let dir =
            std::env::temp_dir().join(format!("flowrt-selfdesc-nosvc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("selfdesc.json");
        std::fs::write(&path, json).unwrap();

        let sd = load_self_description(&path).unwrap();
        assert!(sd.graphs[0].services.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_json_with_services_field_loads_correctly() {
        let json = r#"{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "x" },
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [],
    "services": [{
      "name": "planner.plan_to_executor.execute",
      "canonical_id": "svc_001",
      "client_instance": "planner",
      "client_port": "plan",
      "server_instance": "executor",
      "server_port": "execute",
      "request_type": "PlanRequest",
      "response_type": "PlanResponse"
    }]
  }],
  "message_abi": []
}"#;
        let dir =
            std::env::temp_dir().join(format!("flowrt-selfdesc-withsvc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("selfdesc.json");
        std::fs::write(&path, json).unwrap();

        let sd = load_self_description(&path).unwrap();
        assert_eq!(sd.graphs[0].services.len(), 1);
        assert_eq!(
            sd.graphs[0].services[0].name,
            "planner.plan_to_executor.execute"
        );
        assert_eq!(sd.graphs[0].services[0].request_type, "PlanRequest");
        assert_eq!(sd.graphs[0].services[0].response_type, "PlanResponse");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_json_without_component_types_loads_without_error() {
        let json = r#"{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "x" },
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": []
  }],
  "message_abi": []
}"#;
        let dir =
            std::env::temp_dir().join(format!("flowrt-selfdesc-nocomp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("selfdesc.json");
        std::fs::write(&path, json).unwrap();

        let sd = load_self_description(&path).unwrap();
        assert!(sd.component_types.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_json_with_component_types_loads_correctly() {
        let json = r#"{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "x" },
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": []
  }],
  "component_types": [{
    "name": "sensor",
    "language": "rust",
    "kind": "native",
    "inputs": [],
    "outputs": [{ "name": "imu", "type": "Imu" }],
    "service_clients": [],
    "service_servers": [],
    "params": [{ "name": "rate", "type": "f64", "update": "on_tick" }]
  }],
  "message_abi": []
}"#;
        let dir =
            std::env::temp_dir().join(format!("flowrt-selfdesc-withcomp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("selfdesc.json");
        std::fs::write(&path, json).unwrap();

        let sd = load_self_description(&path).unwrap();
        assert_eq!(sd.component_types.len(), 1);
        assert_eq!(sd.component_types[0].name, "sensor");
        assert_eq!(sd.component_types[0].language, "rust");
        assert_eq!(sd.component_types[0].kind, "native");
        assert_eq!(sd.component_types[0].outputs.len(), 1);
        assert_eq!(sd.component_types[0].outputs[0].name, "imu");
        assert_eq!(sd.component_types[0].outputs[0].ty, "Imu");
        assert_eq!(sd.component_types[0].params.len(), 1);
        assert_eq!(sd.component_types[0].params[0].name, "rate");
        assert_eq!(sd.component_types[0].params[0].ty, "f64");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
