// 多传感器同步示例的用户实现。imu_src/odom_src 周期产样，fusion 在两路样本按
// sample-time 对齐成同步集时被 on_synchronized 触发，sink 消费融合结果。
use crate::components::{Fusion, ImuSrc, OdomSrc, Sink};
use crate::messages::{Estimate, Imu, Odom};
use flowrt::{Latest, Output, Status};

/// IMU 源：每 10ms 产一个样本，sample-time 单调递增（单位 ns）。
#[derive(Default)]
struct ImuSrcNode {
    stamp_ns: u64,
}

impl ImuSrc for ImuSrcNode {
    fn on_tick(&mut self, imu: &mut Output<Imu>) -> Status {
        self.stamp_ns += 10_000_000;
        imu.write(Imu {
            ax: 0.1,
            stamp_ns: self.stamp_ns,
        });
        Status::Ok
    }
}

/// 里程计源：与 IMU 同节奏产样，sample-time 单位 ns。
#[derive(Default)]
struct OdomSrcNode {
    stamp_ns: u64,
}

impl OdomSrc for OdomSrcNode {
    fn on_tick(&mut self, odom: &mut Output<Odom>) -> Status {
        self.stamp_ns += 10_000_000;
        odom.write(Odom {
            vx: 0.2,
            stamp_ns: self.stamp_ns,
        });
        Status::Ok
    }
}

/// 融合组件：on_synchronized 触发，收到的 imu/odom 已按 sample-time 对齐。
#[derive(Default)]
struct FusionNode;

impl Fusion for FusionNode {
    fn on_tick(
        &mut self,
        imu: Latest<'_, Imu>,
        odom: Latest<'_, Odom>,
        estimate: &mut Output<Estimate>,
    ) -> Status {
        let x = imu.as_ref().map(|s| s.ax).unwrap_or(0.0)
            + odom.as_ref().map(|s| s.vx).unwrap_or(0.0);
        estimate.write(Estimate { x });
        Status::Ok
    }
}

/// 下游 sink：消费融合结果。
#[derive(Default)]
struct SinkNode;

impl Sink for SinkNode {
    fn on_tick(&mut self, estimate: Latest<'_, Estimate>) -> Status {
        let _ = estimate;
        Status::Ok
    }
}

pub fn build_app() -> crate::App {
    crate::App::new(
        Box::new(ImuSrcNode::default()),
        Box::new(OdomSrcNode::default()),
        Box::new(FusionNode::default()),
        Box::new(SinkNode::default()),
    )
}
