// FlowRT 管理产物。不要手工修改。

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cmd {
    pub u: f64,
}

impl Default for Cmd {
    fn default() -> Self {
        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub x: f64,
    pub y: f64,
}

impl Default for Pose {
    fn default() -> Self {
        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub pose: Pose,
    pub covariance: [f64; 4],
    pub quality: u8,
}

impl Default for State {
    fn default() -> Self {
        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。
        unsafe { std::mem::zeroed() }
    }
}
