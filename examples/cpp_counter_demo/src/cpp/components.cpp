#include "flowrt_app/runtime_shell.hpp"

#include <cstdint>
#include <memory>

namespace {

class CounterSource final : public flowrt_app::CounterSourceInterface {
public:
    flowrt::Status on_tick(flowrt::Output<flowrt_app::Count>& count) override {
        ++value_;
        count.write(flowrt_app::Count{value_});
        return flowrt::Status::Ok;
    }

private:
    std::uint32_t value_ = 0;
};

class CounterSink final : public flowrt_app::CounterSinkInterface {
public:
    flowrt::Status on_tick(const flowrt::Latest<flowrt_app::Count>& count) override {
        if (!count.present()) {
            return flowrt::Status::Retry;
        }
        return count.get()->value == 0 ? flowrt::Status::Error : flowrt::Status::Ok;
    }
};

}  // namespace

namespace flowrt_user {

flowrt_app::App build_app() {
    return flowrt_app::App(
        std::make_unique<CounterSource>(),
        std::make_unique<CounterSink>());
}

}  // namespace flowrt_user
