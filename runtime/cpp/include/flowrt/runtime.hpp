#pragma once

#include <algorithm>
#include <array>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <deque>
#include <functional>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>
#include <variant>

#ifdef FLOWRT_HAS_ICEORYX2_CXX
#include <iox2/iceoryx2.hpp>
#endif

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
 * @brief runtime 传递给生命周期钩子和调度步骤的上下文。
 *
 * v0.1 暂不暴露资源句柄；保留该类型是为了后续承载 clock、logger、参数快照和 backend
 * 能力视图，同时保持用户接口稳定。
 */
struct Context {};

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
    Overflow = 0,   ///< 有界队列已满且策略要求报告错误。
    Transport = 1,  ///< backend transport 无法完成本次读写。
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
 * @brief 组件输出端口的单样本写入句柄。
 *
 * @tparam T 输出消息类型。
 *
 * 用户回调通过 `write()` 设置本次输出。runtime shell 在回调返回后取走该值并发布到对应
 * channel；如果用户没有写入，则该端口本次 tick 不产生输出。
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
    }

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

   private:
    struct Entry {
        T value;
        std::optional<std::uint64_t> published_at_ms;
    };

    ChannelPushResult push_entry(Entry entry) {
        if (queue_.size() < depth_) {
            queue_.push_back(std::move(entry));
            return ChannelWriteOutcome::Accepted;
        }

        switch (overflow_) {
            case OverflowPolicy::DropOldest:
                queue_.pop_front();
                queue_.push_back(std::move(entry));
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
};

namespace iox2 {

/**
 * @brief FlowRT iox2 transport user header。
 *
 * 该 header 保存 transport 层 runtime timestamp，使 iceoryx2 payload 仍保持业务消息类型 `T`。这样
 * C++ `IOX2_TYPE_NAME` 与 Rust `#[type_name(...)]` 都作用在同一个 payload 类型上。
 */
struct FlowrtIox2Header {
    static constexpr const char *IOX2_TYPE_NAME = "FlowRTIox2Header";

    std::uint64_t published_at_ms{};
};

/**
 * @brief 打开 iceoryx2 publish-subscribe endpoint 时使用的 C++ QoS 配置。
 *
 * 该类型承载 Contract IR channel policy 归一化后的 depth、overflow 和 freshness intent。它不暴露
 * iceoryx2 底层 publisher/subscriber API；生成 shell 用它把 FlowRT 语义传给后续真实 transport
 * binding。
 */
class Iox2ChannelConfig {
   public:
    /**
     * @brief 构造 latest channel 的默认 QoS 配置。
     *
     * @return depth 为 1、overflow 为 DropOldest 的配置。
     */
    static constexpr Iox2ChannelConfig latest() noexcept { return Iox2ChannelConfig{}; }

    /**
     * @brief 构造 FIFO channel 的 QoS 配置。
     *
     * @param depth 队列深度；传入 0 时按 1 处理。
     * @param overflow 队列满时的 FlowRT 语义。
     * @return 归一化后的配置。
     */
    static constexpr Iox2ChannelConfig fifo(std::size_t depth, OverflowPolicy overflow) noexcept {
        return Iox2ChannelConfig(depth == 0 ? 1 : depth, overflow, StaleConfig{});
    }

    /**
     * @brief 设置 freshness 配置。
     *
     * @param stale stale-data policy 和时间窗口。
     * @return 更新后的配置副本。
     */
    constexpr Iox2ChannelConfig with_stale_config(StaleConfig stale) const noexcept {
        return Iox2ChannelConfig(depth_, overflow_, stale);
    }

    /**
     * @brief 返回归一化后的 channel depth。
     */
    constexpr std::size_t depth() const noexcept { return depth_; }

    /**
     * @brief 返回 overflow policy。
     */
    constexpr OverflowPolicy overflow() const noexcept { return overflow_; }

    /**
     * @brief 返回 stale-data 配置。
     */
    constexpr StaleConfig stale() const noexcept { return stale_; }

   private:
    constexpr Iox2ChannelConfig() noexcept = default;

    constexpr Iox2ChannelConfig(std::size_t depth, OverflowPolicy overflow,
                                StaleConfig stale) noexcept
        : depth_(depth), overflow_(overflow), stale_(stale) {}

    std::size_t depth_ = 1;
    OverflowPolicy overflow_ = OverflowPolicy::DropOldest;
    StaleConfig stale_;
};

/**
 * @brief typed iceoryx2 publish-subscribe endpoint 的 C++ API 边界。
 *
 * @tparam T FlowRT Message ABI v0.1 plain-data payload 类型。
 *
 * 开启 `FLOWRT_HAS_ICEORYX2_CXX` 时，该类绑定真实 `iceoryx2-cxx` typed pub/sub endpoint；
 * 默认构建不依赖 iceoryx2，并保持安全失败语义。业务组件接口不应暴露该类型。
 */
template <typename T>
class Iox2PubSub {
   public:
    Iox2PubSub(Iox2PubSub &&) noexcept = default;
    Iox2PubSub(const Iox2PubSub &) = delete;
    auto operator=(Iox2PubSub &&) noexcept -> Iox2PubSub & = default;
    auto operator=(const Iox2PubSub &) -> Iox2PubSub & = delete;
    ~Iox2PubSub() = default;

    /**
     * @brief 打开或创建一个 FlowRT iox2 service endpoint。
     *
     * @param service_name canonical iox2 service name。
     * @param config 从 Contract IR channel policy 生成的 QoS 配置。
     * @return endpoint 对象；底层资源打开失败或未开启 iox2 支持时 `ready()` 返回 false。
     */
    static Iox2PubSub open_with_config(std::string_view service_name, Iox2ChannelConfig config) {
        return Iox2PubSub(service_name, config);
    }

    /**
     * @brief 返回 canonical service name。
     */
    std::string_view service_name() const noexcept { return service_name_; }

    /**
     * @brief 返回 channel QoS 配置。
     */
    constexpr Iox2ChannelConfig config() const noexcept { return config_; }

    /**
     * @brief 判断 transport endpoint 是否已经绑定到底层 iceoryx2 资源。
     */
    bool ready() const noexcept {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        return publisher_.has_value() && subscriber_.has_value();
#else
        return false;
#endif
    }

    /**
     * @brief 带 FlowRT runtime 时间戳发布一个值。
     *
     * @return 写入成功时返回 `Accepted`；transport 无法完成时返回 `ChannelError::Transport`。
     */
    ChannelPushResult publish_at(T value, std::uint64_t published_at_ms) noexcept {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        if (!publisher_) {
            return ChannelError::Transport;
        }

        auto sample = publisher_->loan_uninit();
        if (!sample.has_value()) {
            return ChannelError::Transport;
        }

        auto loaned_sample = std::move(sample).value();
        loaned_sample.user_header_mut().published_at_ms = published_at_ms;
        auto initialized_sample = loaned_sample.write_payload(std::move(value));
        auto sent = ::iox2::send(std::move(initialized_sample));
        if (!sent.has_value()) {
            return ChannelError::Transport;
        }

        return ChannelWriteOutcome::Accepted;
#else
        (void)value;
        (void)published_at_ms;
        return ChannelError::Transport;
#endif
    }

    /**
     * @brief 读取 latest snapshot，并保留 transport 错误通道。
     *
     * @return 读取成功时返回 `Latest<T>`；transport 无法完成时返回 `ChannelError::Transport`。
     */
    std::variant<Latest<T>, ChannelError> receive_latest_at(std::uint64_t now_ms) noexcept {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        if (!subscriber_) {
            return ChannelError::Transport;
        }

        while (true) {
            auto received = subscriber_->receive();
            if (!received.has_value()) {
                return ChannelError::Transport;
            }

            auto sample = std::move(received).value();
            if (!sample.has_value()) {
                break;
            }

            received_ = sample->payload();
            published_at_ms_ = sample->user_header().published_at_ms;
        }

        const bool stale = config_.stale().stale_at(published_at_ms_, now_ms);
        const bool drop_stale = stale && config_.stale().policy() == StalePolicy::Drop;
        return Latest<T>{received_ && !drop_stale ? std::addressof(*received_) : nullptr, stale};
#else
        (void)now_ms;
        return ChannelError::Transport;
#endif
    }

   private:
    Iox2PubSub(std::string_view service_name, Iox2ChannelConfig config)
        : service_name_(service_name), config_(config) {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        static_assert(std::is_trivially_copyable_v<T>,
                      "FlowRT iox2 C++ payload must be trivially copyable");
        open_iox2_endpoint();
#endif
    }

#ifdef FLOWRT_HAS_ICEORYX2_CXX
    using Iox2Node = ::iox2::Node<::iox2::ServiceType::Ipc>;
    using Iox2Service =
        ::iox2::PortFactoryPublishSubscribe<::iox2::ServiceType::Ipc, T, FlowrtIox2Header>;
    using Iox2Publisher = ::iox2::Publisher<::iox2::ServiceType::Ipc, T, FlowrtIox2Header>;
    using Iox2Subscriber = ::iox2::Subscriber<::iox2::ServiceType::Ipc, T, FlowrtIox2Header>;

    static constexpr bool safe_overflow(Iox2ChannelConfig config) noexcept {
        return config.overflow() != OverflowPolicy::Block;
    }

    static constexpr ::iox2::BackpressureStrategy backpressure_strategy(
        Iox2ChannelConfig config) noexcept {
        return config.overflow() == OverflowPolicy::Block
                   ? ::iox2::BackpressureStrategy::RetryUntilDelivered
                   : ::iox2::BackpressureStrategy::DiscardData;
    }

    void open_iox2_endpoint() {
        auto name = ::iox2::ServiceName::create(service_name_.c_str());
        if (!name.has_value()) {
            return;
        }

        auto node = ::iox2::NodeBuilder().create<::iox2::ServiceType::Ipc>();
        if (!node.has_value()) {
            return;
        }
        node_.emplace(std::move(node).value());

        const auto depth = static_cast<std::uint64_t>(config_.depth());
        auto service = node_->service_builder(std::move(name).value())
                           .publish_subscribe<T>()
                           .template user_header<FlowrtIox2Header>()
                           .enable_safe_overflow(safe_overflow(config_))
                           .history_size(depth)
                           .subscriber_max_buffer_size(depth)
                           .open_or_create();
        if (!service.has_value()) {
            node_.reset();
            return;
        }
        service_.emplace(std::move(service).value());

        auto subscriber = service_->subscriber_builder().buffer_size(depth).create();
        if (!subscriber.has_value()) {
            service_.reset();
            node_.reset();
            return;
        }
        subscriber_.emplace(std::move(subscriber).value());

        auto publisher = service_->publisher_builder()
                             .backpressure_strategy(backpressure_strategy(config_))
                             .max_loaned_samples(depth)
                             .create();
        if (!publisher.has_value()) {
            subscriber_.reset();
            service_.reset();
            node_.reset();
            return;
        }
        publisher_.emplace(std::move(publisher).value());
    }
#endif

    std::string service_name_;
    Iox2ChannelConfig config_;
#ifdef FLOWRT_HAS_ICEORYX2_CXX
    std::optional<Iox2Node> node_;
    std::optional<Iox2Service> service_;
    std::optional<Iox2Publisher> publisher_;
    std::optional<Iox2Subscriber> subscriber_;
    std::optional<T> received_;
    std::optional<std::uint64_t> published_at_ms_;
#endif
};

}  // namespace iox2

/**
 * @brief backend capability 的只读视图。
 *
 * capability 字符串来自 Contract IR/backend contract，例如 `channel:latest` 或
 * `topology:multi_process`。validator 使用同一套 capability 语义判断部署是否可满足。
 */
class BackendCapabilities {
   public:
    /**
     * @brief 从静态 capability 列表构造视图。
     *
     * @param items capability 字符串切片；调用方必须保证其生命周期覆盖本对象。
     */
    constexpr explicit BackendCapabilities(std::span<const std::string_view> items) noexcept
        : items_(items) {}

    /**
     * @brief 查询 backend 是否声明某项能力。
     *
     * @param capability capability 字符串。
     * @return 存在时返回 true。
     */
    bool contains(std::string_view capability) const noexcept {
        return std::find(items_.begin(), items_.end(), capability) != items_.end();
    }

    /**
     * @brief 返回完整 capability 列表。
     *
     * @return capability 字符串切片。
     */
    std::span<const std::string_view> items() const noexcept { return items_; }

   private:
    std::span<const std::string_view> items_;
};

/**
 * @brief runtime 当前认识的 backend 类型。
 */
enum class BackendKind : std::uint8_t {
    Inproc = 0,  ///< 单进程内存 backend，主要用于测试、CI 和最小 demo。
    Iox2 = 1,    ///< iceoryx2 backend，用于本机多进程高性能 dataflow。
};

/**
 * @brief 调度器抽象边界。
 *
 * 调度器负责驱动 generated runtime shell 的 tick，不负责用户算法逻辑。v0.1 使用同步 tick
 * 接口表达最小语义，后续可以在不改变组件接口的前提下替换为更完整的实时调度实现。
 */
class Scheduler {
   public:
    /**
     * @brief 单次调度步骤函数。
     *
     * 第一个参数是 tick 序号，第二个参数是本轮共享 runtime context。
     */
    using StepFn = std::function<Status(std::size_t, Context &)>;

    virtual ~Scheduler() = default;

    /**
     * @brief 连续运行固定数量的 tick。
     *
     * @param ticks 要运行的 tick 数量。
     * @param step 每个 tick 调用一次的步骤函数。
     * @return 全部 tick 成功时返回 `Status::Ok`；否则返回第一个非 OK 状态。
     */
    virtual Status run_ticks(std::size_t ticks, StepFn step) const = 0;
};

/**
 * @brief runtime backend 抽象边界。
 *
 * Backend 暴露能力集合和调度器，用于 generated shell 在不依赖具体通信库 API 的情况下绑定运行时。
 */
class Backend {
   public:
    virtual ~Backend() = default;

    /**
     * @brief 返回 backend 类型。
     */
    virtual BackendKind kind() const noexcept = 0;

    /**
     * @brief 返回 backend capability 视图。
     */
    virtual BackendCapabilities capabilities() const noexcept = 0;

    /**
     * @brief 返回 backend 提供的调度器。
     */
    virtual const Scheduler &scheduler() const noexcept = 0;
};

/**
 * @brief 单进程同步调度器。
 *
 * 该调度器按 tick 顺序直接调用步骤函数。它用于 v0.1 的 inproc demo 和测试，不承诺实时线程、
 * 优先级继承或跨进程同步。
 */
class InprocScheduler final : public Scheduler {
   public:
    /**
     * @copydoc Scheduler::run_ticks
     */
    Status run_ticks(std::size_t ticks, StepFn step) const override {
        Context context;
        for (std::size_t tick = 0; tick < ticks; ++tick) {
            const auto status = step(tick, context);
            if (status != Status::Ok) {
                return status;
            }
        }
        return Status::Ok;
    }
};

/**
 * @brief 单进程 backend 实现。
 *
 * InprocBackend 使用进程内 channel 和同步调度器，适合测试、CI 和最小端到端 demo。
 */
class InprocBackend final : public Backend {
   public:
    /**
     * @copydoc Backend::kind
     */
    BackendKind kind() const noexcept override { return BackendKind::Inproc; }

    /**
     * @copydoc Backend::capabilities
     */
    BackendCapabilities capabilities() const noexcept override {
        return BackendCapabilities{std::span<const std::string_view>(kCapabilities)};
    }

    /**
     * @copydoc Backend::scheduler
     */
    const Scheduler &scheduler() const noexcept override { return scheduler_; }

   private:
    static inline constexpr std::array<std::string_view, 22> kCapabilities = {
        "abi:fixed_size_plain_data",
        "layout:native_layout",
        "allocation:bounded",
        "graph:static_graph",
        "trigger:periodic",
        "trigger:on_message",
        "trigger:startup",
        "trigger:shutdown",
        "timing:deadline_aware",
        "channel:latest",
        "channel:fifo",
        "overflow:drop_oldest",
        "overflow:drop_newest",
        "overflow:error",
        "overflow:block",
        "stale:warn",
        "stale:drop",
        "stale:hold_last",
        "stale:error",
        "topology:single_process",
        "transfer:copy",
        "observability:health",
    };

    InprocScheduler scheduler_;
};

/**
 * @brief iceoryx2 backend 的 C++ capability 骨架。
 *
 * 该 backend 报告 iox2 capability，并继续复用同步调度器驱动 generated shell。具体 channel
 * transport 由 `flowrt::iox2::Iox2PubSub<T>` 在 shell 内部绑定；业务组件仍只应依赖 FlowRT
 * runtime API，不直接依赖 iox2 publisher/subscriber。
 */
class Iox2Backend final : public Backend {
   public:
    /**
     * @copydoc Backend::kind
     */
    BackendKind kind() const noexcept override { return BackendKind::Iox2; }

    /**
     * @copydoc Backend::capabilities
     */
    BackendCapabilities capabilities() const noexcept override {
        return BackendCapabilities{std::span<const std::string_view>(kCapabilities)};
    }

    /**
     * @copydoc Backend::scheduler
     */
    const Scheduler &scheduler() const noexcept override { return scheduler_; }

   private:
    static inline constexpr std::array<std::string_view, 24> kCapabilities = {
        "abi:fixed_size_plain_data",
        "layout:native_layout",
        "allocation:bounded",
        "graph:static_graph",
        "trigger:periodic",
        "trigger:on_message",
        "trigger:startup",
        "trigger:shutdown",
        "timing:deadline_aware",
        "channel:latest",
        "channel:fifo",
        "overflow:drop_oldest",
        "overflow:drop_newest",
        "overflow:error",
        "overflow:block",
        "stale:warn",
        "stale:drop",
        "stale:hold_last",
        "stale:error",
        "topology:multi_process",
        "topology:single_host",
        "transfer:zero_copy",
        "transfer:loaned",
        "observability:health",
    };

    InprocScheduler scheduler_;
};

/**
 * @brief 构造默认 inproc backend。
 *
 * @return 可直接传给 generated shell 的单进程 backend。
 */
inline InprocBackend inproc_backend() { return InprocBackend{}; }

/**
 * @brief 构造 iox2 backend capability 骨架。
 *
 * @return 可用于 capability 选择和后续 iox2 shell 绑定的 backend 对象。
 */
inline Iox2Backend iox2_backend() { return Iox2Backend{}; }

}  // namespace flowrt
