use crate::components::{Controller, Navigator, OperationClient_controller_plan};
use crate::messages::{PlanFeedback, PlanGoal, PlanResult};

#[derive(Default)]
pub struct ControllerImpl;

impl Controller for ControllerImpl {
    fn on_tick(&mut self, _plan: &OperationClient_controller_plan) -> flowrt::Status {
        flowrt::Status::ok()
    }
}

#[derive(Default)]
pub struct NavigatorImpl;

impl Navigator for NavigatorImpl {
    fn on_plan_operation(
        &mut self,
        goal: &PlanGoal,
        cancel: flowrt::OperationCancelToken,
        progress: &mut flowrt::OperationProgressPublisher<PlanFeedback>,
    ) -> flowrt::OperationHandlerResult<PlanResult> {
        if cancel.is_canceled() {
            return flowrt::OperationHandlerResult::canceled();
        }

        progress.publish(PlanFeedback { progress: 0.5 });
        progress.publish(PlanFeedback { progress: 1.0 });
        flowrt::OperationHandlerResult::succeeded(PlanResult {
            accepted: goal.target > 0,
        })
    }

    fn on_tick(&mut self) -> flowrt::Status {
        flowrt::Status::ok()
    }
}

pub fn build_app() -> crate::App {
    crate::App::new(
        Box::new(ControllerImpl),
        Box::new(NavigatorImpl),
    )
}
