// FlowRT 管理产物。不要手工修改。

use flowrt::ZeroCopySend;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, flowrt::ZeroCopySend)]
#[type_name("PlanRequest")]
pub struct PlanRequest {
    pub goal: u32,
}

impl Default for PlanRequest {
    fn default() -> Self {
        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, flowrt::ZeroCopySend)]
#[type_name("PlanResponse")]
pub struct PlanResponse {
    pub accepted: bool,
}

impl Default for PlanResponse {
    fn default() -> Self {
        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。
        unsafe { std::mem::zeroed() }
    }
}
