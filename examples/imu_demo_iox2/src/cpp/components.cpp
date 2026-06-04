#include <flowrt_app/runtime_shell.hpp>

#include <memory>

namespace {

class Controller final : public flowrt_app::ControllerInterface {
public:
    auto on_tick(const flowrt::Latest<flowrt_app::Odom>& odom, flowrt::Output<flowrt_app::MotorCmd>& cmd)
        -> flowrt::Status override {
        if (!odom.present()) {
            cmd.write(flowrt_app::MotorCmd{
                .left = 0.0F,
                .right = 0.0F,
            });
            return flowrt::Status::Ok;
        }

        const auto* sample = odom.as_ref();
        cmd.write(flowrt_app::MotorCmd{
            .left = 1.0F - sample->theta,
            .right = 1.0F + sample->theta,
        });
        return flowrt::Status::Ok;
    }
};

}  // namespace

namespace flowrt_user {

auto build_app() -> flowrt_app::App {
    return flowrt_app::App(std::make_unique<Controller>());
}

}  // namespace flowrt_user
