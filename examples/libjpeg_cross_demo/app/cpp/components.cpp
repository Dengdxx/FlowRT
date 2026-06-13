#include "flowrt_app/runtime_shell.hpp"

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <iostream>
#include <memory>
#include <vector>

#include <jpeglib.h>

namespace {

// 该示例验证“公开真实 C/C++ 库通过 pkg-config 进入 FlowRT C++ component”：
// RSDL 只声明 pkg_config = ["libjpeg"]，具体 arm64 头文件和库由外部 SDK overlay 提供。
// 这里使用 libjpeg API；在 Ubuntu/Debian 上该实现来自 libjpeg-turbo 包。
std::vector<unsigned char> make_rgb_pattern(std::uint32_t width, std::uint32_t height) {
    std::vector<unsigned char> rgb(width * height * 3U);
    for (std::uint32_t y = 0; y < height; ++y) {
        for (std::uint32_t x = 0; x < width; ++x) {
            const auto offset = static_cast<std::size_t>((y * width + x) * 3U);
            rgb[offset] = static_cast<unsigned char>((x * 17U + y * 3U) & 0xffU);
            rgb[offset + 1U] = static_cast<unsigned char>((x * 5U + y * 11U) & 0xffU);
            rgb[offset + 2U] = static_cast<unsigned char>((x * 13U + y * 7U) & 0xffU);
        }
    }
    return rgb;
}

flowrt_app::JpegStats encode_pattern_with_libjpeg() {
    constexpr std::uint32_t width = 32;
    constexpr std::uint32_t height = 24;
    auto rgb = make_rgb_pattern(width, height);

    std::uint64_t luma_sum = 0;
    std::uint32_t checksum = 2166136261U;
    for (std::size_t index = 0; index < rgb.size(); index += 3U) {
        const auto red = static_cast<std::uint32_t>(rgb[index]);
        const auto green = static_cast<std::uint32_t>(rgb[index + 1U]);
        const auto blue = static_cast<std::uint32_t>(rgb[index + 2U]);
        luma_sum += (299U * red + 587U * green + 114U * blue) / 1000U;
        checksum ^= red + (green << 8U) + (blue << 16U);
        checksum *= 16777619U;
    }

    // libjpeg 是 C API：jpeg_mem_dest() 会分配输出缓冲区，调用方必须在
    // jpeg_finish_compress() 后释放。示例保持输入图像很小，只验证真实库链接和运行。
    jpeg_compress_struct cinfo {};
    jpeg_error_mgr jerr {};
    cinfo.err = jpeg_std_error(&jerr);
    jpeg_create_compress(&cinfo);

    unsigned char* jpeg_buffer = nullptr;
    unsigned long jpeg_size = 0;
    jpeg_mem_dest(&cinfo, &jpeg_buffer, &jpeg_size);

    cinfo.image_width = width;
    cinfo.image_height = height;
    cinfo.input_components = 3;
    cinfo.in_color_space = JCS_RGB;
    jpeg_set_defaults(&cinfo);
    jpeg_set_quality(&cinfo, 85, TRUE);

    jpeg_start_compress(&cinfo, TRUE);
    const auto row_stride = static_cast<int>(width * 3U);
    while (cinfo.next_scanline < cinfo.image_height) {
        JSAMPROW row_pointer[1] = {
            &rgb[static_cast<std::size_t>(cinfo.next_scanline) * row_stride],
        };
        jpeg_write_scanlines(&cinfo, row_pointer, 1);
    }
    jpeg_finish_compress(&cinfo);
    jpeg_destroy_compress(&cinfo);

    flowrt_app::JpegStats stats {
        static_cast<std::uint32_t>(jpeg_size),
        static_cast<std::uint32_t>((luma_sum * 1000U) / (width * height)),
        checksum,
    };
    std::free(jpeg_buffer);
    return stats;
}

class JpegWorker final : public flowrt_app::JpegWorkerInterface {
public:
    // 生成一帧确定性 RGB 图案并用 libjpeg 压缩；输出统计值供 sink 打印和 smoke 断言。
    flowrt::Status on_tick(flowrt::Output<flowrt_app::JpegStats>& stats) override {
        stats.write(encode_pattern_with_libjpeg());
        return flowrt::Status::Ok;
    }
};

class StatsSink final : public flowrt_app::StatsSinkInterface {
public:
    // sink 只检查上游样本已到达并输出可 grep 的摘要，避免把 demo 变成图像文件处理工具。
    flowrt::Status on_tick(const flowrt::Latest<flowrt_app::JpegStats>& stats) override {
        if (!stats.present()) {
            return flowrt::Status::Retry;
        }
        const auto* value = stats.get();
        std::cout << "libjpeg_stats compressed=" << value->compressed_bytes
                  << " mean_luma_x1000=" << value->mean_luma_x1000
                  << " checksum=" << value->checksum << '\n';
        return value->compressed_bytes == 0 ? flowrt::Status::Error : flowrt::Status::Ok;
    }
};

}  // namespace

namespace flowrt_user {

flowrt_app::App build_app() {
    return flowrt_app::App(std::make_unique<JpegWorker>(), std::make_unique<StatsSink>());
}

}  // namespace flowrt_user
