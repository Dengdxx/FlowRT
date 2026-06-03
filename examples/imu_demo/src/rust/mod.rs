use crate::components::{Controller, Estimator, ImuSim, Monitor};
use crate::messages::{Imu, MotorCmd, Odom};
use flowrt::{Latest, Output, Status};

#[derive(Debug, Default)]
struct ImuSimNode {
    tick: u64,
}

impl ImuSim for ImuSimNode {
    fn on_tick(&mut self, imu: &mut Output<Imu>) -> Status {
        self.tick += 1;
        let sample = Imu {
            timestamp: self.tick * 5,
            ax: 0.1,
            ay: 0.0,
            az: 9.81,
            gx: 0.0,
            gy: 0.0,
            gz: 0.01,
        };
        imu.write(sample);
        Status::Ok
    }
}

#[derive(Debug, Default)]
struct EstimatorNode {
    distance: f32,
}

impl Estimator for EstimatorNode {
    fn on_tick(&mut self, imu: Latest<'_, Imu>, odom: &mut Output<Odom>) -> Status {
        let sample = match imu.as_ref() {
            Some(sample) => *sample,
            None => return Status::Retry,
        };
        self.distance += sample.ax * 0.05;
        odom.write(Odom {
            timestamp: sample.timestamp,
            x: self.distance,
            y: 0.0,
            theta: sample.gz * 0.1,
            vx: sample.ax,
            wz: sample.gz,
        });
        Status::Ok
    }
}

#[derive(Debug, Default)]
struct ControllerNode;

impl Controller for ControllerNode {
    fn on_tick(&mut self, odom: Latest<'_, Odom>, cmd: &mut Output<MotorCmd>) -> Status {
        let pose = match odom.as_ref() {
            Some(pose) => *pose,
            None => return Status::Retry,
        };
        let correction = (pose.x * 0.2).clamp(-0.4, 0.4);
        cmd.write(MotorCmd {
            left: 0.8 - correction,
            right: 0.8 + correction,
        });
        Status::Ok
    }
}

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

pub fn build_app() -> crate::App {
    crate::App::new(
        Box::new(ImuSimNode::default()),
        Box::new(EstimatorNode::default()),
        Box::new(ControllerNode::default()),
        Box::new(MonitorNode::default()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_app_runs() {
        let backend = flowrt::inproc_backend();
        let status = build_app().run(&backend);
        assert_eq!(status, Status::Ok);
    }
}
