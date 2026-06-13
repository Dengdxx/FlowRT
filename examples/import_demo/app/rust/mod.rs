use crate::components::{Estimator, ImuSim};
use crate::messages::{Imu, Odom};

/// IMU 仿真源：生成恒定加速度样本，用于验证模块化 RSDL imports 展开。
///
/// 该示例的 RSDL 使用 `[package.imports]` 将类型、组件、图等拆分到多个文件，
/// 此组件验证 codegen 在 imports 展开后仍能正确生成 runtime shell。
#[derive(Default)]
pub struct ImuSimImpl {
    tick: u64,
}

impl ImuSim for ImuSimImpl {
    fn on_tick(&mut self, imu: &mut flowrt::Output<Imu>) -> flowrt::Status {
        self.tick += 1;
        imu.write(Imu {
            timestamp: self.tick,
            ax: 0.1,   // 恒定前向加速度
            ay: 0.0,
            az: 9.81,  // 重力加速度
        });
        flowrt::Status::ok()
    }
}

/// 状态估计器：将 IMU 加速度直接映射为里程计位置。
///
/// 简化处理：x 直接取 ax，y 直接取 ay，不做积分。仅用于验证多文件 imports
/// 场景下组件接口生成和 channel 转发的正确性。
#[derive(Default)]
pub struct EstimatorImpl;

impl Estimator for EstimatorImpl {
    fn on_tick(
        &mut self,
        imu: flowrt::Latest<'_, Imu>,
        odom: &mut flowrt::Output<Odom>,
    ) -> flowrt::Status {
        let Some(imu) = imu.as_ref() else {
            return flowrt::Status::Retry; // IMU 尚未到达
        };
        odom.write(Odom {
            timestamp: imu.timestamp,
            x: imu.ax,    // 直接映射，非积分
            y: imu.ay,
            theta: 0.0,
        });
        flowrt::Status::ok()
    }
}

/// 组装应用：注入 IMU 仿真源和状态估计器两个组件。
pub fn build_app() -> crate::App {
    crate::App::new(Box::new(ImuSimImpl::default()), Box::new(EstimatorImpl))
}
