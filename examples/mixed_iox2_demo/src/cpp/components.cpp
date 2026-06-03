#include <flowrt_app/runtime_shell.hpp>

#include <memory>

namespace {

class Sink final : public flowrt_app::SinkInterface {
public:
    auto on_tick(const flowrt::Latest<flowrt_app::Sample>& sample) -> flowrt::Status override {
        if (!sample.present()) {
            return flowrt::Status::Ok;
        }
        return sample.as_ref()->value == 0U ? flowrt::Status::Error : flowrt::Status::Ok;
    }
};

}  // namespace

namespace flowrt_user {

auto build_app() -> flowrt_app::App {
    return flowrt_app::App(std::make_unique<Sink>());
}

}  // namespace flowrt_user
