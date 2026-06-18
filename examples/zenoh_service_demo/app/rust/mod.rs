use std::sync::atomic::{AtomicU64, Ordering};

use crate::components::{PlanService, Planner};
use crate::messages::{PlanRequest, PlanResponse};

/// 跨进程 zenoh service server：goal 为偶数时接受。
#[derive(Default)]
pub struct PlanServiceImpl {
    request_count: AtomicU64,
}

impl PlanService for PlanServiceImpl {
    fn on_plan_request(&self, request: &PlanRequest) -> flowrt::ServiceResult<PlanResponse> {
        self.request_count.fetch_add(1, Ordering::Relaxed);
        flowrt::ServiceResult::ok(PlanResponse {
            accepted: request.goal % 2 == 0,
        })
    }

    fn on_tick(&self) -> flowrt::Status {
        flowrt::Status::ok()
    }
}

/// 跨进程 zenoh service client：周期性请求 planner，并把结果写到输出端口。
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

pub fn build_app() -> crate::App {
    crate::App::new(
        Box::new(PlannerImpl::default()),
        Box::new(PlanServiceImpl::default()),
    )
}
