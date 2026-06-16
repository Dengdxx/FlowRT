// FlowRT 管理产物。不要手工修改。

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Imu {
    pub ax: f32,
}

impl Default for Imu {
    fn default() -> Self {
        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Odom {
    pub x: f32,
}

impl Default for Odom {
    fn default() -> Self {
        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。
        unsafe { std::mem::zeroed() }
    }
}

