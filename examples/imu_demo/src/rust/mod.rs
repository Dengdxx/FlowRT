use crate::components::{Estimator, ImuSim, Monitor};
use crate::messages::{Imu, MotorCmd, Odom};
use flowrt::{Latest, Output, Status};

/// IMU 仿真源：以 5ms 周期生成恒定加速度和角速度样本。
///
/// 模拟车辆以 0.1 m/s² 匀加速直行，绕 Z 轴有微小角速度漂移（0.01 rad/s）。
/// 重力分量 az = 9.81 m/s² 表示传感器静止竖直安装。
#[derive(Debug, Default)]
struct ImuSimNode {
    tick: u64,
}

impl ImuSim for ImuSimNode {
    fn on_tick(&mut self, imu: &mut Output<Imu>) -> Status {
        self.tick += 1;
        let sample = Imu {
            timestamp: self.tick * 5, // 5ms 周期，timestamp 单位为毫秒
            ax: 0.1,   // 前向加速度 0.1 m/s²
            ay: 0.0,
            az: 9.81,  // 重力加速度
            gx: 0.0,
            gy: 0.0,
            gz: 0.01,  // 偏航角速度 0.01 rad/s
        };
        imu.write(sample);
        Status::Ok
    }
}

/// 状态估计器：对 IMU 加速度做简单积分，输出累计位移和航向。
///
/// 距离积分公式：`distance += ax * dt`，此处 dt 取 0.05s 作为演示简化值。
/// 航向由角速度 gz 乘以增益 0.1 近似，用于演示差速控制中的航向反馈。
#[derive(Debug, Default)]
struct EstimatorNode {
    distance: f32,
}

impl Estimator for EstimatorNode {
    fn on_tick(&mut self, imu: Latest<'_, Imu>, odom: &mut Output<Odom>) -> Status {
        let sample = match imu.as_ref() {
            Some(sample) => *sample,
            None => return Status::Retry, // IMU 尚未到达，等待下一个 tick
        };
        // 加速度积分：ax * 0.05s = 本 tick 的位移增量
        self.distance += sample.ax * 0.05;
        odom.write(Odom {
            timestamp: sample.timestamp,
            x: self.distance,          // 累计前向位移
            y: 0.0,
            theta: sample.gz * 0.1,    // 航向角（角速度 × 增益）
            vx: sample.ax,             // 瞬时前向速度
            wz: sample.gz,             // 瞬时偏航角速度
        });
        Status::Ok
    }
}

/// 监控节点：统计三个输入通道同时有数据的 tick 数。
///
/// 用于验证多输入 on_message 语义——当所有输入都 present 时才计入观测次数。
/// 如果任意一个输入缺失，该 tick 不计入。
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

/// 组装完整应用：注入三个组件实例，由生成的 runtime shell 管理调度和 channel 连接。
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
    fn demo_app_reports_mixed_runtime_gap() {
        let backend = flowrt::inproc_backend();
        let status = build_app().run(&backend);
        assert_eq!(status, Status::Error);
    }
}
