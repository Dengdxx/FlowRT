// 反馈环（cyclic graph）示例的用户实现。controller↔plant 构成闭环：
// controller 读 plant 状态算控制量 cmd，plant 读 cmd 更新状态 state。
// plant.state→controller.state 的回边标了 feedback=true：作单位延迟 z⁻¹，
// controller 每拍读到的是 plant 上一拍的状态，启动期（tick 0）读到零初值。
use crate::components::{Controller, Plant};
use crate::messages::{Cmd, State};
use flowrt::{Latest, Output, Status};

/// 比例控制器：把状态 x 拉回 0，输出 u = -kp * x。
#[derive(Default)]
struct ControllerNode;

impl Controller for ControllerNode {
    fn on_tick(&mut self, state: Latest<'_, State>, cmd: &mut Output<Cmd>) -> Status {
        let kp = 0.5;
        let x = state.as_ref().map(|s| s.x).unwrap_or(0.0);
        cmd.write(Cmd { u: -kp * x });
        Status::Ok
    }
}

/// 一阶被控对象：在上一状态基础上叠加控制量，x_next = x + u。
#[derive(Default)]
struct PlantNode {
    x: f64,
}

impl Plant for PlantNode {
    fn on_tick(&mut self, cmd: Latest<'_, Cmd>, state: &mut Output<State>) -> Status {
        let u = cmd.as_ref().map(|c| c.u).unwrap_or(0.0);
        self.x += u;
        state.write(State { x: self.x });
        Status::Ok
    }
}

pub fn build_app() -> crate::App {
    crate::App::new(
        Box::new(ControllerNode::default()),
        Box::new(PlantNode::default()),
    )
}
