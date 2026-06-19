use std::sync::atomic::{AtomicU64, Ordering};

use crate::components::{
    NavController, Navigator, OperationClient_nav_controller_nav, PlanService, Planner,
};
use crate::messages::{PlanFeedback, PlanGoal, PlanRequest, PlanResponse, PlanResult};

#[derive(Default)]
pub struct PlanServiceImpl {
    request_count: AtomicU64,
}

impl PlanService for PlanServiceImpl {
    fn on_plan_request(&mut self, request: &PlanRequest) -> flowrt::ServiceResult<PlanResponse> {
        self.request_count.fetch_add(1, Ordering::Relaxed);
        flowrt::ServiceResult::ok(PlanResponse {
            accepted: request.goal % 2 == 0,
        })
    }

    fn on_tick(&mut self) -> flowrt::Status {
        flowrt::Status::ok()
    }
}

#[derive(Default)]
pub struct PlannerImpl {
    next_goal: u32,
    pending: Option<(u32, flowrt::ServiceCallHandle<PlanResponse>)>,
}

impl Planner for PlannerImpl {
    fn on_tick(
        &mut self,
        plan: &crate::components::ServiceClient_planner_plan,
        result: &mut flowrt::Output<i32>,
    ) -> flowrt::Status {
        if let Some((goal, handle)) = self.pending.take() {
            if handle.poll() {
                match handle.complete() {
                    flowrt::ServiceResult::Ok(response) => {
                        result.write(if response.accepted {
                            goal as i32
                        } else {
                            -(goal as i32)
                        });
                    }
                    flowrt::ServiceResult::Err(_, _) => result.write(0),
                }
            } else {
                self.pending = Some((goal, handle));
                return flowrt::Status::ok();
            }
        }

        self.next_goal = self.next_goal.saturating_add(1);
        let goal = self.next_goal;
        self.pending = Some((
            goal,
            plan.start_call(PlanRequest { goal }, std::time::Duration::from_millis(500)),
        ));
        flowrt::Status::ok()
    }
}

#[derive(Default)]
pub struct NavControllerImpl;

impl NavController for NavControllerImpl {
    fn on_tick(&mut self, _nav: &OperationClient_nav_controller_nav) -> flowrt::Status {
        flowrt::Status::ok()
    }
}

#[derive(Default)]
pub struct NavigatorImpl;

impl Navigator for NavigatorImpl {
    fn on_nav_operation(
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
        Box::new(NavControllerImpl),
        Box::new(NavigatorImpl),
        Box::new(PlannerImpl::default()),
        Box::new(PlanServiceImpl::default()),
    )
}
