#pragma once

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <flowrt/backend_health.hpp>
#include <flowrt/channels.hpp>
#include <flowrt/executor.hpp>
#include <flowrt/service.hpp>
#include <flowrt/wire.hpp>
#include <functional>
#include <future>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <thread>
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

inline constexpr std::size_t kWakeEventId = 4;

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
 * @brief 有界 canonical frame 在 iceoryx2 typed payload 中的定容承载。
 *
 * `CAP` 来自 Contract IR 中有界变长类型的 frame 上界。slot 只承载 FlowRT canonical frame
 * bytes，不改变用户侧 `std::vector` / `std::string` API，也不改变 frame wire 格式。
 */
template <std::size_t CAP>
struct Iox2FrameSlot {
    static constexpr const char *IOX2_TYPE_NAME = "FlowRTIox2FrameSlot";

    std::uint32_t len{};
    std::array<std::uint8_t, CAP> data{};

    /**
     * @brief 从已编码的 canonical frame 构造 slot，超出 `CAP` 时返回 nullopt。
     */
    static std::optional<Iox2FrameSlot> try_from_frame(
        std::span<const std::uint8_t> frame) noexcept {
        if (frame.size() > CAP ||
            frame.size() > static_cast<std::size_t>(std::numeric_limits<std::uint32_t>::max())) {
            return std::nullopt;
        }
        Iox2FrameSlot slot{};
        slot.len = static_cast<std::uint32_t>(frame.size());
        std::copy(frame.begin(), frame.end(), slot.data.begin());
        return slot;
    }

    /**
     * @brief 从实现 canonical transport codec 的消息直接编码 slot。
     */
    template <CanonicalTransportMessage T>
    static std::optional<Iox2FrameSlot> try_from_message(const T &message) noexcept {
        auto result = try_from_message_result(message);
        if (std::holds_alternative<Iox2FrameSlot>(result)) {
            return std::get<Iox2FrameSlot>(std::move(result));
        }
        return std::nullopt;
    }

    /**
     * @brief 从消息编码 slot，失败时保留 codec 或容量错误说明。
     */
    template <CanonicalTransportMessage T>
    static std::variant<Iox2FrameSlot, std::string> try_from_message_result(
        const T &message) noexcept {
        try {
            std::vector<std::uint8_t> frame(detail::encoded_frame_size(message));
            detail::encode_frame(message, std::span<std::uint8_t>{frame});
            auto slot = try_from_frame(std::span<const std::uint8_t>{frame});
            if (slot.has_value()) {
                return std::move(*slot);
            }
            return std::string{"frame length "} + std::to_string(frame.size()) +
                   " exceeds iox2 frame slot capacity " + std::to_string(CAP);
        } catch (const std::exception &error) {
            return std::string{error.what()};
        } catch (...) {
            return std::string{"encode FlowRT iox2 frame slot failed"};
        }
    }

    /**
     * @brief 借用 slot 中的 canonical frame bytes。
     */
    [[nodiscard]] std::optional<std::span<const std::uint8_t>> frame() const noexcept {
        if (static_cast<std::size_t>(len) > CAP) {
            return std::nullopt;
        }
        return std::span<const std::uint8_t>{data.data(), static_cast<std::size_t>(len)};
    }

    /**
     * @brief 将 slot 中的 canonical frame 解码成消息。
     */
    template <CanonicalTransportMessage T>
    [[nodiscard]] std::optional<T> decode_message() const noexcept {
        const auto bytes = frame();
        if (!bytes.has_value()) {
            return std::nullopt;
        }
        try {
            return detail::decode_frame<T>(*bytes);
        } catch (...) {
            return std::nullopt;
        }
    }
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
#ifndef FLOWRT_HAS_ICEORYX2_CXX
        return BackendHealthSnapshot{
            .state = BackendHealthState::Unsupported,
            .last_error = std::optional<std::string>{"iceoryx2-cxx support is not compiled"},
            .attempt = 0,
            .next_retry_unix_ms = std::nullopt,
            .recoverable = false,
        };
#endif
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

    /**
     * @brief 返回接收侧已接受样本的修订号。
     */
    std::uint64_t revision() const noexcept { return revision_; }

    /**
     * @brief 注册 scheduler 数据到达唤醒器。
     *
     * iox2 typed pub/sub 不直接暴露 sample-arrival waitable。FlowRT 使用同名 event service
     * 作为 sideband wake：发布成功后 notifier 发送 wake event，接收侧 listener 只唤醒
     * scheduler，不读取用户 payload。
     */
    void set_schedule_waiter(ScheduleWaiter waiter) noexcept {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        schedule_waiter_ = std::move(waiter);
        if (!start_wake_listener()) {
            health_.mark_degraded("failed to start iceoryx2 wake listener");
        }
#else
        (void)waiter;
#endif
    }

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

    /**
     * @brief 测试钩子：模拟 payload 已发送但 wake notifier 失效。
     */
    void reset_wake_notifier_for_test() noexcept {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        notifier_.reset();
#endif
        health_.mark_degraded("iceoryx2 wake notifier reset by test");
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

        auto result = publish_payload(value, published_at_ms);
        if (result == PublishPayloadResult::Sent) {
            health_.mark_ready();
            return ChannelWriteOutcome::Accepted;
        }
        if (result == PublishPayloadResult::SentWakeFailed) {
            return ChannelWriteOutcome::Accepted;
        }
        if (!recover_after_transport_error("publish iceoryx2 sample")) {
            return ChannelError::Transport;
        }
        result = publish_payload(value, published_at_ms);
        if (result == PublishPayloadResult::Failed) {
            return ChannelError::Transport;
        }
        if (result == PublishPayloadResult::SentWakeFailed) {
            return ChannelWriteOutcome::Accepted;
        }
        health_.mark_ready();
        return ChannelWriteOutcome::Accepted;
#else
        (void)value;
        (void)published_at_ms;
        return ChannelError::Unsupported;
#endif
    }

    /**
     * @brief 使用当前 runtime wall clock 发布一个值。
     */
    ChannelPushResult publish(T value) noexcept { return publish_at(value, unix_now_ms()); }

    /**
     * @brief 逐条 take 一个 transport 样本，用于 FIFO/service request-response。
     */
    std::variant<std::optional<T>, ChannelError> receive() noexcept {
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
                return std::optional<T>{};
            }

            ++revision_;
            health_.mark_ready();
            return std::optional<T>{sample->payload()};
        }
#else
        return ChannelError::Unsupported;
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
            ++revision_;
        }

        return cached_latest_at(now_ms);
#else
        (void)now_ms;
        return ChannelError::Unsupported;
#endif
    }

    /**
     * @brief 返回最近一次已接收样本的 cached latest view，不触碰 transport。
     */
    Latest<T> cached_latest_at(std::uint64_t now_ms) const noexcept {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        const bool stale = config_.stale().stale_at(published_at_ms_, now_ms);
        const bool drop_stale = stale && config_.stale().policy() == StalePolicy::Drop;
        return Latest<T>{received_ && !drop_stale ? std::addressof(*received_) : nullptr, stale};
#else
        (void)now_ms;
        return Latest<T>{};
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
        // health() getter 直接返回 Unsupported 状态，无需在此标记。
#endif
    }

#ifdef FLOWRT_HAS_ICEORYX2_CXX
    using Iox2Node = ::iox2::Node<::iox2::ServiceType::Ipc>;
    using Iox2Service =
        ::iox2::PortFactoryPublishSubscribe<::iox2::ServiceType::Ipc, T, FlowrtIox2Header>;
    using Iox2Publisher = ::iox2::Publisher<::iox2::ServiceType::Ipc, T, FlowrtIox2Header>;
    using Iox2Subscriber = ::iox2::Subscriber<::iox2::ServiceType::Ipc, T, FlowrtIox2Header>;

    enum class PublishPayloadResult {
        Sent,
        SentWakeFailed,
        Failed,
    };

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
        stop_wake_listener();
        notifier_.reset();
        event_.reset();
        publisher_.reset();
        subscriber_.reset();
        service_.reset();
        node_.reset();
    }

    PublishPayloadResult publish_payload(const T &value, std::uint64_t published_at_ms) noexcept {
        if (!publisher_) {
            mark_transport_error("iceoryx2 publisher is not ready");
            return PublishPayloadResult::Failed;
        }

        auto sample = publisher_->loan_uninit();
        if (!sample.has_value()) {
            mark_transport_error("loan iceoryx2 sample failed");
            return PublishPayloadResult::Failed;
        }

        auto loaned_sample = std::move(sample).value();
        loaned_sample.user_header_mut().published_at_ms = published_at_ms;
        auto initialized_sample = loaned_sample.write_payload(value);
        auto sent = ::iox2::send(std::move(initialized_sample));
        if (!sent.has_value()) {
            mark_transport_error("send iceoryx2 sample failed");
            return PublishPayloadResult::Failed;
        }
        return notify_wake() ? PublishPayloadResult::Sent : PublishPayloadResult::SentWakeFailed;
    }

    bool notify_wake() noexcept {
        if (!notifier_) {
            mark_transport_error("iceoryx2 wake notifier is not ready");
            return false;
        }
        if (!notifier_->notify_with_custom_event_id(::iox2::EventId{kWakeEventId}).has_value()) {
            mark_transport_error("notify iceoryx2 wake event failed");
            return false;
        }
        return true;
    }

    bool start_wake_listener() noexcept {
        if (!schedule_waiter_.has_value() || wake_thread_.has_value()) {
            return true;
        }

        auto service_name = service_name_;
        auto waiter = *schedule_waiter_;
        auto ready = std::make_shared<std::promise<bool>>();
        auto ready_result = ready->get_future();
        wake_thread_.emplace([service_name = std::move(service_name), waiter = std::move(waiter),
                              ready = std::move(ready)](std::stop_token stop_token) mutable {
            auto fail_ready = [&ready]() {
                if (ready) {
                    ready->set_value(false);
                    ready.reset();
                }
            };
            auto name = ::iox2::ServiceName::create(service_name.c_str());
            if (!name.has_value()) {
                fail_ready();
                return;
            }
            auto node = ::iox2::NodeBuilder().create<::iox2::ServiceType::Ipc>();
            if (!node.has_value()) {
                fail_ready();
                return;
            }
            auto wake_node = std::move(node).value();
            auto event =
                wake_node.service_builder(std::move(name).value()).event().open_or_create();
            if (!event.has_value()) {
                fail_ready();
                return;
            }
            auto wake_event = std::move(event).value();
            auto listener = wake_event.listener_builder().create();
            if (!listener.has_value()) {
                fail_ready();
                return;
            }
            auto wake_listener = std::move(listener).value();
            if (ready) {
                ready->set_value(true);
                ready.reset();
            }

            while (!stop_token.stop_requested()) {
                auto received = wake_listener.timed_wait_one(::iox2::bb::Duration::from_millis(50));
                if (!received.has_value()) {
                    return;
                }
                if (received->has_value()) {
                    waiter.notify_data();
                }
            }
        });
        if (ready_result.wait_for(std::chrono::milliseconds{500}) != std::future_status::ready ||
            !ready_result.get()) {
            stop_wake_listener();
            mark_transport_error("failed to start iceoryx2 wake listener");
            return false;
        }
        return true;
    }

    void stop_wake_listener() noexcept { wake_thread_.reset(); }

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
        auto wake_name = ::iox2::ServiceName::create(service_name_.c_str());
        if (!wake_name.has_value()) {
            reset_iox2_endpoint();
            return false;
        }
        auto event = node_->service_builder(std::move(wake_name).value()).event().open_or_create();
        if (!event.has_value()) {
            reset_iox2_endpoint();
            return false;
        }
        event_.emplace(std::move(event).value());
        auto notifier = event_->notifier_builder().create();
        if (!notifier.has_value()) {
            reset_iox2_endpoint();
            return false;
        }
        notifier_.emplace(std::move(notifier).value());
        if (!start_wake_listener()) {
            reset_iox2_endpoint();
            return false;
        }
        return true;
    }
#endif

    std::string service_name_;
    Iox2ChannelConfig config_;
    BackendHealthTracker health_;
    std::uint64_t revision_ = 0;
#ifdef FLOWRT_HAS_ICEORYX2_CXX
    std::optional<Iox2Node> node_;
    std::optional<Iox2Service> service_;
    std::optional<::iox2::PortFactoryEvent<::iox2::ServiceType::Ipc>> event_;
    std::optional<Iox2Publisher> publisher_;
    std::optional<Iox2Subscriber> subscriber_;
    std::optional<::iox2::Notifier<::iox2::ServiceType::Ipc>> notifier_;
    std::optional<ScheduleWaiter> schedule_waiter_;
    std::optional<std::jthread> wake_thread_;
    std::optional<T> received_;
    std::optional<std::uint64_t> published_at_ms_;
#endif
};

/**
 * @brief iox2 有界变长 frame publish-subscribe endpoint。
 *
 * 该 wrapper 对 generated shell 暴露与 `Iox2PubSub<T>` 对齐的 channel surface，内部用
 * `Iox2FrameSlot<CAP>` 通过 iceoryx2 typed pub/sub 传输 canonical frame bytes。
 */
template <CanonicalTransportMessage T, std::size_t CAP>
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

    std::string_view service_name() const noexcept { return inner_.service_name(); }

    constexpr Iox2ChannelConfig config() const noexcept { return inner_.config(); }

    bool ready() const noexcept { return inner_.ready(); }

    BackendHealthSnapshot health() const { return inner_.health(); }

    ReconnectPolicy reconnect_policy() const noexcept { return inner_.reconnect_policy(); }

    std::uint64_t revision() const noexcept { return inner_.revision(); }

    void set_schedule_waiter(ScheduleWaiter waiter) noexcept {
        inner_.set_schedule_waiter(std::move(waiter));
    }

    ChannelPushResult publish_at(const T &value, std::uint64_t published_at_ms) noexcept {
        const auto slot = Iox2FrameSlot<CAP>::try_from_message(value);
        if (!slot.has_value()) {
            return ChannelError::Transport;
        }
        return inner_.publish_at(*slot, published_at_ms);
    }

    ChannelPushResult publish(const T &value) noexcept { return publish_at(value, unix_now_ms()); }

    std::variant<std::optional<T>, ChannelError> receive() noexcept {
        auto received = inner_.receive();
        if (std::holds_alternative<ChannelError>(received)) {
            return std::get<ChannelError>(received);
        }
        auto slot = std::get<std::optional<Iox2FrameSlot<CAP>>>(std::move(received));
        if (!slot.has_value()) {
            return std::optional<T>{};
        }
        auto decoded = slot->template decode_message<T>();
        if (!decoded.has_value()) {
            return ChannelError::Transport;
        }
        return std::optional<T>{std::move(*decoded)};
    }

    std::variant<Latest<T>, ChannelError> receive_latest_at(std::uint64_t now_ms) noexcept {
        auto latest = inner_.receive_latest_at(now_ms);
        if (std::holds_alternative<ChannelError>(latest)) {
            return std::get<ChannelError>(latest);
        }
        auto slot_latest = std::get<Latest<Iox2FrameSlot<CAP>>>(latest);
        latest_stale_ = slot_latest.stale();
        if (!slot_latest.present()) {
            received_.reset();
            return Latest<T>{nullptr, latest_stale_};
        }
        auto decoded = slot_latest.as_ref()->template decode_message<T>();
        if (!decoded.has_value()) {
            received_.reset();
            return ChannelError::Transport;
        }
        received_ = std::move(*decoded);
        return Latest<T>{std::addressof(*received_), latest_stale_};
    }

    Latest<T> cached_latest_at(std::uint64_t now_ms) const noexcept {
        const auto slot_latest = inner_.cached_latest_at(now_ms);
        const auto stale = slot_latest.stale();
        if (!slot_latest.present() || !received_.has_value()) {
            return Latest<T>{nullptr, stale};
        }
        return Latest<T>{std::addressof(*received_), stale};
    }

   private:
    Iox2FramePubSub(std::string_view service_name, Iox2ChannelConfig config)
        : inner_(Iox2PubSub<Iox2FrameSlot<CAP>>::open_with_config(service_name, config)) {}

    Iox2PubSub<Iox2FrameSlot<CAP>> inner_;
    std::optional<T> received_;
    bool latest_stale_ = false;
};

inline std::string iox2_request_endpoint(std::string_view service_name) {
    std::string endpoint{service_name};
    endpoint += "/req";
    return endpoint;
}

inline std::string iox2_response_endpoint(std::string_view service_name) {
    std::string endpoint{service_name};
    endpoint += "/resp";
    return endpoint;
}

inline std::uint64_t next_session_id() noexcept {
    static std::atomic<std::uint64_t> next{1U};
    return next.fetch_add(1U, std::memory_order_relaxed);
}

/**
 * @brief iox2 service 请求/响应 correlation。
 */
struct Iox2ServiceCorrelation {
    static constexpr const char *IOX2_TYPE_NAME = "FlowRTIox2ServiceCorrelation";

    std::uint64_t session_id{};
    std::uint64_t sequence{};
    std::uint64_t service_id{};
};

template <typename Req>
struct Iox2ServiceRequest {
    static constexpr const char *IOX2_TYPE_NAME = "FlowRTIox2ServiceRequest";

    Iox2ServiceCorrelation correlation{};
    Req payload{};
};

template <typename Resp>
struct Iox2ServiceResponse {
    static constexpr const char *IOX2_TYPE_NAME = "FlowRTIox2ServiceResponse";

    Iox2ServiceCorrelation correlation{};
    std::uint16_t status{};
    std::uint8_t pad[6]{};
    Resp payload{};
};

template <typename Resp>
ServiceResult<Resp> decode_service_response(Iox2ServiceResponse<Resp> response) {
    const auto error = service_error_from_abi(response.status);
    if (!error.has_value()) {
        return ServiceResult<Resp>::err(ServiceError::Protocol);
    }
    if (*error == ServiceError::Ok) {
        return ServiceResult<Resp>::ok(response.payload);
    }
    return ServiceResult<Resp>::err(*error);
}

inline BackendHealthSnapshot unsupported_health(std::string error) {
    return BackendHealthSnapshot{
        .state = BackendHealthState::Unsupported,
        .last_error = std::move(error),
        .attempt = 0,
        .next_retry_unix_ms = std::nullopt,
        .recoverable = false,
    };
}

/**
 * @brief iox2 fixed-size Service client backend primitive。
 */
template <typename Req, typename Resp>
class Iox2ServiceClient {
   public:
    Iox2ServiceClient(const Iox2ServiceClient &) = delete;
    auto operator=(const Iox2ServiceClient &) -> Iox2ServiceClient & = delete;
    Iox2ServiceClient(Iox2ServiceClient &&) = delete;
    auto operator=(Iox2ServiceClient &&) -> Iox2ServiceClient & = delete;
    ~Iox2ServiceClient() = default;

    static Iox2ServiceClient open(std::string_view service_name) {
        return Iox2ServiceClient(service_name, std::nullopt);
    }

    static std::shared_ptr<Iox2ServiceClient> open_shared(std::string_view service_name) {
        return std::shared_ptr<Iox2ServiceClient>(
            new Iox2ServiceClient(service_name, std::nullopt));
    }

    static Iox2ServiceClient unavailable(std::string_view service_name, std::string error) {
        return Iox2ServiceClient(service_name, std::move(error));
    }

    static std::shared_ptr<Iox2ServiceClient> unavailable_shared(std::string_view service_name,
                                                                std::string error) {
        return std::shared_ptr<Iox2ServiceClient>(
            new Iox2ServiceClient(service_name, std::move(error)));
    }

    ServiceResult<Resp> call(const Req &request, std::uint64_t timeout_ms) {
        if (unavailable_.has_value()) {
            return ServiceResult<Resp>::err_with_message(ServiceError::Unavailable, *unavailable_);
        }
        if (timeout_ms == 0U) {
            return ServiceResult<Resp>::err(ServiceError::Timeout);
        }

        std::lock_guard<std::mutex> lock(mutex_);
        if (!request_tx_.has_value() || !response_rx_.has_value()) {
            return ServiceResult<Resp>::err(ServiceError::Unavailable);
        }

        const auto sequence = sequence_++;
        const Iox2ServiceCorrelation correlation{
            .session_id = session_id_,
            .sequence = sequence,
            .service_id = service_id_,
        };
        auto published = request_tx_->publish(
            Iox2ServiceRequest<Req>{.correlation = correlation, .payload = request});
        if (std::holds_alternative<ChannelError>(published)) {
            const auto error = std::get<ChannelError>(published);
            return ServiceResult<Resp>::err(error == ChannelError::Unsupported
                                                ? ServiceError::Unavailable
                                                : ServiceError::Backend);
        }

        if (auto pending = pending_.find(sequence); pending != pending_.end()) {
            auto response = pending->second;
            pending_.erase(pending);
            return decode_service_response(response);
        }

        const auto deadline =
            std::chrono::steady_clock::now() + std::chrono::milliseconds(timeout_ms);
        while (true) {
            auto received = response_rx_->receive();
            if (std::holds_alternative<ChannelError>(received)) {
                const auto error = std::get<ChannelError>(received);
                return ServiceResult<Resp>::err(error == ChannelError::Unsupported
                                                    ? ServiceError::Unavailable
                                                    : ServiceError::Backend);
            }

            auto response = std::get<std::optional<Iox2ServiceResponse<Resp>>>(received);
            if (response.has_value() && response->correlation.service_id == service_id_ &&
                response->correlation.session_id == session_id_) {
                if (response->correlation.sequence == sequence) {
                    return decode_service_response(*response);
                }
                pending_.emplace(response->correlation.sequence, *response);
            }

            if (std::chrono::steady_clock::now() >= deadline) {
                return ServiceResult<Resp>::err(ServiceError::Timeout);
            }
            std::this_thread::sleep_for(std::chrono::milliseconds{1});
        }
    }

    std::string_view service_name() const noexcept { return service_name_; }

    BackendHealthSnapshot health() const {
        if (unavailable_.has_value()) {
            return unsupported_health(*unavailable_);
        }
        std::lock_guard<std::mutex> lock(mutex_);
        if (!request_tx_.has_value()) {
            return unsupported_health("iceoryx2 service client is not open");
        }
        return request_tx_->health();
    }

   private:
    Iox2ServiceClient(std::string_view service_name, std::optional<std::string> unavailable)
        : service_name_(service_name),
          session_id_(next_session_id()),
          service_id_(fnv1a64(service_name_)),
          unavailable_(std::move(unavailable)) {
        static_assert(std::is_trivially_copyable_v<Req>,
                      "FlowRT iox2 service request must be trivially copyable");
        static_assert(std::is_trivially_copyable_v<Resp>,
                      "FlowRT iox2 service response must be trivially copyable");
        if (!unavailable_.has_value()) {
            const auto config = Iox2ChannelConfig::fifo(64U, OverflowPolicy::DropOldest);
            request_tx_.emplace(Iox2PubSub<Iox2ServiceRequest<Req>>::open_with_config(
                iox2_request_endpoint(service_name_), config));
            response_rx_.emplace(Iox2PubSub<Iox2ServiceResponse<Resp>>::open_with_config(
                iox2_response_endpoint(service_name_), config));
        }
    }

    std::string service_name_;
    std::uint64_t session_id_;
    std::uint64_t service_id_;
    mutable std::mutex mutex_;
    std::uint64_t sequence_ = 0;
    std::optional<Iox2PubSub<Iox2ServiceRequest<Req>>> request_tx_;
    std::optional<Iox2PubSub<Iox2ServiceResponse<Resp>>> response_rx_;
    std::map<std::uint64_t, Iox2ServiceResponse<Resp>> pending_;
    std::optional<std::string> unavailable_;
};

/**
 * @brief iox2 有界变长 Service client backend primitive。
 */
template <CanonicalTransportMessage Req, CanonicalTransportMessage Resp, std::size_t REQ_CAP,
          std::size_t RESP_CAP>
class Iox2FrameServiceClient {
   public:
    Iox2FrameServiceClient(const Iox2FrameServiceClient &) = delete;
    auto operator=(const Iox2FrameServiceClient &) -> Iox2FrameServiceClient & = delete;
    Iox2FrameServiceClient(Iox2FrameServiceClient &&) = delete;
    auto operator=(Iox2FrameServiceClient &&) -> Iox2FrameServiceClient & = delete;
    ~Iox2FrameServiceClient() = default;

    static Iox2FrameServiceClient open(std::string_view service_name) {
        return Iox2FrameServiceClient(service_name, std::nullopt);
    }

    static std::shared_ptr<Iox2FrameServiceClient> open_shared(std::string_view service_name) {
        return std::shared_ptr<Iox2FrameServiceClient>(
            new Iox2FrameServiceClient(service_name, std::nullopt));
    }

    static Iox2FrameServiceClient unavailable(std::string_view service_name, std::string error) {
        return Iox2FrameServiceClient(service_name, std::move(error));
    }

    static std::shared_ptr<Iox2FrameServiceClient> unavailable_shared(std::string_view service_name,
                                                                     std::string error) {
        return std::shared_ptr<Iox2FrameServiceClient>(
            new Iox2FrameServiceClient(service_name, std::move(error)));
    }

    ServiceResult<Resp> call(const Req &request, std::uint64_t timeout_ms) {
        auto request_slot = Iox2FrameSlot<REQ_CAP>::try_from_message_result(request);
        if (std::holds_alternative<std::string>(request_slot)) {
            return ServiceResult<Resp>::err_with_message(ServiceError::Backend,
                                                         std::get<std::string>(request_slot));
        }
        auto response =
            inner_.call(std::get<Iox2FrameSlot<REQ_CAP>>(std::move(request_slot)), timeout_ms);
        if (response.is_err()) {
            if (response.error_message().has_value()) {
                return ServiceResult<Resp>::err_with_message(response.error_code(),
                                                             *response.error_message());
            }
            return ServiceResult<Resp>::err(response.error_code());
        }
        const auto response_slot = response.value();
        if (response_slot == nullptr) {
            return ServiceResult<Resp>::err(ServiceError::Protocol);
        }
        auto decoded = response_slot->template decode_message<Resp>();
        if (!decoded.has_value()) {
            return ServiceResult<Resp>::err(ServiceError::Protocol);
        }
        return ServiceResult<Resp>::ok(std::move(*decoded));
    }

    std::string_view service_name() const noexcept { return inner_.service_name(); }

    BackendHealthSnapshot health() const { return inner_.health(); }

   private:
    Iox2FrameServiceClient(std::string_view service_name, std::optional<std::string> unavailable)
        : inner_(unavailable.has_value()
                     ? InnerClient::unavailable(service_name, std::move(*unavailable))
                     : InnerClient::open(service_name)) {}

    using InnerClient = Iox2ServiceClient<Iox2FrameSlot<REQ_CAP>, Iox2FrameSlot<RESP_CAP>>;

    InnerClient inner_;
};

/**
 * @brief iox2 fixed-size Service server backend primitive。
 */
template <typename Req, typename Resp>
class Iox2ServiceServer {
   public:
    Iox2ServiceServer(const Iox2ServiceServer &) = delete;
    auto operator=(const Iox2ServiceServer &) -> Iox2ServiceServer & = delete;
    Iox2ServiceServer(Iox2ServiceServer &&) = default;
    auto operator=(Iox2ServiceServer &&) -> Iox2ServiceServer & = default;
    ~Iox2ServiceServer() = default;

    static Iox2ServiceServer open(std::string_view service_name, std::size_t max_in_flight) {
        return Iox2ServiceServer(service_name, max_in_flight, std::nullopt);
    }

    static Iox2ServiceServer unavailable(std::string_view service_name, std::string error) {
        return Iox2ServiceServer(service_name, 1U, std::move(error));
    }

    void set_schedule_waiter(ScheduleWaiter waiter) noexcept {
        if (request_rx_.has_value()) {
            request_rx_->set_schedule_waiter(std::move(waiter));
        }
    }

    std::optional<std::size_t> poll_requests(
        const std::function<ServiceResult<Resp>(Req)> &handler) {
        if (unavailable_.has_value() || !request_rx_.has_value() || !response_tx_.has_value()) {
            return std::nullopt;
        }

        std::size_t handled = 0;
        while (handled < max_in_flight_) {
            auto received = request_rx_->receive();
            if (std::holds_alternative<ChannelError>(received)) {
                return std::nullopt;
            }
            auto request = std::get<std::optional<Iox2ServiceRequest<Req>>>(received);
            if (!request.has_value()) {
                break;
            }

            const auto result = handler(request->payload);
            const auto status = static_cast<std::uint16_t>(result.error_code());
            const auto payload = result.value() == nullptr ? Resp{} : *result.value();
            auto published = response_tx_->publish(Iox2ServiceResponse<Resp>{
                .correlation = request->correlation,
                .status = status,
                .pad = {},
                .payload = payload,
            });
            if (std::holds_alternative<ChannelError>(published)) {
                return std::nullopt;
            }
            ++handled;
        }
        return handled;
    }

    std::string_view service_name() const noexcept { return service_name_; }

    BackendHealthSnapshot health() const {
        if (unavailable_.has_value()) {
            return unsupported_health(*unavailable_);
        }
        if (!request_rx_.has_value()) {
            return unsupported_health("iceoryx2 service server is not open");
        }
        return request_rx_->health();
    }

   private:
    Iox2ServiceServer(std::string_view service_name, std::size_t max_in_flight,
                      std::optional<std::string> unavailable)
        : service_name_(service_name),
          max_in_flight_(max_in_flight == 0U ? 1U : max_in_flight),
          unavailable_(std::move(unavailable)) {
        static_assert(std::is_trivially_copyable_v<Req>,
                      "FlowRT iox2 service request must be trivially copyable");
        static_assert(std::is_trivially_copyable_v<Resp>,
                      "FlowRT iox2 service response must be trivially copyable");
        if (!unavailable_.has_value()) {
            const auto config = Iox2ChannelConfig::fifo(64U, OverflowPolicy::DropOldest);
            request_rx_.emplace(Iox2PubSub<Iox2ServiceRequest<Req>>::open_with_config(
                iox2_request_endpoint(service_name_), config));
            response_tx_.emplace(Iox2PubSub<Iox2ServiceResponse<Resp>>::open_with_config(
                iox2_response_endpoint(service_name_), config));
        }
    }

    std::string service_name_;
    std::optional<Iox2PubSub<Iox2ServiceRequest<Req>>> request_rx_;
    std::optional<Iox2PubSub<Iox2ServiceResponse<Resp>>> response_tx_;
    std::size_t max_in_flight_;
    std::optional<std::string> unavailable_;
};

/**
 * @brief iox2 有界变长 Service server backend primitive。
 */
template <CanonicalTransportMessage Req, CanonicalTransportMessage Resp, std::size_t REQ_CAP,
          std::size_t RESP_CAP>
class Iox2FrameServiceServer {
   public:
    Iox2FrameServiceServer(const Iox2FrameServiceServer &) = delete;
    auto operator=(const Iox2FrameServiceServer &) -> Iox2FrameServiceServer & = delete;
    Iox2FrameServiceServer(Iox2FrameServiceServer &&) = default;
    auto operator=(Iox2FrameServiceServer &&) -> Iox2FrameServiceServer & = default;
    ~Iox2FrameServiceServer() = default;

    static Iox2FrameServiceServer open(std::string_view service_name, std::size_t max_in_flight) {
        return Iox2FrameServiceServer(service_name, max_in_flight, std::nullopt);
    }

    static Iox2FrameServiceServer unavailable(std::string_view service_name, std::string error) {
        return Iox2FrameServiceServer(service_name, 1U, std::move(error));
    }

    void set_schedule_waiter(ScheduleWaiter waiter) noexcept {
        inner_.set_schedule_waiter(std::move(waiter));
    }

    std::optional<std::size_t> poll_requests(
        const std::function<ServiceResult<Resp>(Req)> &handler) {
        return inner_.poll_requests([&handler](Iox2FrameSlot<REQ_CAP> slot) {
            auto request = slot.template decode_message<Req>();
            if (!request.has_value()) {
                return ServiceResult<Iox2FrameSlot<RESP_CAP>>::err(ServiceError::Protocol);
            }
            auto result = handler(std::move(*request));
            if (result.is_err()) {
                if (result.error_message().has_value()) {
                    return ServiceResult<Iox2FrameSlot<RESP_CAP>>::err_with_message(
                        result.error_code(), *result.error_message());
                }
                return ServiceResult<Iox2FrameSlot<RESP_CAP>>::err(result.error_code());
            }
            const auto value = result.value();
            if (value == nullptr) {
                return ServiceResult<Iox2FrameSlot<RESP_CAP>>::err(ServiceError::Protocol);
            }
            auto response_slot = Iox2FrameSlot<RESP_CAP>::try_from_message_result(*value);
            if (std::holds_alternative<std::string>(response_slot)) {
                return ServiceResult<Iox2FrameSlot<RESP_CAP>>::err_with_message(
                    ServiceError::Backend, std::get<std::string>(response_slot));
            }
            return ServiceResult<Iox2FrameSlot<RESP_CAP>>::ok(
                std::get<Iox2FrameSlot<RESP_CAP>>(std::move(response_slot)));
        });
    }

    std::string_view service_name() const noexcept { return inner_.service_name(); }

    BackendHealthSnapshot health() const { return inner_.health(); }

   private:
    Iox2FrameServiceServer(std::string_view service_name, std::size_t max_in_flight,
                           std::optional<std::string> unavailable)
        : inner_(unavailable.has_value()
                     ? InnerServer::unavailable(service_name, std::move(*unavailable))
                     : InnerServer::open(service_name, max_in_flight)) {}

    using InnerServer = Iox2ServiceServer<Iox2FrameSlot<REQ_CAP>, Iox2FrameSlot<RESP_CAP>>;

    InnerServer inner_;
};

}  // namespace iox2

}  // namespace flowrt
