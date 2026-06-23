#[derive(Default)]
pub struct Controller;
impl crate::components::Controller for Controller {
    fn on_tick(
        &mut self,
        state: flowrt::Latest<'_, crate::messages::State>,
        cmd: &mut flowrt::Output<crate::messages::Cmd>,
    ) -> flowrt::Status {
        let x = state.as_ref().map(|s| s.pose.x).unwrap_or(0.0);
        cmd.write(crate::messages::Cmd { u: -x });
        flowrt::Status::Ok
    }
}
#[derive(Default)]
pub struct Plant;
impl crate::components::Plant for Plant {
    fn on_tick(
        &mut self,
        cmd: flowrt::Latest<'_, crate::messages::Cmd>,
        state: &mut flowrt::Output<crate::messages::State>,
    ) -> flowrt::Status {
        let u = cmd.as_ref().map(|c| c.u).unwrap_or(0.0);
        let mut next = crate::messages::State::default();
        next.pose.x = u;
        state.write(next);
        flowrt::Status::Ok
    }
}
pub fn build_app() -> crate::App {
    crate::App::new(Box::new(Controller::default()), Box::new(Plant::default()))
}
