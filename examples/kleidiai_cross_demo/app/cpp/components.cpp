#include "flowrt_app/runtime_shell.hpp"

#include <cmath>
#include <cstdint>
#include <iostream>
#include <memory>
#include <vector>

#include "kai/ukernels/matmul/matmul_clamp_f32_f32_f32p/kai_matmul_clamp_f32_f32_f32p8x1biasf32_6x8x4_neon_mla.h"
#include "kai/ukernels/matmul/pack/kai_rhs_pack_kxn_f32p8x1biasf32_f32_f32_neon.h"

namespace {

struct MatmulResult {
    std::uint32_t rows;
    std::uint32_t cols;
    std::uint32_t checksum;
    std::uint32_t mismatches;
};

std::uint32_t checksum_f32(const std::vector<float>& values) {
    std::uint32_t hash = 2166136261U;
    for (const float value : values) {
        const auto scaled = static_cast<std::int32_t>(std::lround(value * 1000.0F));
        hash ^= static_cast<std::uint32_t>(scaled);
        hash *= 16777619U;
    }
    return hash;
}

std::vector<float> reference_matmul(
    std::size_t rows, std::size_t cols, std::size_t depth, const std::vector<float>& lhs,
    const std::vector<float>& rhs, const std::vector<float>& bias) {
    std::vector<float> dst(rows * cols);
    for (std::size_t row = 0; row < rows; ++row) {
        for (std::size_t col = 0; col < cols; ++col) {
            float acc = bias[col];
            for (std::size_t k = 0; k < depth; ++k) {
                acc += lhs[row * depth + k] * rhs[k * cols + col];
            }
            dst[row * cols + col] = acc;
        }
    }
    return dst;
}

MatmulResult run_kleidiai_neon_matmul() {
    constexpr std::size_t rows = 6;
    constexpr std::size_t cols = 8;
    constexpr std::size_t depth = 4;

    std::vector<float> lhs(rows * depth);
    std::vector<float> rhs(depth * cols);
    std::vector<float> bias(cols);
    for (std::size_t index = 0; index < lhs.size(); ++index) {
        lhs[index] = static_cast<float>((index % 7U) + 1U) * 0.125F;
    }
    for (std::size_t index = 0; index < rhs.size(); ++index) {
        rhs[index] = static_cast<float>(static_cast<int>(index % 5U) - 2) * 0.25F;
    }
    for (std::size_t index = 0; index < bias.size(); ++index) {
        bias[index] = static_cast<float>(index) * 0.03125F;
    }

    const auto expected = reference_matmul(rows, cols, depth, lhs, rhs, bias);
    const auto nr = kai_get_nr_matmul_clamp_f32_f32_f32p8x1biasf32_6x8x4_neon_mla();
    const auto kr = kai_get_kr_matmul_clamp_f32_f32_f32p8x1biasf32_6x8x4_neon_mla();
    const auto sr = kai_get_sr_matmul_clamp_f32_f32_f32p8x1biasf32_6x8x4_neon_mla();
    const auto rhs_packed_size =
        kai_get_rhs_packed_size_rhs_pack_kxn_f32p8x1biasf32_f32_f32_neon(cols, depth);
    std::vector<std::uint8_t> rhs_packed(rhs_packed_size);

    // KleidiAI 的 matmul kernel 要求 RHS 先按 kernel 布局打包。这里沿用官方
    // kxn packer，FlowRT 只验证真实 arm64 SDK overlay 能被 pkg-config 找到并运行。
    kai_run_rhs_pack_kxn_f32p8x1biasf32_f32_f32_neon(
        1, cols, depth, nr, kr, sr, cols * sizeof(float), rhs.data(), bias.data(), nullptr,
        rhs_packed.data(), 0, nullptr);

    std::vector<float> actual(rows * cols, 0.0F);
    kai_run_matmul_clamp_f32_f32_f32p8x1biasf32_6x8x4_neon_mla(
        rows, cols, depth, lhs.data(), depth * sizeof(float), rhs_packed.data(), actual.data(),
        cols * sizeof(float), sizeof(float), -1000.0F, 1000.0F);

    std::uint32_t mismatches = 0;
    for (std::size_t index = 0; index < actual.size(); ++index) {
        if (std::fabs(actual[index] - expected[index]) > 0.0001F) {
            ++mismatches;
        }
    }

    return MatmulResult {
        static_cast<std::uint32_t>(rows),
        static_cast<std::uint32_t>(cols),
        checksum_f32(actual),
        mismatches,
    };
}

class KleidiaiWorker final : public flowrt_app::KleidiaiWorkerInterface {
public:
    // 周期任务执行一个极小的 NEON matmul；输出摘要而不是矩阵本体，避免 demo 噪声。
    flowrt::Status on_tick(flowrt::Output<flowrt_app::KleidiaiStats>& stats) override {
        const auto result = run_kleidiai_neon_matmul();
        stats.write(flowrt_app::KleidiaiStats {
            result.rows,
            result.cols,
            result.checksum,
            result.mismatches,
        });
        return flowrt::Status::Ok;
    }
};

class StatsSink final : public flowrt_app::StatsSinkInterface {
public:
    // 输出可由 smoke 脚本匹配的单行结果；mismatches 非零说明 SDK 函数运行结果异常。
    flowrt::Status on_tick(const flowrt::Latest<flowrt_app::KleidiaiStats>& stats) override {
        if (!stats.present()) {
            return flowrt::Status::Retry;
        }
        const auto* value = stats.get();
        std::cout << "kleidiai_stats rows=" << value->rows << " cols=" << value->cols
                  << " checksum=" << value->checksum << " mismatches=" << value->mismatches << '\n';
        return value->mismatches == 0 ? flowrt::Status::Ok : flowrt::Status::Error;
    }
};

}  // namespace

namespace flowrt_user {

flowrt_app::App build_app() {
    return flowrt_app::App(std::make_unique<KleidiaiWorker>(), std::make_unique<StatsSink>());
}

}  // namespace flowrt_user
