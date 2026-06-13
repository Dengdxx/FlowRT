#include "flowrt_app/runtime_shell.hpp"

#include <cstdint>
#include <memory>

namespace {

/// 计数器源：每 tick 递增并发布当前计数值。
///
/// 验证 C++ only contract 的 codegen 路径——纯 C++ 组件通过 CMake 构建，
/// 不依赖 Cargo 或 Rust runtime shell。
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

/// 计数器消费端：读取计数值，当值为 0 时返回 Error 终止调度。
///
/// 首次 tick 时计数值为 1（源先递增再发布），不会触发 Error。
/// 此逻辑用于验证 C++ runtime shell 的生命周期管理和错误传播。
class CounterSink final : public flowrt_app::CounterSinkInterface {
public:
    flowrt::Status on_tick(const flowrt::Latest<flowrt_app::Count>& count) override {
        if (!count.present()) {
            return flowrt::Status::Retry; // 计数值尚未到达
        }
        // 正常情况下计数值从 1 开始递增，不会为 0；
        // 如果收到 0 说明数据异常，返回 Error 终止调度。
        return count.get()->value == 0 ? flowrt::Status::Error : flowrt::Status::Ok;
    }
};

}  // namespace

namespace flowrt_user {

/// 组装 C++ 应用：注入计数器源和消费端。
flowrt_app::App build_app() {
    return flowrt_app::App(
        std::make_unique<CounterSource>(),
        std::make_unique<CounterSink>());
}

}  // namespace flowrt_user
