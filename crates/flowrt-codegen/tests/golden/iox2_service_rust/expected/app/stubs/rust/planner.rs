// FlowRT 管理参考模板（Rust）。可删除重建；复制到用户 app/ 后再修改。

#[derive(Default)]
pub struct Planner;

impl flowrt_app::components::Planner for Planner {
    fn on_tick(
        &mut self,
        plan: &flowrt_app::components::ServiceClient_planner_plan,
    ) -> flowrt::Status {
        flowrt::Status::Ok
    }
}

pub fn build_app() -> flowrt_app::runtime_shell::App {
    flowrt_app::runtime_shell::App::new(Box::new(Planner::default()))
}
