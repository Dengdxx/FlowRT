#include <flowrt_app/runtime_shell.hpp>

#include <memory>

namespace {

/// 差速控制器：根据航向角 theta 计算左右电机速度差。
///
/// 基准速度 1.0 m/s，航向偏差 theta 以差速方式叠加：
/// - theta > 0（右偏）→ 左轮减速、右轮加速，产生左转力矩
/// - theta < 0（左偏）→ 左轮加速、右轮减速，产生右转力矩
/// - theta = 0（直行）→ 两轮等速
///
/// 里程计未就绪时输出零速度，等待上游数据。
/// 此组件运行在 C++ 进程中，通过 iox2 与 Rust 进程的估计器跨语言通信。
class Controller final : public flowrt_app::ControllerInterface {
public:
    auto on_tick(const flowrt::Latest<flowrt_app::Odom>& odom, flowrt::Output<flowrt_app::MotorCmd>& cmd)
        -> flowrt::Status override {
        if (!odom.present()) {
            // 里程计数据未到达，输出零速度保持静止
            cmd.write(flowrt_app::MotorCmd{
                .left = 0.0F,
                .right = 0.0F,
            });
            return flowrt::Status::Ok;
        }

        const auto* sample = odom.as_ref();
        // 差速控制：基准速度 ± 航向偏差
        cmd.write(flowrt_app::MotorCmd{
            .left = 1.0F - sample->theta,   // 左轮 = 基准 - 偏差
            .right = 1.0F + sample->theta,  // 右轮 = 基准 + 偏差
        });
        return flowrt::Status::Ok;
    }
};

}  // namespace

namespace flowrt_user {

/// 组装 C++ 控制器组件。
auto build_app() -> flowrt_app::App {
    return flowrt_app::App(std::make_unique<Controller>());
}

}  // namespace flowrt_user
