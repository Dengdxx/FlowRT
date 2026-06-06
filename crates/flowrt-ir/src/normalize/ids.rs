use sha2::{Digest, Sha256};

use crate::EntityId;

/// 计算稳定的 SHA-256 源文本哈希。
pub fn hash_source(source: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(super) fn entity_id(kind: &str, qualified_name: &str) -> EntityId {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update(b":");
    hasher.update(qualified_name.as_bytes());
    let digest = hasher.finalize();
    EntityId(format!("{kind}_{}", hex_prefix(&digest)))
}

pub(super) fn hex_prefix(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
