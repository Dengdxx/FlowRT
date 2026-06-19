// FlowRT 管理产物。不要手工修改。

use flowrt::ZeroCopySend;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, flowrt::ZeroCopySend)]
#[type_name("PlanFeedback")]
pub struct PlanFeedback {
    pub progress: f32,
}

impl Default for PlanFeedback {
    fn default() -> Self {
        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, flowrt::ZeroCopySend)]
#[type_name("PlanGoal")]
pub struct PlanGoal {
    pub target: u32,
}

impl Default for PlanGoal {
    fn default() -> Self {
        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, flowrt::ZeroCopySend)]
#[type_name("PlanResult")]
pub struct PlanResult {
    pub accepted: bool,
}

impl Default for PlanResult {
    fn default() -> Self {
        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。
        unsafe { std::mem::zeroed() }
    }
}
