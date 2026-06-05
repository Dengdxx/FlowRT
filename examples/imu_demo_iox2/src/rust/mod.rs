use crate::components::{Estimator, EstimatorParams, ImuSim, Monitor};
use crate::messages::{Imu, MotorCmd, Odom};
use flowrt::{Latest, Output, Status};

/// IMU 仿真源：与 imu_demo 相同的恒定加速度/角速度样本生成器。
///
/// 本示例中该组件运行在 Rust 进程，通过 iox2 将 IMU 数据发送给
/// C++ 进程的差速控制器，验证跨语言分进程通信。
#[derive(Debug, Default)]
struct ImuSimNode {
    tick: u64,
}

impl ImuSim for ImuSimNode {
    fn on_tick(&mut self, imu: &mut Output<Imu>) -> Status {
        self.tick += 1;
        let sample = Imu {
            timestamp: self.tick * 5, // 5ms 周期
            ax: 0.1,   // 前向加速度 0.1 m/s²
            ay: 0.0,
            az: 9.81,  // 重力加速度
            gx: 0.0,
            gy: 0.0,
            gz: 0.01,  // 偏航角速度
        };
        imu.write(sample);
        Status::Ok
    }
}

/// 状态估计器：对 IMU 加速度积分，输出累计位移和航向。
///
/// 与 imu_demo 相同的积分逻辑，此处运行在 Rust 进程中。
#[derive(Debug, Default)]
struct EstimatorNode {
    distance: f32,
}

impl Estimator for EstimatorNode {
    fn on_tick(
        &mut self,
        imu: Latest<'_, Imu>,
        params: &EstimatorParams,
        odom: &mut Output<Odom>,
    ) -> Status {
        let sample = match imu.as_ref() {
            Some(sample) => *sample,
            None => return Status::Retry,
        };
        self.distance += sample.ax * 0.05; // 加速度积分
        let vertical_accel = sample.az - params.gravity;
        odom.write(Odom {
            timestamp: sample.timestamp,
            x: self.distance,
            y: vertical_accel * 0.05,
            theta: sample.gz * 0.1,
            vx: sample.ax,
            wz: sample.gz,
        });
        Status::Ok
    }
}

/// 监控节点：统计三个输入同时有数据的 tick 数。
///
/// 验证 iox2 跨进程场景下多输入 on_message 语义的正确性。
#[derive(Debug, Default)]
struct MonitorNode {
    observed: u64,
}

impl Monitor for MonitorNode {
    fn on_tick(
        &mut self,
        imu: Latest<'_, Imu>,
        odom: Latest<'_, Odom>,
        cmd: Latest<'_, MotorCmd>,
    ) -> Status {
        if imu.present() && odom.present() && cmd.present() {
            self.observed += 1;
        }
        Status::Ok
    }
}

/// 组装 Rust 侧应用：IMU 仿真源 + 状态估计器 + 监控节点。
pub fn build_app() -> crate::App {
    crate::App::new(
        Box::new(ImuSimNode::default()),
        Box::new(EstimatorNode::default()),
        Box::new(MonitorNode::default()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_app_runs_under_iox2_backend() {
        let backend = flowrt::iox2_backend();
        let status = build_app().run(&backend);
        assert_eq!(status, Status::Ok);
    }
}
