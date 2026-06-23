#pragma once

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <cstdlib>
#include <deque>
#include <exception>
#include <flowrt/backend_health.hpp>
#include <flowrt/channels.hpp>
#include <flowrt/executor.hpp>
#include <flowrt/introspection/json.hpp>
#include <flowrt/introspection/request_parser.hpp>
#include <flowrt/introspection/state.hpp>
#include <flowrt/service.hpp>
#include <flowrt/wire.hpp>
#include <functional>
#include <future>
#include <limits>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <string_view>
#include <thread>
#include <utility>
#include <variant>
#include <vector>

#ifdef FLOWRT_HAS_ZENOH_CXX
#include <zenoh.hxx>
#endif

namespace flowrt {

namespace zenoh {

inline constexpr std::uint64_t SERVICE_TRANSPORT_TIMEOUT_GRACE_MS = 1000U;

inline std::string json_string(std::string_view value) {
    std::string output = "\"";
    for (const char ch : value) {
        switch (ch) {
            case '\\':
                output += "\\\\";
                break;
            case '"':
                output += "\\\"";
                break;
            case '\n':
                output += "\\n";
                break;
            case '\r':
                output += "\\r";
                break;
            case '\t':
                output += "\\t";
                break;
            default:
                output += ch;
                break;
        }
    }
    output += "\"";
    return output;
}

inline std::vector<std::string> endpoint_list_items(std::string_view raw) {
    std::vector<std::string> endpoints;
    std::size_t start = 0;
    while (start <= raw.size()) {
        const auto comma = raw.find(',', start);
        const auto end = comma == std::string_view::npos ? raw.size() : comma;
        auto item = raw.substr(start, end - start);
        while (!item.empty() && (item.front() == ' ' || item.front() == '\t')) {
            item.remove_prefix(1);
        }
        while (!item.empty() && (item.back() == ' ' || item.back() == '\t')) {
            item.remove_suffix(1);
        }
        if (!item.empty()) {
            endpoints.emplace_back(item);
        }
        if (comma == std::string_view::npos) {
            break;
        }
        start = comma + 1;
    }
    return endpoints;
}

inline std::string endpoint_list_json(std::string_view raw) {
    const auto endpoints = endpoint_list_items(raw);
    if (endpoints.empty()) {
        return {};
    }

    std::string json = "[";
    for (std::size_t index = 0; index < endpoints.size(); ++index) {
        if (index != 0U) {
            json += ",";
        }
        json += json_string(endpoints[index]);
    }
    json += "]";
    return json;
}

inline bool env_flag_enabled(const char *value) noexcept {
    if (value == nullptr) {
        return false;
    }
    const auto flag = std::string_view{value};
    return flag == "1" || flag == "true" || flag == "TRUE" || flag == "yes" || flag == "on";
}

inline std::string service_key_expr(std::string_view service_name) {
    static constexpr char HEX[] = "0123456789ABCDEF";

    std::string encoded;
    encoded.reserve(service_name.size());
    for (const unsigned char byte : service_name) {
        const bool keep =
            (byte >= static_cast<unsigned char>('a') && byte <= static_cast<unsigned char>('z')) ||
            (byte >= static_cast<unsigned char>('A') && byte <= static_cast<unsigned char>('Z')) ||
            (byte >= static_cast<unsigned char>('0') && byte <= static_cast<unsigned char>('9')) ||
            byte == static_cast<unsigned char>('.') || byte == static_cast<unsigned char>('-');
        if (keep) {
            encoded.push_back(static_cast<char>(byte));
        } else {
            encoded.push_back('_');
            encoded.push_back('x');
            encoded.push_back(HEX[(byte >> 4U) & 0x0FU]);
            encoded.push_back(HEX[byte & 0x0FU]);
            encoded.push_back('_');
        }
    }

    return "flowrt/service/" + encoded + "/request";
}

inline std::string operation_key_expr(std::string_view package, std::string_view selfdesc_hash,
                                      std::uint32_t pid) {
    std::string key;
    key.reserve(std::string_view{"flowrt/op/"}.size() + package.size() + 1U + selfdesc_hash.size() +
                1U + 10U);
    key.append("flowrt/op/");
    key.append(package);
    key.push_back('/');
    key.append(selfdesc_hash);
    key.push_back('/');
    key.append(std::to_string(pid));
    return key;
}

template <typename Resp>
inline ServiceResult<Resp> service_result_from_response_error_code(std::uint16_t error_code,
                                                                   std::string_view message) {
    if (const auto parsed = service_error_from_abi(error_code); parsed.has_value()) {
        if (message.empty()) {
            return ServiceResult<Resp>::err(parsed.value());
        }
        return ServiceResult<Resp>::err_with_message(parsed.value(), std::string(message));
    }
    std::string detail = "unknown service error code " + std::to_string(error_code);
    if (!message.empty()) {
        detail.append(": ");
        detail.append(message);
    }
    return ServiceResult<Resp>::err_with_message(ServiceError::Protocol, std::move(detail));
}

/**
 * @brief zenoh service runtime 配置。
 *
 * 当前配置只暴露 in-flight handler 上限。该上限限制同时运行的 handler 线程数量，
 * 防止 queryable callback 在请求洪峰下无限制 detach 线程。
 */
class ZenohServiceConfig {
   public:
    static constexpr ZenohServiceConfig defaults() noexcept { return ZenohServiceConfig{}; }

    constexpr std::size_t max_in_flight() const noexcept { return max_in_flight_; }

    constexpr void set_max_in_flight(std::size_t value) noexcept {
        max_in_flight_ = value == 0U ? 1U : value;
    }

   private:
    std::size_t max_in_flight_ = 64;
};

/**
 * @brief 打开 zenoh publish-subscribe endpoint 时使用的 FlowRT channel 配置。
 *
 * 该类型承载 Contract IR channel policy 归一化后的 depth、overflow 和 freshness intent。
 * 它不暴露 zenoh-cpp API；generated shell 只通过该配置和 `ZenohPubSub<T>` 绑定 transport。
 */
class ZenohChannelConfig {
   public:
    /**
     * @brief channel buffering kind。
     */
    enum class Kind : std::uint8_t {
        Latest = 0,
        Fifo = 1,
    };

    /**
     * @brief 构造 latest channel 的默认配置。
     *
     * @return depth 为 1、overflow 为 DropOldest 的配置。
     */
    static constexpr ZenohChannelConfig latest() noexcept { return ZenohChannelConfig{}; }

    /**
     * @brief 构造 FIFO channel 配置。
     *
     * @param depth 队列深度；传入 0 时按 1 处理。
     * @param overflow 队列满时的 FlowRT 语义。
     * @return 归一化后的配置。
     */
    static constexpr ZenohChannelConfig fifo(std::size_t depth, OverflowPolicy overflow) noexcept {
        return ZenohChannelConfig(Kind::Fifo, depth == 0 ? 1 : depth, overflow, StaleConfig{});
    }

    /**
     * @brief 设置 freshness 配置。
     *
     * @param stale stale-data policy 和时间窗口。
     * @return 更新后的配置副本。
     */
    constexpr ZenohChannelConfig with_stale_config(StaleConfig stale) const noexcept {
        return ZenohChannelConfig(kind_, depth_, overflow_, stale);
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

    /**
     * @brief 判断是否为 latest channel。
     */
    constexpr bool is_latest() const noexcept { return kind_ == Kind::Latest; }

   private:
    constexpr ZenohChannelConfig() noexcept = default;

    constexpr ZenohChannelConfig(Kind kind, std::size_t depth, OverflowPolicy overflow,
                                 StaleConfig stale) noexcept
        : kind_(kind), depth_(depth), overflow_(overflow), stale_(stale) {}

    Kind kind_ = Kind::Latest;
    std::size_t depth_ = 1;
    OverflowPolicy overflow_ = OverflowPolicy::DropOldest;
    StaleConfig stale_;
};

/**
 * @brief canonical wire message 的 zenoh publish-subscribe endpoint。
 *
 * @tparam T 满足 `CanonicalTransportMessage` 的 generated message 类型。
 *
 * 开启 `FLOWRT_HAS_ZENOH_CXX` 时，该类绑定 zenoh-cpp 1.9 publish-subscribe endpoint，并把
 * runtime timestamp 与 generated message canonical payload 组合成 wire frame。默认构建不包含或
 * 依赖 zenoh-cpp，并保持安全失败语义。业务组件接口不应暴露该类型。
 */
template <CanonicalTransportMessage T>
class ZenohPubSub {
   public:
    ZenohPubSub(ZenohPubSub &&) noexcept = default;
    ZenohPubSub(const ZenohPubSub &) = delete;
    auto operator=(ZenohPubSub &&) noexcept -> ZenohPubSub & = default;
    auto operator=(const ZenohPubSub &) -> ZenohPubSub & = delete;
    ~ZenohPubSub() = default;

    /**
     * @brief 打开一个 canonical zenoh key expression 对应的 endpoint。
     *
     * @param key_expr generated shell 提供的 canonical key expression。
     * @param config 从 Contract IR channel policy 生成的配置。
     * @return endpoint 对象；未开启 zenoh-cpp 支持时 `ready()` 返回 false。
     */
    static ZenohPubSub open_with_config(std::string_view key_expr, ZenohChannelConfig config) {
        return ZenohPubSub(key_expr, config);
    }

    /**
     * @brief 返回 canonical zenoh key expression。
     */
    std::string_view key_expr() const noexcept { return key_expr_; }

    /**
     * @brief 返回 channel 配置。
     */
    constexpr ZenohChannelConfig config() const noexcept { return config_; }

    /**
     * @brief 判断 endpoint 是否已经绑定到底层 zenoh transport 资源。
     */
    bool ready() const noexcept {
#ifdef FLOWRT_HAS_ZENOH_CXX
        return session_.has_value() && publisher_.has_value() && subscriber_.has_value() &&
               !session_->is_closed();
#else
        return false;
#endif
    }

    /**
     * @brief 返回 endpoint 健康快照。
     */
    BackendHealthSnapshot health() const {
#ifndef FLOWRT_HAS_ZENOH_CXX
        return BackendHealthSnapshot{
            .state = BackendHealthState::Unsupported,
            .last_error = std::optional<std::string>{"zenoh-cpp support is not compiled"},
            .attempt = 0,
            .next_retry_unix_ms = std::nullopt,
            .recoverable = false,
        };
#endif
        if (!ready() && health_.snapshot().state == BackendHealthState::Ready) {
            return BackendHealthSnapshot{
                .state = BackendHealthState::Degraded,
                .last_error = std::optional<std::string>{"Zenoh endpoint is not ready"},
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
     * zenoh callback 只把 canonical frame 放进 endpoint 内部有界 inbox，然后通知 generated
     * scheduler；用户组件仍由 FlowRT scheduler 同步调用。
     */
    void set_schedule_waiter(ScheduleWaiter waiter) noexcept {
#ifdef FLOWRT_HAS_ZENOH_CXX
        if (inbox_) {
            inbox_->set_schedule_waiter(std::move(waiter));
        }
#else
        (void)waiter;
#endif
    }

#ifdef FLOWRT_ENABLE_TEST_HOOKS
    /**
     * @brief 测试钩子：模拟本地 session 被关闭。
     */
    void close_session_for_test() {
#ifdef FLOWRT_HAS_ZENOH_CXX
        if (session_) {
            session_->close();
        }
#endif
    }
#endif

    /**
     * @brief 带 FlowRT runtime 时间戳发布一个 canonical wire message。
     *
     * @param value 要发布的 generated message。
     * @param published_at_ms 样本发布时间，单位为 runtime 毫秒。
     * @return 未开启 zenoh-cpp 支持时返回 `ChannelError::Transport`。
     */
    ChannelPushResult publish_at(T value, std::uint64_t published_at_ms) noexcept {
#ifdef FLOWRT_HAS_ZENOH_CXX
        if (!ensure_ready()) {
            return ChannelError::Transport;
        }

        auto frame = encode_transport_frame(value, published_at_ms);
        if (!frame) {
            return ChannelError::Transport;
        }
        if (publish_frame(std::move(*frame))) {
            health_.mark_ready();
            return ChannelWriteOutcome::Accepted;
        }
        if (!recover_after_transport_error("publish Zenoh sample")) {
            return ChannelError::Transport;
        }
        frame = encode_transport_frame(value, published_at_ms);
        if (!frame) {
            return ChannelError::Transport;
        }
        if (!publish_frame(std::move(*frame))) {
            return ChannelError::Transport;
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
     * @brief 非阻塞读取 latest snapshot。
     *
     * @param now_ms 当前 runtime 时间，单位为毫秒。
     * @return 读取成功时返回 latest view；未开启 zenoh-cpp 支持或 transport/codec 失败时返回
     * `ChannelError::Transport`。
     *
     * zenoh subscriber 使用有界 `RingChannel`，callback 在容量满时覆盖旧样本而不阻塞。latest
     * channel 会排空当前 ring 后暴露最新值；FIFO channel 每次只消费一个最旧可用样本。
     */
    std::variant<Latest<T>, ChannelError> receive_latest_at(std::uint64_t now_ms) noexcept {
#ifdef FLOWRT_HAS_ZENOH_CXX
        if (!ensure_ready()) {
            return ChannelError::Transport;
        }

        bool retried = false;
        try {
            for (;;) {
                auto frame = inbox_->pop();
                if (frame.has_value()) {
                    if (!decode_frame(*frame)) {
                        return ChannelError::Transport;
                    }
                    if (!config_.is_latest()) {
                        break;
                    }
                    continue;
                }
                health_.mark_ready();
                break;
            }
        } catch (const std::exception &error) {
            mark_transport_error(error.what());
            if (retried || !recover_after_transport_error("receive Zenoh sample")) {
                return ChannelError::Transport;
            }
            retried = true;
        } catch (...) {
            mark_transport_error("receive Zenoh sample failed");
            if (retried || !recover_after_transport_error("receive Zenoh sample")) {
                return ChannelError::Transport;
            }
            retried = true;
        }

        if (retried) {
            return receive_latest_at(now_ms);
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
#ifdef FLOWRT_HAS_ZENOH_CXX
        const bool stale = config_.stale().stale_at(published_at_ms_, now_ms);
        const bool drop_stale = stale && config_.stale().policy() == StalePolicy::Drop;
        return Latest<T>{received_ && !drop_stale ? std::addressof(*received_) : nullptr, stale};
#else
        (void)now_ms;
        return Latest<T>{};
#endif
    }

   private:
    ZenohPubSub(std::string_view key_expr, ZenohChannelConfig config)
        : key_expr_(key_expr), config_(config), health_(ReconnectPolicy{}) {
#ifdef FLOWRT_HAS_ZENOH_CXX
        if (open_zenoh_endpoint()) {
            health_.mark_ready();
        } else {
            health_.mark_degraded("failed to open Zenoh endpoint");
        }
#else
        // health() getter 直接返回 Unsupported 状态，无需在此标记。
#endif
    }

    bool ensure_ready() noexcept {
#ifdef FLOWRT_HAS_ZENOH_CXX
        if (ready()) {
            return true;
        }
        if (health_.snapshot().state != BackendHealthState::Reconnecting) {
            mark_transport_error("Zenoh endpoint is not ready");
        }
        return recover_after_transport_error("reopen Zenoh endpoint");
#else
        return false;
#endif
    }

    void mark_transport_error(std::string error) { health_.mark_degraded(std::move(error)); }

    bool recover_after_transport_error(std::string error) noexcept {
#ifdef FLOWRT_HAS_ZENOH_CXX
        if (config_.overflow() != OverflowPolicy::DropOldest) {
            health_.mark_failed("Zenoh channel config is not recoverable",
                                health_.snapshot().attempt);
            return false;
        }

        const auto snapshot = health_.snapshot();
        if (snapshot.state == BackendHealthState::Reconnecting && snapshot.next_retry_unix_ms &&
            unix_now_ms() < *snapshot.next_retry_unix_ms) {
            return false;
        }

        const auto attempt = snapshot.attempt;
        if (!health_.policy().can_retry(attempt)) {
            health_.mark_failed("Zenoh endpoint reconnect budget exhausted", attempt);
            return false;
        }

        const auto now_ms = unix_now_ms();
        health_.mark_reconnecting(attempt, now_ms + health_.policy().delay_for_attempt(attempt));
        subscriber_.reset();
        publisher_.reset();
        session_.reset();
        if (open_zenoh_endpoint()) {
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
#else
        return false;
#endif
    }

#ifdef FLOWRT_HAS_ZENOH_CXX
    struct ZenohInbox {
        explicit ZenohInbox(std::size_t depth) : depth_(std::max<std::size_t>(1, depth)) {}

        void push(::zenoh::Sample &sample) {
            {
                std::lock_guard lock(mutex_);
                if (frames_.size() >= depth_) {
                    frames_.pop_front();
                }
                frames_.push_back(sample.get_payload().as_vector());
            }
            std::optional<ScheduleWaiter> waiter;
            {
                std::lock_guard lock(mutex_);
                waiter = schedule_waiter_;
            }
            if (waiter.has_value()) {
                waiter->notify_data();
            }
        }

        std::optional<std::vector<std::uint8_t>> pop() {
            std::lock_guard lock(mutex_);
            if (frames_.empty()) {
                return std::nullopt;
            }
            auto frame = std::move(frames_.front());
            frames_.pop_front();
            return frame;
        }

        void set_schedule_waiter(ScheduleWaiter waiter) {
            std::lock_guard lock(mutex_);
            schedule_waiter_ = std::move(waiter);
        }

        std::size_t depth_;
        std::mutex mutex_;
        std::deque<std::vector<std::uint8_t>> frames_;
        std::optional<ScheduleWaiter> schedule_waiter_;
    };

    using ZenohSubscriber = ::zenoh::Subscriber<void>;

    static constexpr std::size_t timestamp_wire_size() noexcept { return sizeof(std::uint64_t); }

    bool open_zenoh_endpoint() noexcept {
        if (config_.overflow() != OverflowPolicy::DropOldest) {
            return false;
        }

        try {
            if (!inbox_) {
                inbox_ = std::make_shared<ZenohInbox>(config_.depth());
            }
            auto inbox = inbox_;
            session_.emplace(::zenoh::Session::open(config_from_environment()));
            publisher_.emplace(session_->declare_publisher(::zenoh::KeyExpr(key_expr_)));
            subscriber_.emplace(session_->declare_subscriber(
                ::zenoh::KeyExpr(key_expr_),
                [inbox](::zenoh::Sample &sample) {
                    if (inbox) {
                        inbox->push(sample);
                    }
                },
                []() {}));
            return true;
        } catch (...) {
            subscriber_.reset();
            publisher_.reset();
            session_.reset();
            return false;
        }
    }

    std::optional<std::vector<std::uint8_t>> encode_transport_frame(
        const T &value, std::uint64_t published_at_ms) noexcept {
        try {
            std::vector<std::uint8_t> frame(timestamp_wire_size() +
                                            detail::encoded_frame_size(value));
            auto output = std::span<std::uint8_t>{frame};
            write_wire_le(output, 0, published_at_ms);
            detail::encode_frame(value, output.subspan(timestamp_wire_size()));
            return frame;
        } catch (...) {
            return std::nullopt;
        }
    }

    bool publish_frame(std::vector<std::uint8_t> frame) noexcept {
        if (!publisher_) {
            mark_transport_error("Zenoh publisher is not ready");
            return false;
        }
        try {
            publisher_->put(::zenoh::Bytes(std::move(frame)));
            return true;
        } catch (const std::exception &error) {
            mark_transport_error(error.what());
            return false;
        } catch (...) {
            mark_transport_error("publish Zenoh sample failed");
            return false;
        }
    }

    bool decode_frame(const std::vector<std::uint8_t> &frame) {
        if (frame.size() < timestamp_wire_size()) {
            return false;
        }

        try {
            const auto input = std::span<const std::uint8_t>{frame};
            const auto published_at_ms = read_wire_le<std::uint64_t>(input, 0);
            auto decoded = detail::decode_frame<T>(input.subspan(timestamp_wire_size()));
            published_at_ms_ = published_at_ms;
            received_ = std::move(decoded);
            ++revision_;
            return true;
        } catch (...) {
            return false;
        }
    }

    static ::zenoh::Config config_from_environment() {
        auto config = ::zenoh::Config::create_default();
        if (const auto *mode = std::getenv("FLOWRT_ZENOH_MODE")) {
            config.insert_json5(Z_CONFIG_MODE_KEY, json_string(std::string_view{mode}));
        }
        if (const auto *listen = std::getenv("FLOWRT_ZENOH_LISTEN")) {
            if (const auto json = endpoint_list_json(std::string_view{listen}); !json.empty()) {
                config.insert_json5(Z_CONFIG_LISTEN_KEY, json);
            }
        }
        if (const auto *connect = std::getenv("FLOWRT_ZENOH_CONNECT")) {
            if (const auto json = endpoint_list_json(std::string_view{connect}); !json.empty()) {
                config.insert_json5(Z_CONFIG_CONNECT_KEY, json);
            }
        }
        if (const auto *no_multicast = std::getenv("FLOWRT_ZENOH_NO_MULTICAST");
            env_flag_enabled(no_multicast)) {
            config.insert_json5(Z_CONFIG_MULTICAST_SCOUTING_KEY, "false");
        }
        return config;
    }

    static bool env_flag_enabled(const char *value) noexcept {
        if (value == nullptr) {
            return false;
        }
        const auto flag = std::string_view{value};
        return flag == "1" || flag == "true" || flag == "TRUE" || flag == "yes" || flag == "on";
    }

    static std::vector<std::string> endpoint_list_items(std::string_view raw) {
        std::vector<std::string> endpoints;
        std::size_t start = 0;
        while (start <= raw.size()) {
            const auto comma = raw.find(',', start);
            const auto end = comma == std::string_view::npos ? raw.size() : comma;
            auto item = raw.substr(start, end - start);
            while (!item.empty() && (item.front() == ' ' || item.front() == '\t')) {
                item.remove_prefix(1);
            }
            while (!item.empty() && (item.back() == ' ' || item.back() == '\t')) {
                item.remove_suffix(1);
            }
            if (!item.empty()) {
                endpoints.emplace_back(item);
            }
            if (comma == std::string_view::npos) {
                break;
            }
            start = comma + 1;
        }
        return endpoints;
    }

    static std::string endpoint_list_json(std::string_view raw) {
        const auto endpoints = endpoint_list_items(raw);
        if (endpoints.empty()) {
            return {};
        }

        std::string json = "[";
        for (std::size_t index = 0; index < endpoints.size(); ++index) {
            if (index != 0U) {
                json += ",";
            }
            json += json_string(endpoints[index]);
        }
        json += "]";
        return json;
    }

    static std::string json_string(std::string_view value) {
        std::string output = "\"";
        for (const char ch : value) {
            switch (ch) {
                case '\\':
                    output += "\\\\";
                    break;
                case '"':
                    output += "\\\"";
                    break;
                case '\n':
                    output += "\\n";
                    break;
                case '\r':
                    output += "\\r";
                    break;
                case '\t':
                    output += "\\t";
                    break;
                default:
                    output += ch;
                    break;
            }
        }
        output += "\"";
        return output;
    }
#endif

    std::string key_expr_;
    ZenohChannelConfig config_;
    BackendHealthTracker health_;
    std::uint64_t revision_ = 0;
#ifdef FLOWRT_HAS_ZENOH_CXX
    std::optional<::zenoh::Session> session_;
    std::optional<::zenoh::Publisher> publisher_;
    std::optional<ZenohSubscriber> subscriber_;
    std::shared_ptr<ZenohInbox> inbox_;
    std::optional<T> received_;
    std::optional<std::uint64_t> published_at_ms_;
#endif
};

/**
 * @brief Zenoh service client。
 *
 * 使用 zenoh query 实现 request/response 语义。client 通过 shared_ptr 共享 session。
 */
template <typename Req, typename Resp>
class ZenohServiceClient {
   public:
    ZenohServiceClient(ZenohServiceClient &&) noexcept = default;
    ZenohServiceClient(const ZenohServiceClient &) = delete;
    auto operator=(ZenohServiceClient &&) noexcept -> ZenohServiceClient & = default;
    auto operator=(const ZenohServiceClient &) -> ZenohServiceClient & = delete;
    ~ZenohServiceClient() = default;

    /**
     * @brief 使用已有 session 打开 zenoh service client。
     */
    static ZenohServiceClient open(std::string_view service_name
#ifdef FLOWRT_HAS_ZENOH_CXX
                                   ,
                                   std::shared_ptr<::zenoh::Session> session
#endif
    ) {
        return ZenohServiceClient(service_name
#ifdef FLOWRT_HAS_ZENOH_CXX
                                  ,
                                  std::move(session)
#endif
        );
    }

    /**
     * @brief 发送请求并等待响应。
     */
    ServiceResult<Resp> call(const Req &request, std::uint64_t timeout_ms) {
#ifndef FLOWRT_HAS_ZENOH_CXX
        (void)request;
        (void)timeout_ms;
        return ServiceResult<Resp>::err(ServiceError::Backend);
#else
        if (!session_ || session_->is_closed()) {
            return ServiceResult<Resp>::err(ServiceError::Unavailable);
        }

        const auto now_ms = unix_now_ms();
        const auto deadline = Deadline::make(timeout_ms, now_ms);
        if (!deadline.has_value()) {
            return ServiceResult<Resp>::err(ServiceError::Timeout);
        }

        const auto sequence = sequence_++;
        const RequestId request_id{session_id_, sequence, service_id_};

        std::vector<std::uint8_t> payload;
        try {
            payload.resize(detail::encoded_frame_size(request));
            detail::encode_frame(request, std::span<std::uint8_t>{payload});
        } catch (...) {
            return ServiceResult<Resp>::err_with_message(ServiceError::Protocol,
                                                         "encode request payload failed");
        }

        constexpr std::uint64_t correlation_id = 0;
        constexpr std::uint64_t schema_hash = 0;
        const auto header =
            ServiceFrameHeader::make_request(request_id, *deadline, correlation_id, schema_hash);
        const auto frame = encode_service_frame(header, payload, {});

        try {
            struct ReplyState {
                std::mutex mutex;
                std::condition_variable cv;
                std::optional<::zenoh::Reply> reply;
                bool done = false;
            };

            auto state = std::make_shared<ReplyState>();
            auto on_reply = [state](::zenoh::Reply &reply) {
                {
                    std::lock_guard<std::mutex> lock(state->mutex);
                    if (!state->reply.has_value()) {
                        state->reply = std::move(reply);
                    }
                    state->done = true;
                }
                state->cv.notify_one();
            };
            auto on_drop = [state]() {
                {
                    std::lock_guard<std::mutex> lock(state->mutex);
                    state->done = true;
                }
                state->cv.notify_one();
            };
            const auto transport_timeout_ms =
                timeout_ms > std::numeric_limits<std::uint64_t>::max() -
                                 SERVICE_TRANSPORT_TIMEOUT_GRACE_MS
                    ? std::numeric_limits<std::uint64_t>::max()
                    : timeout_ms + SERVICE_TRANSPORT_TIMEOUT_GRACE_MS;
            auto opts = ::zenoh::Session::GetOptions::create_default();
            opts.timeout_ms = transport_timeout_ms;
            opts.payload = ::zenoh::Bytes(std::vector<std::uint8_t>(frame));
            session_->get(::zenoh::KeyExpr(key_expr_), "", std::move(on_reply), std::move(on_drop),
                          std::move(opts));

            // 等待回调触发或超时
            const auto wait_deadline =
                std::chrono::steady_clock::now() + std::chrono::milliseconds(timeout_ms);
            std::optional<::zenoh::Reply> reply_holder;
            {
                std::unique_lock<std::mutex> lock(state->mutex);
                if (!state->cv.wait_until(lock, wait_deadline,
                                          [&state]() { return state->done; })) {
                    return ServiceResult<Resp>::err(ServiceError::Timeout);
                }
                if (state->reply.has_value()) {
                    reply_holder = std::move(state->reply);
                }
            }

            if (!reply_holder.has_value()) {
                return ServiceResult<Resp>::err(ServiceError::Timeout);
            }
            auto &reply = *reply_holder;

            if (!reply.is_ok()) {
                return ServiceResult<Resp>::err(ServiceError::Timeout);
            }
            auto &sample = reply.get_ok();

            const auto reply_payload = sample.get_payload().as_vector();
            const auto decoded = decode_service_frame(reply_payload);
            const auto &resp_header = decoded.header;

            if (resp_header.service_id != request_id.service_id ||
                resp_header.session_id != request_id.session_id ||
                resp_header.sequence != request_id.sequence) {
                return ServiceResult<Resp>::err_with_message(ServiceError::Protocol,
                                                             "response request id mismatch");
            }
            if (resp_header.correlation_id != correlation_id) {
                return ServiceResult<Resp>::err_with_message(ServiceError::Protocol,
                                                             "response correlation id mismatch");
            }
            if (resp_header.schema_hash != schema_hash) {
                return ServiceResult<Resp>::err_with_message(ServiceError::Protocol,
                                                             "response schema hash mismatch");
            }

            if (resp_header.error_code != static_cast<std::uint16_t>(ServiceError::Ok)) {
                return service_result_from_response_error_code<Resp>(
                    resp_header.error_code,
                    std::string_view(reinterpret_cast<const char *>(decoded.error_msg.data()),
                                     decoded.error_msg.size()));
            }

            auto resp = detail::decode_frame<Resp>(decoded.payload);
            return ServiceResult<Resp>::ok(std::move(resp));
        } catch (const WireCodecError &e) {
            return ServiceResult<Resp>::err_with_message(ServiceError::Protocol, e.what());
        } catch (const std::exception &e) {
            return ServiceResult<Resp>::err_with_message(ServiceError::Backend, e.what());
        } catch (...) {
            return ServiceResult<Resp>::err_with_message(ServiceError::Backend,
                                                         "zenoh query failed");
        }
#endif
    }

    /**
     * @brief 返回 service 名称。
     */
    std::string_view service_name() const noexcept { return service_name_; }

    /**
     * @brief 判断 client 是否已绑定到底层 zenoh transport 资源。
     */
    bool ready() const noexcept {
#ifdef FLOWRT_HAS_ZENOH_CXX
        return session_ && !session_->is_closed();
#else
        return false;
#endif
    }

   private:
    ZenohServiceClient(std::string_view service_name
#ifdef FLOWRT_HAS_ZENOH_CXX
                       ,
                       std::shared_ptr<::zenoh::Session> session
#endif
                       )
        : service_name_(service_name),
          service_id_(fnv1a64(service_name)),
          key_expr_(service_key_expr(service_name)),
          session_id_(reinterpret_cast<std::uint64_t>(this))
#ifdef FLOWRT_HAS_ZENOH_CXX
          ,
          session_(std::move(session))
#endif
    {
    }

    std::string service_name_;
    std::uint64_t service_id_;
    std::string key_expr_;
    std::uint64_t session_id_;
    std::uint64_t sequence_ = 0;
#ifdef FLOWRT_HAS_ZENOH_CXX
    std::shared_ptr<::zenoh::Session> session_;
#endif
};

/**
 * @brief Zenoh service server。
 *
 * 使用 zenoh queryable 实现 request/response 语义。server 不持有 session 所有权，
 * session 生命周期由调用方管理。
 */
template <typename Req, typename Resp>
class ZenohServiceServer {
   public:
    using Handler = std::function<ServiceResult<Resp>(const Req &)>;

    ZenohServiceServer(ZenohServiceServer &&) noexcept = default;
    ZenohServiceServer(const ZenohServiceServer &) = delete;
    auto operator=(ZenohServiceServer &&) noexcept -> ZenohServiceServer & = default;
    auto operator=(const ZenohServiceServer &) -> ZenohServiceServer & = delete;
    ~ZenohServiceServer() = default;

    /**
     * @brief 使用已有 session 打开 zenoh service server。
     */
    static ZenohServiceServer open(std::string_view service_name
#ifdef FLOWRT_HAS_ZENOH_CXX
                                   ,
                                   std::shared_ptr<::zenoh::Session> session
#endif
                                   ,
                                   Handler handler) {
        return open_with_config(service_name
#ifdef FLOWRT_HAS_ZENOH_CXX
                                ,
                                std::move(session)
#endif
                                    ,
                                ZenohServiceConfig::defaults(), std::move(handler));
    }

    /**
     * @brief 使用显式配置打开 zenoh service server。
     */
    static ZenohServiceServer open_with_config(std::string_view service_name
#ifdef FLOWRT_HAS_ZENOH_CXX
                                               ,
                                               std::shared_ptr<::zenoh::Session> session
#endif
                                               ,
                                               ZenohServiceConfig config, Handler handler) {
        return ZenohServiceServer(service_name
#ifdef FLOWRT_HAS_ZENOH_CXX
                                  ,
                                  std::move(session)
#endif
                                      ,
                                  config, std::move(handler));
    }

    /**
     * @brief 返回 service 名称。
     */
    std::string_view service_name() const noexcept { return service_name_; }

    /**
     * @brief 判断 server 是否已绑定到底层 zenoh transport 资源。
     */
    bool ready() const noexcept {
#ifdef FLOWRT_HAS_ZENOH_CXX
        return queryable_.has_value();
#else
        return false;
#endif
    }

   private:
    ZenohServiceServer(std::string_view service_name
#ifdef FLOWRT_HAS_ZENOH_CXX
                       ,
                       std::shared_ptr<::zenoh::Session> session
#endif
                       ,
                       ZenohServiceConfig config, Handler handler)
        : service_name_(service_name),
          service_id_(fnv1a64(service_name)),
          key_expr_(service_key_expr(service_name)),
          config_(config),
          handler_(std::move(handler)) {
#ifdef FLOWRT_HAS_ZENOH_CXX
        auto handler_fn = handler_;
        auto service_id = service_id_;
        auto ke = key_expr_;
        auto max_in_flight = config_.max_in_flight();
        auto in_flight = in_flight_;

        queryable_.emplace(session->declare_queryable(
            ::zenoh::KeyExpr(ke),
            [handler_fn, service_id, ke, max_in_flight, in_flight](::zenoh::Query &query) {
                const auto reply_ke = ::zenoh::KeyExpr(ke);

                std::vector<std::uint8_t> payload;
                auto payload_ref = query.get_payload();
                if (payload_ref.has_value()) {
                    auto bytes = payload_ref->get().as_vector();
                    payload = std::move(bytes);
                } else {
                    const auto header = ServiceFrameHeader::make_response(
                        RequestId{0, 0, service_id},
                        Deadline::make(1000, unix_now_ms()).value_or(Deadline{1000, 0}), 0, 0,
                        ServiceError::Protocol);
                    const auto frame = encode_service_frame(header, {}, {});
                    query.reply(reply_ke, ::zenoh::Bytes(std::vector<std::uint8_t>(frame)));
                    return;
                }

                DecodedServiceFrame decoded;
                try {
                    decoded = decode_service_frame(payload);
                } catch (...) {
                    const auto header = ServiceFrameHeader::make_response(
                        RequestId{0, 0, service_id},
                        Deadline::make(1000, unix_now_ms()).value_or(Deadline{1000, 0}), 0, 0,
                        ServiceError::Protocol);
                    const auto frame = encode_service_frame(header, {}, {});
                    query.reply(reply_ke, ::zenoh::Bytes(std::vector<std::uint8_t>(frame)));
                    return;
                }

                const auto &req_header = decoded.header;
                const auto now_ms = unix_now_ms();
                const Deadline deadline{req_header.timeout_ms, req_header.absolute_deadline_ms};
                if (req_header.service_id != service_id) {
                    const auto header = ServiceFrameHeader::make_response(
                        RequestId{req_header.session_id, req_header.sequence, service_id}, deadline,
                        req_header.correlation_id, req_header.schema_hash, ServiceError::Protocol);
                    const auto message = std::string_view{"request service id mismatch"};
                    const auto frame = encode_service_frame(
                        header, {},
                        std::span<const std::uint8_t>{
                            reinterpret_cast<const std::uint8_t *>(message.data()),
                            message.size()});
                    query.reply(reply_ke, ::zenoh::Bytes(std::vector<std::uint8_t>(frame)));
                    return;
                }
                if (deadline.expired(now_ms)) {
                    const auto header = ServiceFrameHeader::make_response(
                        RequestId{req_header.session_id, req_header.sequence, service_id}, deadline,
                        req_header.correlation_id, req_header.schema_hash, ServiceError::Timeout);
                    const auto frame = encode_service_frame(header, {}, {});
                    query.reply(reply_ke, ::zenoh::Bytes(std::vector<std::uint8_t>(frame)));
                    return;
                }

                Req request;
                try {
                    request = detail::decode_frame<Req>(decoded.payload);
                } catch (...) {
                    const auto header = ServiceFrameHeader::make_response(
                        RequestId{req_header.session_id, req_header.sequence, service_id}, deadline,
                        req_header.correlation_id, req_header.schema_hash, ServiceError::Protocol);
                    const auto frame = encode_service_frame(header, {}, {});
                    query.reply(reply_ke, ::zenoh::Bytes(std::vector<std::uint8_t>(frame)));
                    return;
                }

                const auto handler_wait_now_ms = unix_now_ms();
                const auto remaining_ms = deadline.absolute_deadline_ms > handler_wait_now_ms
                                              ? deadline.absolute_deadline_ms - handler_wait_now_ms
                                              : 0U;
                auto reply_error = [&query, &reply_ke, &req_header, &deadline, service_id](
                                       ServiceError error, std::string_view message) {
                    const auto header = ServiceFrameHeader::make_response(
                        RequestId{req_header.session_id, req_header.sequence, service_id}, deadline,
                        req_header.correlation_id, req_header.schema_hash, error);
                    const auto frame = encode_service_frame(
                        header, {},
                        std::span<const std::uint8_t>{
                            reinterpret_cast<const std::uint8_t *>(message.data()),
                            message.size()});
                    query.reply(reply_ke, ::zenoh::Bytes(std::vector<std::uint8_t>(frame)));
                };

                const auto previous_in_flight = in_flight->fetch_add(1, std::memory_order_acq_rel);
                if (previous_in_flight >= max_in_flight) {
                    in_flight->fetch_sub(1, std::memory_order_acq_rel);
                    reply_error(ServiceError::Busy, "zenoh service handler limit reached");
                    return;
                }

                auto result_promise = std::make_shared<std::promise<ServiceResult<Resp>>>();
                auto result_future = result_promise->get_future();
                auto release_in_flight = std::shared_ptr<void>(nullptr, [in_flight](void *) {
                    in_flight->fetch_sub(1, std::memory_order_acq_rel);
                });
                try {
                    std::thread([handler_fn, request, result_promise,
                                 release_in_flight = std::move(release_in_flight)]() mutable {
                        (void)release_in_flight;
                        try {
                            result_promise->set_value(handler_fn(request));
                        } catch (const std::exception &e) {
                            result_promise->set_value(ServiceResult<Resp>::err_with_message(
                                ServiceError::HandlerError, e.what()));
                        } catch (...) {
                            result_promise->set_value(ServiceResult<Resp>::err_with_message(
                                ServiceError::HandlerError, "service handler threw"));
                        }
                    }).detach();
                } catch (const std::exception &e) {
                    reply_error(ServiceError::Backend, e.what());
                    return;
                } catch (...) {
                    reply_error(ServiceError::Backend, "failed to spawn zenoh service handler");
                    return;
                }

                if (result_future.wait_for(std::chrono::milliseconds{remaining_ms}) !=
                    std::future_status::ready) {
                    const auto header = ServiceFrameHeader::make_response(
                        RequestId{req_header.session_id, req_header.sequence, service_id}, deadline,
                        req_header.correlation_id, req_header.schema_hash, ServiceError::Timeout);
                    const auto message =
                        std::string_view{"request deadline expired while handler was running"};
                    const auto frame = encode_service_frame(
                        header, {},
                        std::span<const std::uint8_t>{
                            reinterpret_cast<const std::uint8_t *>(message.data()),
                            message.size()});
                    query.reply(reply_ke, ::zenoh::Bytes(std::vector<std::uint8_t>(frame)));
                    return;
                }

                auto result = result_future.get();
                ServiceError error_code = ServiceError::Ok;
                std::vector<std::uint8_t> response_payload;
                std::vector<std::uint8_t> error_msg;

                if (result.is_ok()) {
                    const auto *value = result.value();
                    if (value) {
                        response_payload.resize(detail::encoded_frame_size(*value));
                        detail::encode_frame(*value, std::span<std::uint8_t>{response_payload});
                    }
                } else {
                    error_code = result.error_code();
                    const auto &msg = result.error_message();
                    if (msg.has_value()) {
                        error_msg.assign(msg->begin(), msg->end());
                    }
                }

                const auto header = ServiceFrameHeader::make_response(
                    RequestId{req_header.session_id, req_header.sequence, service_id}, deadline,
                    req_header.correlation_id, req_header.schema_hash, error_code);
                const auto frame = encode_service_frame(header, response_payload, error_msg);
                query.reply(reply_ke, ::zenoh::Bytes(std::vector<std::uint8_t>(frame)));
            },
            []() {}));
#endif
    }

    std::string service_name_;
    std::uint64_t service_id_;
    std::string key_expr_;
    ZenohServiceConfig config_;
    Handler handler_;
#ifdef FLOWRT_HAS_ZENOH_CXX
    std::shared_ptr<std::atomic_size_t> in_flight_ = std::make_shared<std::atomic_size_t>(0);
    std::optional<::zenoh::Queryable<void>> queryable_;
#endif
};

#ifdef FLOWRT_HAS_ZENOH_CXX
/**
 * @brief 从 FLOWRT_ZENOH_* 环境变量构建 zenoh session 配置。
 *
 * 读取 FLOWRT_ZENOH_MODE、FLOWRT_ZENOH_LISTEN、FLOWRT_ZENOH_CONNECT、
 * FLOWRT_ZENOH_NO_MULTICAST 环境变量，与 Rust 端 config_from_environment 对齐。
 */
inline ::zenoh::Config zenoh_config_from_environment() {
    auto config = ::zenoh::Config::create_default();
    if (const auto *mode = std::getenv("FLOWRT_ZENOH_MODE")) {
        config.insert_json5(Z_CONFIG_MODE_KEY, json_string(std::string_view{mode}));
    }
    if (const auto *listen = std::getenv("FLOWRT_ZENOH_LISTEN")) {
        if (const auto json = endpoint_list_json(std::string_view{listen}); !json.empty()) {
            config.insert_json5(Z_CONFIG_LISTEN_KEY, json);
        }
    }
    if (const auto *connect = std::getenv("FLOWRT_ZENOH_CONNECT")) {
        if (const auto json = endpoint_list_json(std::string_view{connect}); !json.empty()) {
            config.insert_json5(Z_CONFIG_CONNECT_KEY, json);
        }
    }
    if (const auto *no_mc = std::getenv("FLOWRT_ZENOH_NO_MULTICAST"); env_flag_enabled(no_mc)) {
        config.insert_json5(Z_CONFIG_MULTICAST_SCOUTING_KEY, "false");
    }
    return config;
}

/**
 * @brief 从环境变量打开 zenoh session。
 */
inline ::zenoh::Session open_zenoh_session_from_env() {
    return ::zenoh::Session::open(zenoh_config_from_environment());
}
#endif

inline std::string operation_response_json(const IntrospectionHandshake &handshake,
                                           const IntrospectionState &state,
                                           std::string_view payload) {
    const auto request = flowrt::detail::parse_introspection_request(payload);
    if (!request) {
        return flowrt::detail::error_response_json(handshake,
                                                   "invalid zenoh operation request JSON");
    }

    switch (request->kind) {
        case flowrt::detail::IntrospectionRequestKind::Status:
            return flowrt::detail::status_response_json(handshake, state.status());
        case flowrt::detail::IntrospectionRequestKind::OperationStart: {
            const auto result =
                state.start_operation(request->operation_name, request->operation_payload,
                                      request->operation_timeout_ms, request->operation_owner);
            if (std::holds_alternative<IntrospectionOperationStartStatus>(result)) {
                return flowrt::detail::operation_started_response_json(
                    handshake, std::get<IntrospectionOperationStartStatus>(result));
            }
            return flowrt::detail::error_response_json(handshake, std::get<std::string>(result));
        }
        case flowrt::detail::IntrospectionRequestKind::OperationStatus: {
            const auto result = state.status_operation(request->operation_id);
            if (std::holds_alternative<IntrospectionOperationStatus>(result)) {
                return flowrt::detail::operation_value_response_json(
                    handshake, std::get<IntrospectionOperationStatus>(result));
            }
            return flowrt::detail::error_response_json(handshake, std::get<std::string>(result));
        }
        case flowrt::detail::IntrospectionRequestKind::OperationCancel: {
            const auto result = state.cancel_operation(request->operation_id);
            if (std::holds_alternative<IntrospectionOperationStatus>(result)) {
                return flowrt::detail::operation_value_response_json(
                    handshake, std::get<IntrospectionOperationStatus>(result));
            }
            return flowrt::detail::error_response_json(handshake, std::get<std::string>(result));
        }
        case flowrt::detail::IntrospectionRequestKind::OperationResult: {
            const auto result = state.result_operation(request->operation_id);
            if (std::holds_alternative<IntrospectionOperationResult>(result)) {
                return flowrt::detail::operation_result_response_json(
                    handshake, std::get<IntrospectionOperationResult>(result));
            }
            return flowrt::detail::error_response_json(handshake, std::get<std::string>(result));
        }
        case flowrt::detail::IntrospectionRequestKind::OperationObserve: {
            const auto result = state.observe_operation(
                request->operation_id, request->operation_after_sequence, request->operation_limit);
            if (std::holds_alternative<IntrospectionState::OperationObservePage>(result)) {
                const auto &page = std::get<IntrospectionState::OperationObservePage>(result);
                return flowrt::detail::operation_events_response_json(
                    handshake, request->operation_id, page.events, page.next_sequence,
                    page.terminal);
            }
            return flowrt::detail::error_response_json(handshake, std::get<std::string>(result));
        }
        default:
            return flowrt::detail::error_response_json(handshake,
                                                       "unsupported zenoh operation command");
    }
}

/**
 * @brief Zenoh remote Operation control-plane server。
 *
 * 该 server 只暴露 Operation introspection command-plane，不承载 typed Operation data plane。
 * 未编译 zenoh-cpp 时对象仍可构造，`ready()` 返回 false，generated inproc/iox2 app 不因此失败。
 */
class ZenohOperationServer {
   public:
    ZenohOperationServer(ZenohOperationServer &&) noexcept = default;
    ZenohOperationServer(const ZenohOperationServer &) = delete;
    auto operator=(ZenohOperationServer &&) noexcept -> ZenohOperationServer & = default;
    auto operator=(const ZenohOperationServer &) -> ZenohOperationServer & = delete;
    ~ZenohOperationServer() = default;

    static ZenohOperationServer open_from_environment(std::string_view key_expr,
                                                      IntrospectionHandshake handshake,
                                                      IntrospectionState state) {
        return ZenohOperationServer(key_expr, std::move(handshake), std::move(state));
    }

    std::string_view key_expr() const noexcept { return key_expr_; }

    bool ready() const noexcept {
#ifdef FLOWRT_HAS_ZENOH_CXX
        return session_.has_value() && queryable_.has_value() && !session_->is_closed();
#else
        return false;
#endif
    }

   private:
    ZenohOperationServer(std::string_view key_expr, IntrospectionHandshake handshake,
                         IntrospectionState state)
        : key_expr_(key_expr) {
#ifdef FLOWRT_HAS_ZENOH_CXX
        try {
            session_.emplace(open_zenoh_session_from_env());
            auto shared_handshake = std::make_shared<IntrospectionHandshake>(std::move(handshake));
            auto shared_state = std::make_shared<IntrospectionState>(std::move(state));
            auto reply_key = key_expr_;
            queryable_.emplace(session_->declare_queryable(
                ::zenoh::KeyExpr(key_expr_),
                [shared_handshake, shared_state, reply_key](::zenoh::Query &query) {
                    std::string response;
                    auto payload_ref = query.get_payload();
                    if (!payload_ref.has_value()) {
                        response = flowrt::detail::error_response_json(
                            *shared_handshake, "empty zenoh operation request payload");
                    } else {
                        const auto payload = payload_ref->get().as_vector();
                        response = operation_response_json(
                            *shared_handshake, *shared_state,
                            std::string_view{reinterpret_cast<const char *>(payload.data()),
                                             payload.size()});
                    }
                    query.reply(::zenoh::KeyExpr(reply_key),
                                ::zenoh::Bytes(
                                    std::vector<std::uint8_t>(response.begin(), response.end())));
                },
                []() {}));
        } catch (...) {
            queryable_.reset();
            session_.reset();
        }
#else
        (void)handshake;
        (void)state;
#endif
    }

    std::string key_expr_;
#ifdef FLOWRT_HAS_ZENOH_CXX
    std::optional<::zenoh::Session> session_;
    std::optional<::zenoh::Queryable<void>> queryable_;
#endif
};

}  // namespace zenoh

}  // namespace flowrt
