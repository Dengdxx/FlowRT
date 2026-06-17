// FlowRT 管理产物。不要手工修改。

pub(crate) mod selfdesc;
pub mod components;
pub mod messages;
pub mod runtime_shell;
pub mod supervisor;
#[path = "../../../app/rust/mod.rs"]
pub mod user;

pub use runtime_shell::{run, run_process, App};
