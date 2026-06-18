// FlowRT 管理产物。不要手工修改。
#include "flowrt_app/runtime_shell.hpp"

#include <charconv>
#include <cstddef>
#include <optional>
#include <string_view>
#include <system_error>

int main(int argc, char** argv) {
    std::string_view process;
    std::optional<std::size_t> run_ticks;
    for (int index = 1; index < argc; ++index) {
        const std::string_view arg(argv[index]);
        if (arg == "--process") {
            if (index + 1 >= argc) {
                return 2;
            }
            process = argv[++index];
        } else if (arg == "--flowrt-run-ticks" || arg == "--flowrt-run-steps") {
            if (index + 1 >= argc) {
                return 2;
            }
            const std::string_view raw(argv[++index]);
            std::size_t ticks = 0;
            const auto result = std::from_chars(raw.data(), raw.data() + raw.size(), ticks);
            if (result.ec != std::errc{} || result.ptr != raw.data() + raw.size() || ticks == 0) {
                return 2;
            }
            run_ticks = ticks;
        } else {
            return 2;
        }
    }

    const auto status = process.empty() ? flowrt_app::run(run_ticks) : flowrt_app::run_process(process, run_ticks);
    return status == flowrt::Status::Ok ? 0 : 1;
}
