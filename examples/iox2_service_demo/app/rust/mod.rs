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
            accepted: !request.goal.is_empty(),
            detail: if request.goal.len() <= 4 {
                "ok".to_string()
            } else {
                "retry".to_string()
            },
        })
    }

    fn on_tick(&mut self) -> flowrt::Status {
        flowrt::Status::ok()
    }
}

#[derive(Default)]
pub struct PlannerImpl {
    next_goal: u32,
    pending: Option<(i32, flowrt::ServiceCallHandle<PlanResponse>)>,
}

impl Planner for PlannerImpl {
    fn on_tick(
        &mut self,
        plan: &crate::components::ServiceClient_planner_plan,
        result: &mut flowrt::Output<i32>,
    ) -> flowrt::Status {
        if let Some((goal_value, handle)) = self.pending.take() {
            if handle.poll() {
                match handle.complete() {
                    flowrt::ServiceResult::Ok(response) => {
                        result.write(if response.accepted {
                            goal_value
                        } else {
                            -goal_value
                        });
                    }
                    flowrt::ServiceResult::Err(_, _) => result.write(0),
                }
            } else {
                self.pending = Some((goal_value, handle));
                return flowrt::Status::ok();
            }
        }

        self.next_goal = self.next_goal.saturating_add(1);
        let goal_value = (self.next_goal % 1000) as i32;
        let goal = format!("g{goal_value}");
        self.pending = Some((
            goal_value,
            plan.start_call(PlanRequest { goal }, std::time::Duration::from_millis(500)),
        ));
        flowrt::Status::ok()
    }
}

#[derive(Default)]
pub struct NavControllerImpl {
    nav_started: bool,
}

impl NavController for NavControllerImpl {
    fn on_tick(&mut self, nav: &OperationClient_nav_controller_nav) -> flowrt::Status {
        if self.nav_started {
            return flowrt::Status::ok();
        }

        if let Ok(ack) = nav.start(
            PlanGoal {
                target: "dock".to_string(),
            },
            std::time::Duration::from_millis(500),
        ) {
            self.nav_started = ack.accepted;
        }

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
            accepted: !goal.target.is_empty(),
        })
    }

    fn on_tick(&mut self) -> flowrt::Status {
        flowrt::Status::ok()
    }
}

pub fn build_app() -> crate::App {
    crate::App::new(
        Box::new(NavControllerImpl::default()),
        Box::new(NavigatorImpl),
        Box::new(PlannerImpl::default()),
        Box::new(PlanServiceImpl::default()),
    )
}
