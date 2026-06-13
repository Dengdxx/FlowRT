#pragma once

#include <chrono>
#include <cstddef>
#include <cstdint>
#include <deque>
#include <flowrt/executor.hpp>
#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <utility>
#include <variant>
#include <vector>

namespace flowrt {

/**
 * @brief 有界 channel 写满时的处理策略。
 */
enum class OverflowPolicy : std::uint8_t {
    DropOldest = 0,  ///< 丢弃最旧样本，接收新样本。
    DropNewest = 1,  ///< 丢弃当前写入样本，保留已有队列。
    Error = 2,       ///< 返回溢出错误，由 runtime shell 或用户代码处理。
    Block = 3,       ///< 表达背压意图；实时路径不应默认使用无界阻塞。
};

/**
 * @brief channel 写入成功后的结果。
 */
enum class ChannelWriteOutcome : std::uint8_t {
    Accepted = 0,       ///< 样本已进入 channel。
    DroppedOldest = 1,  ///< 为接收新样本丢弃了最旧样本。
    DroppedNewest = 2,  ///< 当前样本被丢弃。
    Backpressured = 3,  ///< 写入方遇到背压，样本未进入 channel。
};

/**
 * @brief channel 严格写入失败时的错误。
 */
enum class ChannelError : std::uint8_t {
    Overflow = 0,     ///< 有界队列已满且策略要求报告错误。
    Transport = 1,    ///< backend transport 无法完成本次读写。
    Unsupported = 2,  ///< backend SDK 未编译进当前构建，配置错误不可恢复。
};

/**
 * @brief 有界 channel 写入结果。
 *
 * variant 左侧表示已按策略处理，右侧表示需要调用方显式处理的错误。
 */
using ChannelPushResult = std::variant<ChannelWriteOutcome, ChannelError>;

/**
 * @brief 输入样本过期时的处理策略。
 */
enum class StalePolicy : std::uint8_t {
    Warn = 0,      ///< 保留样本并暴露 stale 标记。
    Drop = 1,      ///< 过期后隐藏样本。
    HoldLast = 2,  ///< 保留最后一个样本并暴露 stale 标记。
    Error = 3,     ///< 由 generated shell 将过期输入提升为错误状态。
};

/**
 * @brief 带时间戳 channel 读取时的 freshness 配置。
 *
 * C++ runtime 使用 `std::chrono::milliseconds` 表达时间窗口，避免在公共 API 中传递没有单位的
 * 裸整数。generated shell 仍可把调度 tick 归一化为毫秒计数，再交给 channel 计算 stale 状态。
 */
class StaleConfig {
   public:
    /**
     * @brief freshness 时间窗口类型。
     */
    using Duration = std::chrono::milliseconds;

    /**
     * @brief 构造不检查过期时间的默认配置。
     */
    constexpr StaleConfig() noexcept = default;

    /**
     * @brief 构造不检查过期时间、但保留指定 stale policy 的配置。
     *
     * @param policy 样本过期时的处理策略。
     */
    constexpr explicit StaleConfig(StalePolicy policy) noexcept : policy_(policy) {}

    /**
     * @brief 构造带最大样本年龄的 freshness 配置。
     *
     * @param max_age 最大允许样本年龄。
     * @param policy 样本过期时的处理策略。
     */
    constexpr StaleConfig(Duration max_age, StalePolicy policy) noexcept
        : max_age_(max_age), policy_(policy) {}

    /**
     * @brief 返回不检查过期时间的默认配置。
     *
     * @return 默认 freshness 配置。
     */
    static constexpr StaleConfig none() noexcept { return StaleConfig{}; }

    /**
     * @brief 返回最大允许样本年龄。
     *
     * @return 配置了年龄窗口时返回该窗口，否则返回空值。
     */
    constexpr std::optional<Duration> max_age() const noexcept { return max_age_; }

    /**
     * @brief 返回样本过期时的处理策略。
     *
     * @return stale policy。
     */
    constexpr StalePolicy policy() const noexcept { return policy_; }

    /**
     * @brief 判断指定发布时间的样本在当前时间是否过期。
     *
     * @param published_at_ms 样本发布时间，单位为 runtime 毫秒。
     * @param now_ms 当前 runtime 时间，单位为毫秒。
     * @return 超过 `max_age` 时返回 true。
     */
    constexpr bool stale_at(std::optional<std::uint64_t> published_at_ms,
                            std::uint64_t now_ms) const noexcept {
        if (!max_age_ || !published_at_ms || now_ms <= *published_at_ms) {
            return false;
        }
        if (max_age_->count() < 0) {
            return true;
        }
        const auto max_age_ms = static_cast<std::uint64_t>(max_age_->count());
        return now_ms - *published_at_ms > max_age_ms;
    }

   private:
    std::optional<Duration> max_age_;
    StalePolicy policy_ = StalePolicy::Warn;
};

/**
 * @brief latest snapshot 输入视图。
 *
 * @tparam T 消息类型，必须由 Contract IR 和 Message ABI 约束保证布局稳定。
 *
 * `Latest<T>` 不拥有消息对象，只在一次用户回调期间借用 runtime shell 中的最新样本。
 * `present()` 表示当前是否有可读样本，`stale()` 表示样本是否超过 RSDL 声明的 freshness
 * 约束。用户代码不得保存内部指针到回调之外。
 */
template <typename T>
class Latest {
   public:
    /**
     * @brief 构造一个空输入视图。
     */
    constexpr Latest() noexcept = default;

    /**
     * @brief 从借用指针和 stale 标记构造输入视图。
     *
     * @param value 当前可读样本；为 `nullptr` 时表示没有样本。
     * @param stale 当前样本是否过期。
     */
    constexpr Latest(const T *value, bool stale = false) noexcept : value_(value), stale_(stale) {}

    /**
     * @brief 判断当前输入是否有样本。
     *
     * @return 有样本时返回 true。
     */
    constexpr bool present() const noexcept { return value_ != nullptr; }

    /**
     * @brief 判断当前样本是否已过期。
     *
     * @return 样本超过 freshness 约束时返回 true。
     */
    constexpr bool stale() const noexcept { return stale_; }

    /**
     * @brief 返回当前样本指针。
     *
     * @return 样本存在时返回非空指针，否则返回 `nullptr`。
     */
    constexpr const T *get() const noexcept { return value_; }

    /**
     * @brief 返回当前样本指针。
     *
     * @return 样本存在时返回非空指针，否则返回 `nullptr`。
     */
    constexpr const T *as_ref() const noexcept { return value_; }

   private:
    const T *value_ = nullptr;
    bool stale_ = false;
};

/**
 * @brief generated shell 在 scheduler 线程提交输出时使用的 typed commit record。
 *
 * @tparam T 输出消息类型。
 *
 * record 保存 FlowRT task、port、payload 和调度时间上下文。真正写入哪个 backend route
 * 由 generated shell 根据 `port` 或 route-specific closure 决定。
 */
template <typename T>
struct OutputCommitRecord {
    TaskId task;
    std::string task_name;
    std::string port;
    T payload;
    std::uint64_t published_at_ms{};
    std::uint64_t tick_time_ms{};

    /**
     * @brief 用 route-specific closure 消费 payload 并提交。
     */
    template <typename Fn>
    decltype(auto) commit_with(Fn &&fn) && {
        return std::invoke(std::forward<Fn>(fn), std::move(payload), published_at_ms, tick_time_ms);
    }
};

/**
 * @brief 组件输出端口的单样本写入句柄。
 *
 * @tparam T 输出消息类型。
 *
 * 用户回调通过 `write()` 设置本次输出。generated shell 在回调返回后取走该值，只有
 * task 返回 `Status::Ok` 时才由 scheduler 线程提交到对应 channel；如果用户没有写入，
 * 该端口本次 tick 不产生输出。
 */
template <typename T>
class Output {
   public:
    /**
     * @brief 构造空输出句柄。
     */
    Output() = default;

    /**
     * @brief 写入本次回调的输出样本。
     *
     * @param value 要发布的消息。若重复调用，后一次写入覆盖前一次值。
     */
    void write(T value) { value_ = std::move(value); }

    /**
     * @brief 判断本次回调是否已经写入输出。
     *
     * @return 已写入时返回 true。
     */
    bool present() const noexcept { return value_.has_value(); }

    /**
     * @brief 借用当前输出样本。
     *
     * @return 样本存在时返回非空指针，否则返回 `nullptr`。
     */
    const T *as_ref() const noexcept { return value_ ? std::addressof(*value_) : nullptr; }

    /**
     * @brief 可变借用当前输出样本。
     *
     * @return 样本存在时返回非空指针，否则返回 `nullptr`。
     */
    T *as_mut() noexcept { return value_ ? std::addressof(*value_) : nullptr; }

    /**
     * @brief 取走当前输出样本并清空句柄。
     *
     * @return 样本存在时返回该样本，否则返回空值。
     */
    std::optional<T> take() {
        std::optional<T> value = std::move(value_);
        value_.reset();
        return value;
    }

    /**
     * @brief 取走当前输出样本并附加 scheduler commit 所需上下文。
     *
     * @param task 产生该输出的 task id。
     * @param task_name 产生该输出的 task 名称。
     * @param port 输出端口名称。
     * @param published_at_ms scheduler 决定发布输出的 runtime 毫秒时间。
     * @param tick_time_ms 本次 task tick 对应的 runtime 毫秒时间。
     * @return 样本存在时返回 commit record，否则返回空值。
     */
    std::optional<OutputCommitRecord<T>> take_commit_record(TaskId task, std::string task_name,
                                                            std::string port,
                                                            std::uint64_t published_at_ms,
                                                            std::uint64_t tick_time_ms) {
        auto value = take();
        if (!value.has_value()) {
            return std::nullopt;
        }
        return OutputCommitRecord<T>{.task = task,
                                     .task_name = std::move(task_name),
                                     .port = std::move(port),
                                     .payload = std::move(*value),
                                     .published_at_ms = published_at_ms,
                                     .tick_time_ms = tick_time_ms};
    }

   private:
    std::optional<T> value_;
};

/**
 * @brief latest channel 的最小内存态实现。
 *
 * @tparam T channel 承载的消息类型。
 *
 * 该类型服务于 C++ inproc demo 和生成 shell 的语义验证。真实跨进程 backend 需要保持同样的
 * `Latest<T>` 用户视图语义，但可以使用不同存储和传输机制。
 */
template <typename T>
class LatestChannel {
   public:
    /**
     * @brief 构造空 latest channel。
     */
    LatestChannel() = default;

    /**
     * @brief 使用 freshness 配置构造空 latest channel。
     *
     * @param stale_config 读取时使用的 freshness 配置。
     */
    explicit LatestChannel(StaleConfig stale_config) noexcept : stale_config_(stale_config) {}

    /**
     * @brief 使用 freshness 配置构造空 latest channel。
     *
     * 该工厂函数与 Rust runtime 的 `with_stale_config` 保持命名一致，方便 codegen
     * 在跨语言 shell 中使用同一套语义表达。
     *
     * @param stale_config 读取时使用的 freshness 配置。
     * @return 配置后的空 latest channel。
     */
    static LatestChannel with_stale_config(StaleConfig stale_config) noexcept {
        return LatestChannel(stale_config);
    }

    /**
     * @brief 发布一个新样本并清除 stale 标记。
     *
     * @param value 新样本。
     */
    void publish(T value) {
        value_ = std::move(value);
        stale_ = false;
        published_at_ms_.reset();
        ++revision_;
    }

    /**
     * @brief 带 runtime 时间戳发布一个新样本。
     *
     * @param value 新样本。
     * @param now_ms 当前 runtime 时间，单位为毫秒。
     */
    void publish_at(T value, std::uint64_t now_ms) {
        value_ = std::move(value);
        stale_ = false;
        published_at_ms_ = now_ms;
        ++revision_;
    }

    /**
     * @brief 返回已进入 channel 的样本修订号。
     *
     * @return 每次成功发布后递增的计数。
     */
    std::uint64_t revision() const noexcept { return revision_; }

    /**
     * @brief 设置当前样本的 stale 标记。
     *
     * @param stale 为 true 时，后续 `view()` 会暴露过期状态。
     */
    void mark_stale(bool stale) noexcept { stale_ = stale; }

    /**
     * @brief 借用当前 latest snapshot。
     *
     * @return 只在 channel 当前状态有效期间可用的输入视图。
     */
    Latest<T> view() const noexcept {
        return Latest<T>{value_ ? std::addressof(*value_) : nullptr, stale_};
    }

    /**
     * @brief 以指定 runtime 时间读取 latest snapshot，并按 freshness 配置计算 stale 状态。
     *
     * @param now_ms 当前 runtime 时间，单位为毫秒。
     * @return 只在 channel 当前状态有效期间可用的输入视图。
     */
    Latest<T> view_at(std::uint64_t now_ms) const noexcept {
        const bool stale = stale_ || stale_config_.stale_at(published_at_ms_, now_ms);
        const bool drop_stale = stale && stale_config_.policy() == StalePolicy::Drop;
        return Latest<T>{value_ && !drop_stale ? std::addressof(*value_) : nullptr, stale};
    }

    /**
     * @brief 取走当前样本并清空 channel。
     *
     * @return 样本存在时返回该样本，否则返回空值。
     */
    std::optional<T> take() {
        std::optional<T> value = std::move(value_);
        value_.reset();
        return value;
    }

   private:
    std::optional<T> value_;
    bool stale_ = false;
    std::optional<std::uint64_t> published_at_ms_;
    StaleConfig stale_config_;
    std::uint64_t revision_ = 0;
};

/**
 * @brief FIFO channel 的单次读取结果。
 *
 * @tparam T channel 承载的消息类型。
 *
 * 该类型拥有从 FIFO 队列取出的样本，并在一次调度步骤内借出 `Latest<T>` 用户视图。
 */
template <typename T>
class FifoRead {
   public:
    /**
     * @brief 构造空读取结果。
     */
    FifoRead() = default;

    /**
     * @brief 从样本和 stale 标记构造读取结果。
     *
     * @param value 本次读取到的样本；为空表示没有样本或 stale drop 已隐藏样本。
     * @param stale 本次读取是否发现样本过期。
     */
    FifoRead(std::optional<T> value, bool stale) : value_(std::move(value)), stale_(stale) {}

    /**
     * @brief 借用本次读取结果，形成组件输入使用的 latest-style 视图。
     *
     * @return 只在本读取结果对象存活期间可用的输入视图。
     */
    Latest<T> view() const noexcept {
        return Latest<T>{value_ ? std::addressof(*value_) : nullptr, stale_};
    }

    /**
     * @brief 判断本次读取是否有可见样本。
     *
     * @return 有可见样本时返回 true。
     */
    bool present() const noexcept { return value_.has_value(); }

    /**
     * @brief 判断本次读取是否发现样本过期。
     *
     * @return 样本超过 freshness 约束时返回 true。
     */
    bool stale() const noexcept { return stale_; }

    /**
     * @brief 借用本次读取的样本。
     *
     * @return 样本存在时返回非空指针，否则返回 `nullptr`。
     */
    const T *as_ref() const noexcept { return value_ ? std::addressof(*value_) : nullptr; }

   private:
    std::optional<T> value_;
    bool stale_ = false;
};

/**
 * @brief 有界 FIFO channel 的最小内存态实现。
 *
 * @tparam T channel 承载的消息类型。
 *
 * `FifoChannel` 用于表达 RSDL 中 `fifo(depth = N)` 的基础行为。它不提供线程同步；
 * 多线程或跨进程 backend 应在自己的实现中保证并发安全，并保持相同的 overflow 语义。
 */
template <typename T>
class FifoChannel {
   public:
    /**
     * @brief 构造有界 FIFO channel。
     *
     * @param depth 队列深度；传入 0 时按 1 处理。
     * @param overflow 队列满时的处理策略。
     */
    explicit FifoChannel(std::size_t depth, OverflowPolicy overflow)
        : depth_(depth == 0 ? 1 : depth), overflow_(overflow) {}

    /**
     * @brief 使用 freshness 配置构造有界 FIFO channel。
     *
     * @param depth 队列深度；传入 0 时按 1 处理。
     * @param overflow 队列满时的处理策略。
     * @param stale_config 读取时使用的 freshness 配置。
     * @return 配置后的空 FIFO channel。
     */
    static FifoChannel with_stale_config(std::size_t depth, OverflowPolicy overflow,
                                         StaleConfig stale_config) noexcept {
        FifoChannel channel(depth, overflow);
        channel.stale_config_ = stale_config;
        return channel;
    }

    /**
     * @brief 写入一个样本。
     *
     * @param value 要写入的消息。
     * @return 成功处理结果或严格错误。
     */
    ChannelPushResult push(T value) { return push_entry(Entry{std::move(value), std::nullopt}); }

    /**
     * @brief 带 runtime 时间戳写入一个样本。
     *
     * @param value 要写入的消息。
     * @param now_ms 当前 runtime 时间，单位为毫秒。
     * @return 成功处理结果或严格错误。
     */
    ChannelPushResult push_at(T value, std::uint64_t now_ms) {
        return push_entry(Entry{std::move(value), now_ms});
    }

    /**
     * @brief 弹出最旧样本。
     *
     * @return 队列非空时返回样本，否则返回空值。
     */
    std::optional<T> pop() {
        if (queue_.empty()) {
            return std::nullopt;
        }
        Entry entry = std::move(queue_.front());
        queue_.pop_front();
        return std::move(entry.value);
    }

    /**
     * @brief 以指定 runtime 时间弹出最旧样本，并按 freshness 配置计算 stale 状态。
     *
     * @param now_ms 当前 runtime 时间，单位为毫秒。
     * @return 拥有样本的读取结果。
     */
    FifoRead<T> pop_at(std::uint64_t now_ms) {
        if (queue_.empty()) {
            return FifoRead<T>{};
        }
        Entry entry = std::move(queue_.front());
        queue_.pop_front();
        const bool stale = stale_config_.stale_at(entry.published_at_ms, now_ms);
        if (stale && stale_config_.policy() == StalePolicy::Drop) {
            return FifoRead<T>{std::nullopt, true};
        }
        return FifoRead<T>{std::move(entry.value), stale};
    }

    /**
     * @brief 返回当前队列长度。
     *
     * @return 当前样本数量。
     */
    std::size_t len() const noexcept { return queue_.size(); }

    /**
     * @brief 判断队列是否为空。
     *
     * @return 队列为空时返回 true。
     */
    bool empty() const noexcept { return queue_.empty(); }

    /**
     * @brief 返回归一化后的队列深度。
     *
     * @return 至少为 1 的队列深度。
     */
    std::size_t depth() const noexcept { return depth_; }

    /**
     * @brief 返回已进入 channel 的样本修订号。
     *
     * @return 每次样本进入队列后递增的计数。
     */
    std::uint64_t revision() const noexcept { return revision_; }

   private:
    struct Entry {
        T value;
        std::optional<std::uint64_t> published_at_ms;
    };

    ChannelPushResult push_entry(Entry entry) {
        if (queue_.size() < depth_) {
            queue_.push_back(std::move(entry));
            ++revision_;
            return ChannelWriteOutcome::Accepted;
        }

        switch (overflow_) {
            case OverflowPolicy::DropOldest:
                queue_.pop_front();
                queue_.push_back(std::move(entry));
                ++revision_;
                return ChannelWriteOutcome::DroppedOldest;
            case OverflowPolicy::DropNewest:
                return ChannelWriteOutcome::DroppedNewest;
            case OverflowPolicy::Error:
                return ChannelError::Overflow;
            case OverflowPolicy::Block:
                return ChannelWriteOutcome::Backpressured;
        }

        return ChannelWriteOutcome::Backpressured;
    }

    std::deque<Entry> queue_;
    std::size_t depth_ = 1;
    OverflowPolicy overflow_ = OverflowPolicy::DropOldest;
    StaleConfig stale_config_;
    std::uint64_t revision_ = 0;
};

/**
 * @brief island boundary input 的一次读取结果。
 *
 * @tparam T 输入消息类型。
 *
 * 读取结果持有一个 shared_ptr snapshot，使 generated shell 在释放 boundary input 锁后仍能
 * 构造 `Latest<T>` 视图。该类型只用于显式 `boundary.input`，不参与普通 channel 热路径。
 */
template <typename T>
class BoundaryInputRead {
   public:
    /**
     * @brief 构造空读取结果。
     */
    BoundaryInputRead() = default;

    /**
     * @brief 从 snapshot、stale 标记和修订号构造读取结果。
     *
     * @param value 当前 snapshot；为空表示没有样本或 stale drop 已隐藏样本。
     * @param stale 本次读取是否发现样本过期。
     * @param revision boundary input 注入修订号。
     */
    BoundaryInputRead(std::shared_ptr<const T> value, bool stale, std::uint64_t revision)
        : value_(std::move(value)), stale_(stale), revision_(revision) {}

    /**
     * @brief 借用本次读取结果，形成组件输入使用的 latest-style 视图。
     *
     * @return 只在本读取结果对象存活期间可用的输入视图。
     */
    Latest<T> view() const noexcept { return Latest<T>{value_.get(), stale_}; }

    /**
     * @brief 返回本次读取对应的注入修订号。
     *
     * @return 每次注入后递增的计数。
     */
    std::uint64_t revision() const noexcept { return revision_; }

   private:
    std::shared_ptr<const T> value_;
    bool stale_ = false;
    std::uint64_t revision_ = 0;
};

/**
 * @brief island boundary input 的显式注入端。
 *
 * @tparam T 输入消息类型。
 *
 * 测试工具、ROS2 adapter 或其他边界驱动通过 `inject*` 写入 latest snapshot，generated shell
 * 通过 `read*` 读取。普通 dataflow channel 不依赖该类型，因此未启用 boundary 时不会增加
 * 常驻观测或额外分支。
 */
template <typename T>
class BoundaryInput {
   public:
    /**
     * @brief 构造空 boundary input。
     */
    BoundaryInput() = default;

    /**
     * @brief 使用 freshness 配置构造空 boundary input。
     *
     * @param stale_config 读取时使用的 freshness 配置。
     */
    explicit BoundaryInput(StaleConfig stale_config) : stale_config_(stale_config) {}

    /**
     * @brief 使用 freshness 配置构造空 boundary input。
     *
     * @param stale_config 读取时使用的 freshness 配置。
     * @return 配置后的 boundary input。
     */
    static BoundaryInput with_stale_config(StaleConfig stale_config) {
        return BoundaryInput(stale_config);
    }

    /**
     * @brief 绑定 scheduler waiter；后续注入会唤醒数据触发任务。
     *
     * @param waiter scheduler waiter 副本。
     */
    void set_schedule_waiter(ScheduleWaiter waiter) {
        std::lock_guard lock(mutex_);
        schedule_waiter_ = std::move(waiter);
    }

    /**
     * @brief 注入一个无 runtime 时间戳的样本。
     *
     * @param value 新样本。
     * @return 注入后的修订号。
     */
    std::uint64_t inject(T value) { return inject_entry(std::move(value), std::nullopt); }

    /**
     * @brief 注入一个带 runtime 毫秒时间戳的样本。
     *
     * @param value 新样本。
     * @param now_ms 当前 runtime 时间，单位为毫秒。
     * @return 注入后的修订号。
     */
    std::uint64_t inject_at(T value, std::uint64_t now_ms) {
        return inject_entry(std::move(value), now_ms);
    }

    /**
     * @brief 读取当前 latest snapshot；不重新计算 freshness。
     *
     * @return 本次读取结果。
     */
    BoundaryInputRead<T> read() const {
        std::lock_guard lock(mutex_);
        return BoundaryInputRead<T>{value_, false, revision_};
    }

    /**
     * @brief 按 runtime 毫秒时间读取 latest snapshot，并应用 freshness 配置。
     *
     * @param now_ms 当前 runtime 时间，单位为毫秒。
     * @return 本次读取结果。
     */
    BoundaryInputRead<T> read_at(std::uint64_t now_ms) const {
        std::lock_guard lock(mutex_);
        const bool stale = stale_config_.stale_at(published_at_ms_, now_ms);
        const bool drop_stale = stale && stale_config_.policy() == StalePolicy::Drop;
        return BoundaryInputRead<T>{drop_stale ? nullptr : value_, stale, revision_};
    }

    /**
     * @brief 返回已注入样本修订号。
     *
     * @return 每次注入后递增的计数。
     */
    std::uint64_t revision() const noexcept {
        std::lock_guard lock(mutex_);
        return revision_;
    }

   private:
    std::uint64_t inject_entry(T value, std::optional<std::uint64_t> published_at_ms) {
        std::optional<ScheduleWaiter> waiter;
        std::uint64_t revision{};
        {
            std::lock_guard lock(mutex_);
            value_ = std::make_shared<T>(std::move(value));
            published_at_ms_ = published_at_ms;
            ++revision_;
            revision = revision_;
            waiter = schedule_waiter_;
        }
        if (waiter.has_value()) {
            waiter->notify_data();
        }
        return revision;
    }

    mutable std::mutex mutex_;
    std::shared_ptr<const T> value_;
    std::optional<std::uint64_t> published_at_ms_;
    StaleConfig stale_config_;
    std::uint64_t revision_ = 0;
    std::optional<ScheduleWaiter> schedule_waiter_;
};

/**
 * @brief island boundary output 的显式观测 sink。
 *
 * @tparam T 输出消息类型。
 *
 * generated shell 只会在声明了 `boundary.output` 的端口发布到该类型。sink 由工具或 adapter
 * 显式注册，guard 析构后自动移除，避免临时观测永久改变运行时行为。
 */
template <typename T>
class BoundaryOutput {
    struct Inner;

   public:
    class SinkGuard;
    using Sink = std::function<void(const T &, std::optional<std::uint64_t>)>;

    /**
     * @brief 构造没有 sink 的 boundary output。
     */
    BoundaryOutput() : inner_(std::make_shared<Inner>()) {}

    /**
     * @brief 注册一个 sink，并返回自动注销 guard。
     *
     * @param sink sink 回调。回调在发布线程同步执行。
     * @return sink 生命周期 guard。
     */
    SinkGuard register_sink(Sink sink) {
        std::lock_guard lock(inner_->mutex);
        const auto id = inner_->next_sink_id++;
        inner_->sinks.emplace(id, std::move(sink));
        return SinkGuard{inner_, id};
    }

    /**
     * @brief 发布一个无 runtime 时间戳的样本到当前已注册 sink。
     *
     * @param value 输出样本。
     */
    void publish(const T &value) const { publish_entry(value, std::nullopt); }

    /**
     * @brief 发布一个带 runtime 毫秒时间戳的样本到当前已注册 sink。
     *
     * @param value 输出样本。
     * @param now_ms 当前 runtime 时间，单位为毫秒。
     */
    void publish_at(const T &value, std::uint64_t now_ms) const { publish_entry(value, now_ms); }

    /**
     * @brief 返回当前注册 sink 数量。
     *
     * @return sink 数量。
     */
    std::size_t sink_count() const noexcept {
        std::lock_guard lock(inner_->mutex);
        return inner_->sinks.size();
    }

    /**
     * @brief boundary output sink 注册生命周期 guard。
     */
    class SinkGuard {
       public:
        SinkGuard() = default;
        SinkGuard(const SinkGuard &) = delete;
        SinkGuard &operator=(const SinkGuard &) = delete;

        SinkGuard(SinkGuard &&other) noexcept : inner_(std::move(other.inner_)), id_(other.id_) {
            other.id_.reset();
        }

        SinkGuard &operator=(SinkGuard &&other) noexcept {
            if (this != &other) {
                reset();
                inner_ = std::move(other.inner_);
                id_ = other.id_;
                other.id_.reset();
            }
            return *this;
        }

        ~SinkGuard() { reset(); }

        /**
         * @brief 主动注销 sink。
         */
        void reset() noexcept {
            if (!id_.has_value()) {
                return;
            }
            if (const auto inner = inner_.lock()) {
                std::lock_guard lock(inner->mutex);
                inner->sinks.erase(*id_);
            }
            id_.reset();
        }

       private:
        friend class BoundaryOutput<T>;

        SinkGuard(std::weak_ptr<Inner> inner, std::uint64_t id)
            : inner_(std::move(inner)), id_(id) {}

        std::weak_ptr<Inner> inner_;
        std::optional<std::uint64_t> id_;
    };

   private:
    struct Inner {
        mutable std::mutex mutex;
        std::map<std::uint64_t, Sink> sinks;
        std::uint64_t next_sink_id = 0;
    };

    void publish_entry(const T &value, std::optional<std::uint64_t> published_at_ms) const {
        std::vector<Sink> sinks;
        {
            std::lock_guard lock(inner_->mutex);
            sinks.reserve(inner_->sinks.size());
            for (const auto &[_, sink] : inner_->sinks) {
                sinks.push_back(sink);
            }
        }
        for (const auto &sink : sinks) {
            sink(value, published_at_ms);
        }
    }

    std::shared_ptr<Inner> inner_;
};

}  // namespace flowrt
