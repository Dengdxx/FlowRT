use crate::components::{Estimator, ImuSim};
use crate::messages::{Imu, Odom};

#[derive(Default)]
pub struct ImuSimImpl {
    tick: u64,
}

impl ImuSim for ImuSimImpl {
    fn on_tick(&mut self, imu: &mut flowrt::Output<Imu>) -> flowrt::Status {
        self.tick += 1;
        imu.write(Imu {
            timestamp: self.tick,
            ax: 0.1,
            ay: 0.0,
            az: 9.81,
        });
        flowrt::Status::ok()
    }
}

#[derive(Default)]
pub struct EstimatorImpl;

impl Estimator for EstimatorImpl {
    fn on_tick(
        &mut self,
        imu: flowrt::Latest<'_, Imu>,
        odom: &mut flowrt::Output<Odom>,
    ) -> flowrt::Status {
        let Some(imu) = imu.as_ref() else {
            return flowrt::Status::Retry;
        };
        odom.write(Odom {
            timestamp: imu.timestamp,
            x: imu.ax,
            y: imu.ay,
            theta: 0.0,
        });
        flowrt::Status::ok()
    }
}

pub fn build_app() -> crate::App {
    crate::App::new(Box::new(ImuSimImpl::default()), Box::new(EstimatorImpl))
}
