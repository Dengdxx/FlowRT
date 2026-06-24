#include "flowrt_app/runtime_shell.hpp"

#include "control_gain.hpp"

#include <memory>

namespace {

struct ControlProcessor final : flowrt_app::ControlProcessorInterface {
    auto on_tick(const flowrt::Latest<flowrt_app::PerceptionSample> &sample,
                 flowrt::Output<flowrt_app::ControlSample> &command) -> flowrt::Status override {
        const auto *value = sample.as_ref();
        if (value == nullptr) {
            return flowrt::Status::Retry;
        }

        command.write(flowrt_app::ControlSample{
            .command = value->ax * workspace_demo::control::kCommandGain,
        });
        return flowrt::Status::Ok;
    }
};

}  // namespace

namespace flowrt_user {

auto build_app() -> flowrt_app::App {
    return flowrt_app::App(std::make_unique<ControlProcessor>());
}

}  // namespace flowrt_user
