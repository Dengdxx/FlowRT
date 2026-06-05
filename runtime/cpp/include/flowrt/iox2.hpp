#pragma once

#include <cstdint>
#include <flowrt/backend_health.hpp>
#include <flowrt/channels.hpp>
#include <flowrt/wire.hpp>
#include <optional>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>
#include <variant>

#ifdef FLOWRT_HAS_ICEORYX2_CXX
#include <iox2/iceoryx2.hpp>
#endif

namespace flowrt {

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
     * @brief 返回 endpoint 健康快照。
     */
    BackendHealthSnapshot health() const {
        if (!ready() && health_.snapshot().state == BackendHealthState::Ready) {
            return BackendHealthSnapshot{
                .state = BackendHealthState::Degraded,
                .last_error = std::optional<std::string>{"iceoryx2 endpoint is not ready"},
                .attempt = 0,
                .next_retry_unix_ms = std::nullopt,
                .recoverable = true,
            };
        }
        return health_.snapshot();
    }

    /**
     * @brief 返回 endpoint 重连策略。
     */
    ReconnectPolicy reconnect_policy() const noexcept { return health_.policy(); }

#ifdef FLOWRT_ENABLE_TEST_HOOKS
    /**
     * @brief 测试钩子：模拟本地 iox2 endpoint 资源丢失。
     */
    void reset_transport_for_test() noexcept {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        reset_iox2_endpoint();
#endif
        health_.mark_degraded("iceoryx2 endpoint reset by test");
    }
#endif

    /**
     * @brief 带 FlowRT runtime 时间戳发布一个值。
     *
     * @return 写入成功时返回 `Accepted`；transport 无法完成时返回 `ChannelError::Transport`。
     */
    ChannelPushResult publish_at(T value, std::uint64_t published_at_ms) noexcept {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        if (!ensure_ready()) {
            return ChannelError::Transport;
        }

        if (publish_payload(std::move(value), published_at_ms)) {
            health_.mark_ready();
            return ChannelWriteOutcome::Accepted;
        }
        if (!recover_after_transport_error("publish iceoryx2 sample")) {
            return ChannelError::Transport;
        }
        if (!publish_payload(std::move(value), published_at_ms)) {
            return ChannelError::Transport;
        }
        health_.mark_ready();
        return ChannelWriteOutcome::Accepted;
#else
        (void)value;
        (void)published_at_ms;
        health_.mark_failed("iceoryx2-cxx support is disabled", health_.snapshot().attempt);
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
        if (!ensure_ready()) {
            return ChannelError::Transport;
        }

        bool retried = false;
        while (true) {
            auto received = subscriber_->receive();
            if (!received.has_value()) {
                mark_transport_error("receive iceoryx2 sample");
                if (retried || !recover_after_transport_error("receive iceoryx2 sample")) {
                    return ChannelError::Transport;
                }
                retried = true;
                continue;
            }

            auto sample = std::move(received).value();
            if (!sample.has_value()) {
                health_.mark_ready();
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
        health_.mark_failed("iceoryx2-cxx support is disabled", health_.snapshot().attempt);
        return ChannelError::Transport;
#endif
    }

   private:
    Iox2PubSub(std::string_view service_name, Iox2ChannelConfig config)
        : service_name_(service_name), config_(config), health_(ReconnectPolicy{}) {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        static_assert(std::is_trivially_copyable_v<T>,
                      "FlowRT iox2 C++ payload must be trivially copyable");
        if (open_iox2_endpoint()) {
            health_.mark_ready();
        } else {
            health_.mark_degraded("failed to open iceoryx2 endpoint");
        }
#else
        health_.mark_degraded("iceoryx2-cxx support is disabled");
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

    bool ensure_ready() noexcept {
        if (ready()) {
            return true;
        }
        if (health_.snapshot().state != BackendHealthState::Reconnecting) {
            mark_transport_error("iceoryx2 endpoint is not ready");
        }
        return recover_after_transport_error("reopen iceoryx2 endpoint");
    }

    void mark_transport_error(std::string error) { health_.mark_degraded(std::move(error)); }

    bool recover_after_transport_error(std::string error) noexcept {
        const auto snapshot = health_.snapshot();
        if (snapshot.state == BackendHealthState::Reconnecting && snapshot.next_retry_unix_ms &&
            unix_now_ms() < *snapshot.next_retry_unix_ms) {
            return false;
        }

        const auto attempt = snapshot.attempt;
        if (!health_.policy().can_retry(attempt)) {
            health_.mark_failed("iceoryx2 endpoint reconnect budget exhausted", attempt);
            return false;
        }

        const auto now_ms = unix_now_ms();
        health_.mark_reconnecting(attempt, now_ms + health_.policy().delay_for_attempt(attempt));
        reset_iox2_endpoint();
        if (open_iox2_endpoint()) {
            health_.mark_ready();
            return true;
        }

        const auto next_attempt = attempt + 1U;
        if (health_.policy().can_retry(next_attempt)) {
            health_.mark_reconnecting(next_attempt,
                                      now_ms + health_.policy().delay_for_attempt(next_attempt));
        } else {
            health_.mark_failed(std::move(error), next_attempt);
        }
        return false;
    }

    void reset_iox2_endpoint() noexcept {
        publisher_.reset();
        subscriber_.reset();
        service_.reset();
        node_.reset();
    }

    bool publish_payload(T value, std::uint64_t published_at_ms) noexcept {
        if (!publisher_) {
            mark_transport_error("iceoryx2 publisher is not ready");
            return false;
        }

        auto sample = publisher_->loan_uninit();
        if (!sample.has_value()) {
            mark_transport_error("loan iceoryx2 sample failed");
            return false;
        }

        auto loaned_sample = std::move(sample).value();
        loaned_sample.user_header_mut().published_at_ms = published_at_ms;
        auto initialized_sample = loaned_sample.write_payload(std::move(value));
        auto sent = ::iox2::send(std::move(initialized_sample));
        if (!sent.has_value()) {
            mark_transport_error("send iceoryx2 sample failed");
            return false;
        }
        return true;
    }

    bool open_iox2_endpoint() {
        reset_iox2_endpoint();
        auto name = ::iox2::ServiceName::create(service_name_.c_str());
        if (!name.has_value()) {
            return false;
        }

        auto node = ::iox2::NodeBuilder().create<::iox2::ServiceType::Ipc>();
        if (!node.has_value()) {
            return false;
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
            return false;
        }
        service_.emplace(std::move(service).value());

        auto subscriber = service_->subscriber_builder().buffer_size(depth).create();
        if (!subscriber.has_value()) {
            service_.reset();
            node_.reset();
            return false;
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
            return false;
        }
        publisher_.emplace(std::move(publisher).value());
        return true;
    }
#endif

    std::string service_name_;
    Iox2ChannelConfig config_;
    BackendHealthTracker health_;
#ifdef FLOWRT_HAS_ICEORYX2_CXX
    std::optional<Iox2Node> node_;
    std::optional<Iox2Service> service_;
    std::optional<Iox2Publisher> publisher_;
    std::optional<Iox2Subscriber> subscriber_;
    std::optional<T> received_;
    std::optional<std::uint64_t> published_at_ms_;
#endif
};

/**
 * @brief 固定容量 canonical frame slot 的 iox2 publish-subscribe endpoint。
 *
 * @tparam T 用户组件看到的结构化消息类型。
 * @tparam Slot codegen 生成的 fixed-size iox2 payload slot。
 *
 * 该类型把动态消息本体与 iox2 typed payload 解耦。generated shell 在发布时把 `T` 编码到
 * `Slot`，接收时再从 `Slot` 解码回 `T`；用户组件接口仍只暴露结构化消息。
 */
template <typename T, typename Slot>
    requires Iox2FrameSlot<Slot, T>
class Iox2FramePubSub {
   public:
    Iox2FramePubSub(Iox2FramePubSub &&) noexcept = default;
    Iox2FramePubSub(const Iox2FramePubSub &) = delete;
    auto operator=(Iox2FramePubSub &&) noexcept -> Iox2FramePubSub & = default;
    auto operator=(const Iox2FramePubSub &) -> Iox2FramePubSub & = delete;
    ~Iox2FramePubSub() = default;

    /**
     * @brief 打开或创建一个 FlowRT iox2 frame service endpoint。
     */
    static Iox2FramePubSub open_with_config(std::string_view service_name,
                                            Iox2ChannelConfig config) {
        return Iox2FramePubSub(service_name, config);
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
     * @brief 返回 endpoint 健康快照。
     */
    BackendHealthSnapshot health() const {
        if (!ready() && health_.snapshot().state == BackendHealthState::Ready) {
            return BackendHealthSnapshot{
                .state = BackendHealthState::Degraded,
                .last_error = std::optional<std::string>{"iceoryx2 frame endpoint is not ready"},
                .attempt = 0,
                .next_retry_unix_ms = std::nullopt,
                .recoverable = true,
            };
        }
        return health_.snapshot();
    }

    /**
     * @brief 返回 endpoint 重连策略。
     */
    ReconnectPolicy reconnect_policy() const noexcept { return health_.policy(); }

#ifdef FLOWRT_ENABLE_TEST_HOOKS
    /**
     * @brief 测试钩子：模拟本地 iox2 frame endpoint 资源丢失。
     */
    void reset_transport_for_test() noexcept {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        reset_iox2_endpoint();
#endif
        health_.mark_degraded("iceoryx2 frame endpoint reset by test");
    }
#endif

    /**
     * @brief 带 FlowRT runtime 时间戳发布一个结构化变长消息。
     */
    ChannelPushResult publish_at(T value, std::uint64_t published_at_ms) noexcept {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        if (!ensure_ready()) {
            return ChannelError::Transport;
        }

        try {
            const auto slot = Slot::from_message(value);
            if (publish_slot(slot, published_at_ms)) {
                health_.mark_ready();
                return ChannelWriteOutcome::Accepted;
            }
            if (!recover_after_transport_error("publish iceoryx2 frame sample")) {
                return ChannelError::Transport;
            }
            if (!publish_slot(slot, published_at_ms)) {
                return ChannelError::Transport;
            }
            health_.mark_ready();
            return ChannelWriteOutcome::Accepted;
        } catch (...) {
            return ChannelError::Transport;
        }
#else
        (void)value;
        (void)published_at_ms;
        health_.mark_failed("iceoryx2-cxx support is disabled", health_.snapshot().attempt);
        return ChannelError::Transport;
#endif
    }

    /**
     * @brief 读取 latest snapshot，并把固定 slot 解码回结构化消息。
     */
    std::variant<Latest<T>, ChannelError> receive_latest_at(std::uint64_t now_ms) noexcept {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        if (!ensure_ready()) {
            return ChannelError::Transport;
        }

        bool retried = false;
        try {
            while (true) {
                auto received = subscriber_->receive();
                if (!received.has_value()) {
                    mark_transport_error("receive iceoryx2 frame sample");
                    if (retried ||
                        !recover_after_transport_error("receive iceoryx2 frame sample")) {
                        return ChannelError::Transport;
                    }
                    retried = true;
                    continue;
                }

                auto sample = std::move(received).value();
                if (!sample.has_value()) {
                    health_.mark_ready();
                    break;
                }

                received_ = sample->payload().decode_message();
                published_at_ms_ = sample->user_header().published_at_ms;
            }
        } catch (...) {
            return ChannelError::Transport;
        }

        const bool stale = config_.stale().stale_at(published_at_ms_, now_ms);
        const bool drop_stale = stale && config_.stale().policy() == StalePolicy::Drop;
        return Latest<T>{received_ && !drop_stale ? std::addressof(*received_) : nullptr, stale};
#else
        (void)now_ms;
        health_.mark_failed("iceoryx2-cxx support is disabled", health_.snapshot().attempt);
        return ChannelError::Transport;
#endif
    }

   private:
    Iox2FramePubSub(std::string_view service_name, Iox2ChannelConfig config)
        : service_name_(service_name), config_(config), health_(ReconnectPolicy{}) {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        if (open_iox2_endpoint()) {
            health_.mark_ready();
        } else {
            health_.mark_degraded("failed to open iceoryx2 frame endpoint");
        }
#else
        health_.mark_degraded("iceoryx2-cxx support is disabled");
#endif
    }

#ifdef FLOWRT_HAS_ICEORYX2_CXX
    using Iox2Node = ::iox2::Node<::iox2::ServiceType::Ipc>;
    using Iox2Service =
        ::iox2::PortFactoryPublishSubscribe<::iox2::ServiceType::Ipc, Slot, FlowrtIox2Header>;
    using Iox2Publisher = ::iox2::Publisher<::iox2::ServiceType::Ipc, Slot, FlowrtIox2Header>;
    using Iox2Subscriber = ::iox2::Subscriber<::iox2::ServiceType::Ipc, Slot, FlowrtIox2Header>;

    static constexpr bool safe_overflow(Iox2ChannelConfig config) noexcept {
        return config.overflow() != OverflowPolicy::Block;
    }

    static constexpr ::iox2::BackpressureStrategy backpressure_strategy(
        Iox2ChannelConfig config) noexcept {
        return config.overflow() == OverflowPolicy::Block
                   ? ::iox2::BackpressureStrategy::RetryUntilDelivered
                   : ::iox2::BackpressureStrategy::DiscardData;
    }

    bool ensure_ready() noexcept {
        if (ready()) {
            return true;
        }
        if (health_.snapshot().state != BackendHealthState::Reconnecting) {
            mark_transport_error("iceoryx2 frame endpoint is not ready");
        }
        return recover_after_transport_error("reopen iceoryx2 frame endpoint");
    }

    void mark_transport_error(std::string error) { health_.mark_degraded(std::move(error)); }

    bool recover_after_transport_error(std::string error) noexcept {
        const auto snapshot = health_.snapshot();
        if (snapshot.state == BackendHealthState::Reconnecting && snapshot.next_retry_unix_ms &&
            unix_now_ms() < *snapshot.next_retry_unix_ms) {
            return false;
        }

        const auto attempt = snapshot.attempt;
        if (!health_.policy().can_retry(attempt)) {
            health_.mark_failed("iceoryx2 frame endpoint reconnect budget exhausted", attempt);
            return false;
        }

        const auto now_ms = unix_now_ms();
        health_.mark_reconnecting(attempt, now_ms + health_.policy().delay_for_attempt(attempt));
        reset_iox2_endpoint();
        if (open_iox2_endpoint()) {
            health_.mark_ready();
            return true;
        }

        const auto next_attempt = attempt + 1U;
        if (health_.policy().can_retry(next_attempt)) {
            health_.mark_reconnecting(next_attempt,
                                      now_ms + health_.policy().delay_for_attempt(next_attempt));
        } else {
            health_.mark_failed(std::move(error), next_attempt);
        }
        return false;
    }

    void reset_iox2_endpoint() noexcept {
        publisher_.reset();
        subscriber_.reset();
        service_.reset();
        node_.reset();
    }

    bool publish_slot(Slot slot, std::uint64_t published_at_ms) noexcept {
        if (!publisher_) {
            mark_transport_error("iceoryx2 frame publisher is not ready");
            return false;
        }

        auto sample = publisher_->loan_uninit();
        if (!sample.has_value()) {
            mark_transport_error("loan iceoryx2 frame sample failed");
            return false;
        }

        auto loaned_sample = std::move(sample).value();
        loaned_sample.user_header_mut().published_at_ms = published_at_ms;
        auto initialized_sample = loaned_sample.write_payload(slot);
        auto sent = ::iox2::send(std::move(initialized_sample));
        if (!sent.has_value()) {
            mark_transport_error("send iceoryx2 frame sample failed");
            return false;
        }
        return true;
    }

    bool open_iox2_endpoint() {
        reset_iox2_endpoint();
        auto name = ::iox2::ServiceName::create(service_name_.c_str());
        if (!name.has_value()) {
            return false;
        }

        auto node = ::iox2::NodeBuilder().create<::iox2::ServiceType::Ipc>();
        if (!node.has_value()) {
            return false;
        }
        node_.emplace(std::move(node).value());

        const auto depth = static_cast<std::uint64_t>(config_.depth());
        auto service = node_->service_builder(std::move(name).value())
                           .publish_subscribe<Slot>()
                           .template user_header<FlowrtIox2Header>()
                           .enable_safe_overflow(safe_overflow(config_))
                           .history_size(depth)
                           .subscriber_max_buffer_size(depth)
                           .open_or_create();
        if (!service.has_value()) {
            node_.reset();
            return false;
        }
        service_.emplace(std::move(service).value());

        auto subscriber = service_->subscriber_builder().buffer_size(depth).create();
        if (!subscriber.has_value()) {
            service_.reset();
            node_.reset();
            return false;
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
            return false;
        }
        publisher_.emplace(std::move(publisher).value());
        return true;
    }
#endif

    std::string service_name_;
    Iox2ChannelConfig config_;
    BackendHealthTracker health_;
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

}  // namespace flowrt
