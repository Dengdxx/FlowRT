#pragma once

#include <atomic>
#include <chrono>
#include <csignal>
#include <cstdint>
#include <functional>
#include <map>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace flowrt {

/**
 * @brief 组件回调和调度步骤的统一返回状态。
 *
 * 生成的 runtime shell 通过该状态决定是否继续当前调度循环。算法代码不应抛出异常来表达
 * FlowRT 语义错误；需要重试或停止时返回对应状态。
 */
enum class Status : std::uint8_t {
    Ok = 0,     ///< 本次步骤完成，调度器可以继续执行后续 tick。
    Retry = 1,  ///< 本次步骤未完成，调用方可按调度策略稍后重试。
    Error = 2,  ///< 本次步骤失败，调度器应停止当前运行序列并向上报告。
};

/**
 * @brief 返回成功状态的便捷函数。
 *
 * @return `Status::Ok`。
 */
constexpr Status ok() noexcept { return Status::Ok; }

/**
 * @brief frame descriptor 中的开放 metadata 键值。
 */
using FrameMetadata = std::map<std::string, std::string>;

/**
 * @brief RSDL frame descriptor message 的标准 fixed ABI 字段集合。
 *
 * 该结构和 validator 要求的 message 字段一一对应，可作为 generated message 与
 * recorder/lease helper 之间的稳定中间形状。真实 payload 仍由 side-channel 管理。
 */
struct FrameDescriptorFields {
    std::uint64_t resource_id_hash = 0;
    std::uint32_t slot = 0;
    std::uint64_t generation = 0;
    std::uint64_t size_bytes = 0;
    std::uint64_t timestamp_unix_ns = 0;
    std::uint32_t width = 0;
    std::uint32_t height = 0;
    std::uint32_t stride_bytes = 0;
    std::uint32_t format_id = 0;
    std::uint32_t encoding_id = 0;
    std::uint32_t flags = 0;

    [[nodiscard]] std::string resource_id_string() const {
        return std::to_string(resource_id_hash);
    }

    [[nodiscard]] std::string slot_string() const { return std::to_string(slot); }

    [[nodiscard]] FrameMetadata metadata() const {
        return FrameMetadata{
            {"timestamp_unix_ns", std::to_string(timestamp_unix_ns)},
            {"width", std::to_string(width)},
            {"height", std::to_string(height)},
            {"stride_bytes", std::to_string(stride_bytes)},
            {"format_id", std::to_string(format_id)},
            {"encoding_id", std::to_string(encoding_id)},
            {"flags", std::to_string(flags)},
        };
    }
};

/**
 * @brief side-channel 资源中的一个可寻址 payload slot。
 */
struct ResourceDescriptor {
    std::string resource_id;
    std::string slot;
    std::uint64_t generation = 0;
};

/**
 * @brief 普通 FlowRT channel 传递的 frame descriptor。
 *
 * descriptor 只携带 resource/slot/generation、大小、格式、编码和 metadata；真实 payload
 * 生命周期由 I/O boundary 或 external package 管理。
 */
class FrameDescriptor {
   public:
    static FrameDescriptor make(ResourceDescriptor resource, std::uint64_t size_bytes,
                                std::string format, std::string encoding,
                                FrameMetadata metadata) {
        return FrameDescriptor{std::move(resource), size_bytes, std::move(format),
                               std::move(encoding), std::move(metadata)};
    }

    [[nodiscard]] const ResourceDescriptor &resource() const noexcept { return resource_; }

    [[nodiscard]] std::uint64_t size_bytes() const noexcept { return size_bytes_; }

    [[nodiscard]] const std::string &format() const noexcept { return format_; }

    [[nodiscard]] const std::string &encoding() const noexcept { return encoding_; }

    [[nodiscard]] const FrameMetadata &metadata() const noexcept { return metadata_; }

    [[nodiscard]] static FrameDescriptor from_fields(const FrameDescriptorFields &fields) {
        return FrameDescriptor::make(
            ResourceDescriptor{.resource_id = fields.resource_id_string(),
                               .slot = fields.slot_string(),
                               .generation = fields.generation},
            fields.size_bytes, std::to_string(fields.format_id), std::to_string(fields.encoding_id),
            fields.metadata());
    }

   private:
    FrameDescriptor(ResourceDescriptor resource, std::uint64_t size_bytes, std::string format,
                    std::string encoding, FrameMetadata metadata)
        : resource_(std::move(resource)),
          size_bytes_(size_bytes),
          format_(std::move(format)),
          encoding_(std::move(encoding)),
          metadata_(std::move(metadata)) {}

    ResourceDescriptor resource_;
    std::uint64_t size_bytes_ = 0;
    std::string format_;
    std::string encoding_;
    FrameMetadata metadata_;
};

/**
 * @brief side-channel lease 当前状态。
 */
enum class FrameLeaseStatus : std::uint8_t {
    Attached = 0,
    Acquired = 1,
    Released = 2,
    Expired = 3,
    GenerationMismatch = 4,
    Error = 5,
};

/**
 * @brief side-channel lease 操作错误。
 */
enum class FrameLeaseError : std::uint8_t {
    None = 0,
    Released = 1,
    Expired = 2,
    GenerationMismatch = 3,
    Error = 4,
};

/**
 * @brief 无硬件 side-channel lease primitive。
 *
 * 该类型只表达 attach/acquire/release 的状态转换，不打开真实 SHM 或设备。
 */
class FrameLease {
   public:
    FrameLease(FrameDescriptor descriptor, std::uint64_t current_generation)
        : descriptor_(std::move(descriptor)), current_generation_(current_generation) {}

    [[nodiscard]] const FrameDescriptor &descriptor() const noexcept { return descriptor_; }

    [[nodiscard]] FrameLeaseStatus status() const noexcept { return status_; }

    [[nodiscard]] const std::string &last_error() const noexcept { return last_error_; }

    FrameLeaseError acquire(std::uint64_t expected_generation) {
        if (status_ == FrameLeaseStatus::Released) {
            return FrameLeaseError::Released;
        }
        if (status_ == FrameLeaseStatus::Expired) {
            return FrameLeaseError::Expired;
        }
        if (status_ == FrameLeaseStatus::Error) {
            return FrameLeaseError::Error;
        }
        if (expected_generation != current_generation_ ||
            descriptor_.resource().generation != current_generation_) {
            status_ = FrameLeaseStatus::GenerationMismatch;
            return FrameLeaseError::GenerationMismatch;
        }
        status_ = FrameLeaseStatus::Acquired;
        return FrameLeaseError::None;
    }

    FrameLeaseError release() {
        if (status_ == FrameLeaseStatus::Expired) {
            return FrameLeaseError::Expired;
        }
        if (status_ == FrameLeaseStatus::Error) {
            return FrameLeaseError::Error;
        }
        status_ = FrameLeaseStatus::Released;
        return FrameLeaseError::None;
    }

    void expire() noexcept { status_ = FrameLeaseStatus::Expired; }

    void fail(std::string error) {
        last_error_ = std::move(error);
        status_ = FrameLeaseStatus::Error;
    }

   private:
    FrameDescriptor descriptor_;
    std::uint64_t current_generation_ = 0;
    FrameLeaseStatus status_ = FrameLeaseStatus::Attached;
    std::string last_error_;
};

/**
 * @brief I/O boundary 声明资源的运行态状态。
 */
struct BoundaryResourceStatus {
    std::string name;
    std::string kind;
    bool ready = false;
    std::optional<std::string> message;
    std::optional<std::string> last_error;
    std::optional<std::uint64_t> updated_unix_ms;
};

/**
 * @brief 单个 I/O boundary component 的运行态健康状态。
 */
struct BoundaryStatus {
    std::string name;
    std::string component;
    bool ready = false;
    bool healthy = true;
    std::optional<std::string> last_error;
    std::vector<BoundaryResourceStatus> resources;
    std::optional<std::uint64_t> updated_unix_ms;
};

/**
 * @brief I/O boundary 记录 descriptor 事件的结果。
 */
struct BoundaryRecordOutcome {
    bool recorded = false;
    bool dropped = false;
};

namespace detail {

inline std::uint64_t boundary_unix_time_ms() {
    const auto now = std::chrono::system_clock::now().time_since_epoch();
    const auto millis = std::chrono::duration_cast<std::chrono::milliseconds>(now).count();
    return millis < 0 ? 0U : static_cast<std::uint64_t>(millis);
}

}  // namespace detail

/**
 * @brief I/O boundary 组件可用的运行态上报上下文。
 *
 * 该类型只表达 FlowRT 的资源、readiness 和 health 语义，不暴露串口、SHM、网络或 backend
 * SDK 句柄。真实 I/O 仍由用户代码管理。
 */
class BoundaryContext {
   public:
    using Reporter = std::function<void(BoundaryStatus)>;
    using DescriptorReporter = std::function<BoundaryRecordOutcome(
        std::string_view, const FrameDescriptor &, FrameLeaseStatus, bool)>;

    BoundaryContext() = default;

    BoundaryContext(std::string instance, std::string component,
                    std::vector<BoundaryResourceStatus> resources, Reporter reporter,
                    DescriptorReporter descriptor_reporter = {})
        : status_(BoundaryStatus{
              .name = std::move(instance),
              .component = std::move(component),
              .resources = std::move(resources),
          }),
          reporter_(std::move(reporter)),
          descriptor_reporter_(std::move(descriptor_reporter)) {}

    [[nodiscard]] const std::string &instance() const noexcept { return status_.name; }

    [[nodiscard]] const std::string &component() const noexcept { return status_.component; }

    void mark_ready() {
        status_.ready = true;
        touch_and_report();
    }

    void mark_not_ready() {
        status_.ready = false;
        touch_and_report();
    }

    void report_healthy() {
        status_.healthy = true;
        status_.last_error.reset();
        touch_and_report();
    }

    void report_error(std::string error) {
        status_.healthy = false;
        status_.last_error = std::move(error);
        touch_and_report();
    }

    void mark_resource_ready(std::string_view resource) {
        auto &entry = resource_entry(resource);
        entry.ready = true;
        entry.message.reset();
        entry.last_error.reset();
        entry.updated_unix_ms = detail::boundary_unix_time_ms();
        touch_and_report();
    }

    void mark_resource_not_ready(std::string_view resource, std::string message) {
        auto &entry = resource_entry(resource);
        entry.ready = false;
        entry.message = std::move(message);
        entry.updated_unix_ms = detail::boundary_unix_time_ms();
        touch_and_report();
    }

    void report_resource_error(std::string_view resource, std::string error) {
        auto &entry = resource_entry(resource);
        entry.ready = false;
        entry.last_error = std::move(error);
        entry.updated_unix_ms = detail::boundary_unix_time_ms();
        status_.healthy = false;
        touch_and_report();
    }

    BoundaryRecordOutcome record_frame_descriptor_event(std::string_view name,
                                                        const FrameDescriptor &descriptor,
                                                        FrameLeaseStatus status,
                                                        bool payload_recording) const {
        if (!descriptor_reporter_) {
            return BoundaryRecordOutcome{};
        }
        return descriptor_reporter_(name, descriptor, status, payload_recording);
    }

    BoundaryRecordOutcome record_frame_descriptor_fields_event(
        std::string_view name, const FrameDescriptorFields &descriptor, FrameLeaseStatus status,
        bool payload_recording) const {
        return record_frame_descriptor_event(name, FrameDescriptor::from_fields(descriptor), status,
                                             payload_recording);
    }

    BoundaryRecordOutcome record_frame_descriptor_acquired(std::string_view name,
                                                           const FrameDescriptor &descriptor,
                                                           bool payload_recording) const {
        return record_frame_descriptor_event(name, descriptor, FrameLeaseStatus::Acquired,
                                             payload_recording);
    }

    BoundaryRecordOutcome record_frame_descriptor_released(std::string_view name,
                                                           const FrameDescriptor &descriptor,
                                                           bool payload_recording) const {
        return record_frame_descriptor_event(name, descriptor, FrameLeaseStatus::Released,
                                             payload_recording);
    }

   private:
    BoundaryResourceStatus &resource_entry(std::string_view resource) {
        for (auto &entry : status_.resources) {
            if (entry.name == resource) {
                return entry;
            }
        }
        status_.resources.push_back(BoundaryResourceStatus{.name = std::string{resource}});
        return status_.resources.back();
    }

    void touch_and_report() {
        status_.updated_unix_ms = detail::boundary_unix_time_ms();
        if (reporter_) {
            reporter_(status_);
        }
    }

    BoundaryStatus status_;
    Reporter reporter_;
    DescriptorReporter descriptor_reporter_;
};

/**
 * @brief task timing 使用的毫秒时间来源。
 *
 * `Runtime` 表示 generated scheduler 维护的运行态单调毫秒模型；`Replay` 表示
 * `flowrt replay` 或 temporary island overlay 使用 fixture `at_ms` 驱动同一时间模型。
 * 该枚举只标记来源，不在 runtime primitive 中采样 wall-clock 时间。
 */
enum class ClockSource : std::uint8_t {
    Runtime,
    Replay,
};

/**
 * @brief 单次 task 回调可观察的调度时间上下文。
 *
 * 该结构由 generated scheduler 或测试注入，runtime primitive 本身不读取 system time。
 * `scheduled_*` 表达 scheduler 计划时间，`observed_*` 表达回调边界观察到的 runtime
 * 毫秒时间；replay 模式下这些值来自 fixture `at_ms` 驱动的同一时间模型。
 */
struct TaskTiming {
    std::uint64_t step{};                            ///< 当前 scheduler step 序号。
    std::string task_name;                           ///< 当前执行的 task 名称。
    std::string trigger;                             ///< task trigger 名称。
    ClockSource clock_source{ClockSource::Runtime};  ///< 本次 timing 的时间来源。
    std::uint64_t scheduled_time_ms{};       ///< scheduler 计划执行该 task 的毫秒时间。
    std::uint64_t observed_time_ms{};        ///< runtime 在回调边界观察到的毫秒时间。
    std::uint64_t scheduled_delta_ms{};      ///< 相对上一计划时间的毫秒间隔。
    std::uint64_t observed_delta_ms{};       ///< 相对上一观察时间的毫秒间隔。
    std::optional<std::uint64_t> period_ms;  ///< periodic task 的声明周期。
    std::optional<std::uint64_t> deadline_ms;  ///< task 的声明 deadline。
    std::uint64_t lateness_ms{};               ///< 迟到毫秒数；未迟到时为 0。
    std::uint64_t missed_periods{};            ///< scheduler 已跳过的周期数。
    bool deadline_missed{};                    ///< 本次回调是否超过声明 deadline。
    bool overrun{};  ///< 本次执行是否超过声明周期或调度窗口。
};

/**
 * @brief runtime 传递给生命周期钩子和调度步骤的上下文。
 *
 * 普通组件看到空上下文；I/O boundary 组件会收到带 `BoundaryContext` 的上下文，用于上报
 * 资源、readiness 和 health。task 回调可额外收到 `TaskTiming`，生命周期上下文默认不携带
 * timing。上下文不暴露底层 backend SDK。
 */
class Context {
   public:
    Context() = default;

    static Context with_timing(TaskTiming timing) {
        Context context;
        context.set_timing(std::move(timing));
        return context;
    }

    static Context for_boundary(BoundaryContext boundary) {
        Context context;
        context.boundary_ = std::move(boundary);
        return context;
    }

    [[nodiscard]] BoundaryContext *boundary() noexcept {
        return boundary_ ? std::addressof(*boundary_) : nullptr;
    }

    [[nodiscard]] const BoundaryContext *boundary() const noexcept {
        return boundary_ ? std::addressof(*boundary_) : nullptr;
    }

    [[nodiscard]] bool is_io_boundary() const noexcept { return boundary_.has_value(); }

    void set_timing(TaskTiming timing) { timing_ = std::move(timing); }

    [[nodiscard]] const TaskTiming *timing() const noexcept {
        return timing_ ? std::addressof(*timing_) : nullptr;
    }

   private:
    std::optional<BoundaryContext> boundary_;
    std::optional<TaskTiming> timing_;
};

/**
 * @brief 调度循环可查询的关闭请求。
 *
 * token 可以由 Unix signal handler 驱动，也可以由测试或更高层 runtime 代码手动 request。
 * `is_requested()` 为 true 时，generated shell 应退出 tick loop，并继续执行 shutdown task 与
 * 生命周期清理。
 */
class ShutdownToken {
   public:
    ShutdownToken() : requested_(std::make_shared<std::atomic_bool>(false)) {}

    static ShutdownToken new_for_test() { return ShutdownToken{}; }

    void request() const noexcept {
        if (requested_) {
            requested_->store(true, std::memory_order_seq_cst);
        }
    }

    bool is_requested() const noexcept {
        const bool local_requested = requested_ && requested_->load(std::memory_order_seq_cst);
        const bool external_requested = external_signal_ != nullptr && *external_signal_ != 0;
        return local_requested || external_requested;
    }

   private:
    friend ShutdownToken install_signal_shutdown_token();

    explicit ShutdownToken(const volatile std::sig_atomic_t *external_signal)
        : requested_(std::make_shared<std::atomic_bool>(false)),
          external_signal_(external_signal) {}

    std::shared_ptr<std::atomic_bool> requested_;
    const volatile std::sig_atomic_t *external_signal_ = nullptr;
};

namespace detail {

inline volatile std::sig_atomic_t signal_shutdown_requested = 0;
inline std::atomic_bool signal_handlers_installed{false};

inline void handle_shutdown_signal(int) noexcept { signal_shutdown_requested = 1; }

inline void install_signal_handlers_once() noexcept {
    bool expected = false;
    if (!signal_handlers_installed.compare_exchange_strong(
            expected, true, std::memory_order_seq_cst, std::memory_order_seq_cst)) {
        return;
    }
    (void)std::signal(SIGINT, handle_shutdown_signal);
    (void)std::signal(SIGTERM, handle_shutdown_signal);
}

}  // namespace detail

/**
 * @brief 安装 SIGINT/SIGTERM handler，并返回进程信号驱动的 shutdown token。
 */
inline ShutdownToken install_signal_shutdown_token() {
    detail::install_signal_handlers_once();
    return ShutdownToken{std::addressof(detail::signal_shutdown_requested)};
}

}  // namespace flowrt
