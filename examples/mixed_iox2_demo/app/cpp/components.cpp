#include <flowrt_app/runtime_shell.hpp>

#include <memory>

namespace {

/// 数据消费端：接收 Rust 进程通过 iox2 发来的 Sample，校验值非零。
///
/// 当收到 value == 0 时返回 Error 终止调度，用于验证：
/// 1. iox2 跨语言传输的正确性（Rust 发布 → C++ 接收）
/// 2. C++ runtime shell 的错误传播机制
/// 3. 分进程场景下的 shutdown 逆序清理
class Sink final : public flowrt_app::SinkInterface {
public:
    auto on_tick(const flowrt::Latest<flowrt_app::Sample>& sample) -> flowrt::Status override {
        if (!sample.present()) {
            return flowrt::Status::Ok; // 数据尚未到达，跳过本 tick
        }
        // 正常情况下 value 从 1 开始递增，不会为 0；
        // 收到 0 表示数据异常，返回 Error 触发调度器停止。
        return sample.as_ref()->value == 0U ? flowrt::Status::Error : flowrt::Status::Ok;
    }
};

}  // namespace

namespace flowrt_user {

/// 组装 C++ 侧应用：仅包含数据消费端。
auto build_app() -> flowrt_app::App {
    return flowrt_app::App(std::make_unique<Sink>());
}

}  // namespace flowrt_user
