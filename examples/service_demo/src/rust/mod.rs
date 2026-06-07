use crate::components::{PlanService, Planner};
use crate::messages::{PlanRequest, PlanResponse};

/// Service server 实现：接收 PlanRequest，根据 goal 值决定是否接受。
///
/// goal 为偶数时接受（accepted = true），奇数时拒绝。
/// 这是一个最小示例，展示 `on_plan_request` handler 的返回语义。
#[derive(Default)]
pub struct PlanServiceImpl {
    request_count: u64,
}

impl PlanService for PlanServiceImpl {
    fn on_plan_request(&mut self, request: &PlanRequest) -> flowrt::ServiceResult<PlanResponse> {
        self.request_count += 1;
        let accepted = request.goal % 2 == 0;
        let reason = if accepted {
            format!("goal {} accepted (total: {})", request.goal, self.request_count)
        } else {
            format!("goal {} rejected: odd number", request.goal)
        };
        flowrt::ServiceResult::ok(PlanResponse {
            accepted,
            reason,
        })
    }
}

/// Service client 实现：周期性调用 plan service，将结果写入输出端口。
///
/// 每次 on_tick 发起一次同步 service call，根据结果更新输出值。
/// 展示 `ServiceClient_planner_plan` typed handle 的 `call()` 用法。
#[derive(Default)]
pub struct PlannerImpl {
    tick: u64,
}

impl Planner for PlannerImpl {
    fn on_tick(
        &mut self,
        plan: &crate::components::ServiceClient_planner_plan,
        result: &mut flowrt::Output<i32>,
    ) -> flowrt::Status {
        self.tick += 1;
        let goal = self.tick as u32;
        match plan.call(PlanRequest { goal }, std::time::Duration::from_millis(500)) {
            flowrt::ServiceResult::Ok(resp) => {
                result.write(if resp.accepted { goal as i32 } else { -(goal as i32) });
            }
            flowrt::ServiceResult::Err(_code, _msg) => {
                // service 调用失败时写 0，不影响调度继续
                result.write(0);
            }
        }
        flowrt::Status::ok()
    }
}

/// 组装应用：注入 service server 和 client 两个组件。
pub fn build_app() -> crate::App {
    crate::App::new(
        Box::new(PlanServiceImpl::default()),
        Box::new(PlannerImpl::default()),
    )
}
