//! FlowRT 共享自描述 schema、加载/校验与 ABI 格式化。
//!
//! 本 crate 承载 CLI、codegen 和 runtime 共用的 self-description wire 类型，避免在多处
//! 复制结构体；同时提供 `.flowrt.selfdesc` section 与 `selfdesc.json` 读取、SHA-256
//! 哈希、协议版本校验和 Message ABI / variable frame 字段格式化基础。

mod format;
mod loader;
mod schema;

pub use format::{format_fixed_abi_fields, format_frame_fields};
pub use loader::{
    load_self_description, load_self_description_json_bytes, load_self_description_with_hash,
    self_description_hash,
};
pub use schema::*;
