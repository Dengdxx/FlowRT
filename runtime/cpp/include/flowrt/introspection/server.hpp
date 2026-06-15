#pragma once

#include <atomic>
#include <cerrno>
#include <chrono>
#include <cstdio>
#include <fcntl.h>
#include <filesystem>
#include <flowrt/introspection/json.hpp>
#include <flowrt/introspection/request_parser.hpp>
#include <flowrt/introspection/socket.hpp>
#include <flowrt/introspection/state.hpp>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <sys/socket.h>
#include <sys/un.h>
#include <system_error>
#include <thread>
#include <unistd.h>
#include <utility>

namespace flowrt {

namespace detail {

inline void handle_introspection_connection(
    int client_fd, const IntrospectionHandshake &handshake, const IntrospectionState &state,
    const std::shared_ptr<std::atomic_bool> &stop,
    std::optional<IntrospectionClientPermit> initial_permit,
    const std::shared_ptr<std::atomic_size_t> &active_observers) {
    const auto line = read_line(client_fd);
    std::string response;
    if (!line) {
        response = error_response_json(handshake, "invalid FlowRT introspection request");
    } else if (const auto request = parse_introspection_request(*line)) {
        initial_permit.reset();
        switch (request->kind) {
            case IntrospectionRequestKind::Status: {
                const auto status = state.status();
                response = status_response_json(handshake, status);
                break;
            }
            case IntrospectionRequestKind::SelfDescription: {
                const auto json = state.self_description_json();
                response = json ? self_description_response_json(handshake, *json)
                                : error_response_json(handshake,
                                                      "FlowRT self-description is not registered");
                break;
            }
            case IntrospectionRequestKind::ChannelSnapshot: {
                const auto channel = state.channel_snapshot(request->channel);
                response = channel ? channel_snapshot_response_json(handshake, *channel)
                                   : error_response_json(handshake, "unknown FlowRT channel");
                break;
            }
            case IntrospectionRequestKind::ObserveChannel: {
                auto observer_permit = try_acquire_introspection_client_permit(
                    active_observers, MAX_INTROSPECTION_OBSERVERS);
                if (!observer_permit.has_value()) {
                    response = error_response_json(
                        handshake, "FlowRT introspection observe connection limit reached");
                    break;
                }
                auto guard = state.observe_channel(request->channel);
                const auto channel = state.channel_status(request->channel);
                if (!guard || !channel) {
                    response = error_response_json(handshake, "unknown FlowRT channel");
                    break;
                }
                response = observe_ready_response_json(handshake, *channel);
                response.push_back('\n');
                (void)write_all(client_fd, response);
                while (!stop->load(std::memory_order_relaxed)) {
                    const auto keepalive = read_line_result(client_fd);
                    switch (keepalive.status) {
                        case ReadLineStatus::Line:
                        case ReadLineStatus::Timeout:
                            continue;
                        case ReadLineStatus::Closed:
                        case ReadLineStatus::Error:
                            return;
                    }
                }
                return;
            }
            case IntrospectionRequestKind::ParamList:
                response = param_list_response_json(handshake, state.params());
                break;
            case IntrospectionRequestKind::ParamGet: {
                const auto param = state.param(request->param_name);
                response = param ? param_value_response_json(handshake, *param)
                                 : error_response_json(handshake, "unknown FlowRT parameter `" +
                                                                      request->param_name + "`");
                break;
            }
            case IntrospectionRequestKind::ParamSet: {
                const auto result =
                    state.set_param_pending(request->param_name, request->param_value);
                if (std::holds_alternative<IntrospectionParamStatus>(result)) {
                    response = param_value_response_json(
                        handshake, std::get<IntrospectionParamStatus>(result));
                } else {
                    response = error_response_json(handshake, std::get<std::string>(result));
                }
                break;
            }
            case IntrospectionRequestKind::BoundaryPublish: {
                const auto result = state.publish_boundary_input(
                    request->boundary_endpoint,
                    std::span<const std::uint8_t>{request->boundary_payload.data(),
                                                  request->boundary_payload.size()},
                    request->boundary_published_at_ms);
                if (std::holds_alternative<IntrospectionBoundaryPublishStatus>(result)) {
                    response = boundary_publish_response_json(
                        handshake, std::get<IntrospectionBoundaryPublishStatus>(result));
                } else {
                    response = error_response_json(handshake, std::get<std::string>(result));
                }
                break;
            }
            case IntrospectionRequestKind::OperationCancel: {
                const auto result = state.cancel_operation(request->operation_id);
                if (std::holds_alternative<IntrospectionOperationStatus>(result)) {
                    response = operation_value_response_json(
                        handshake, std::get<IntrospectionOperationStatus>(result));
                } else {
                    response = error_response_json(handshake, std::get<std::string>(result));
                }
                break;
            }
            case IntrospectionRequestKind::RecorderStart: {
                const auto recorder = state.start_recorder(IntrospectionRecorderStart{
                    .output = request->recorder_output,
                    .filters = request->recorder_filters,
                    .queue_depth = request->recorder_queue_depth.value_or(1024U),
                    .package = handshake.package,
                    .process = handshake.process,
                    .runtime_pid = handshake.pid,
                    .self_description_hash = handshake.self_description_hash,
                });
                state.record_current_diagnostics();
                response = recorder_value_response_json(handshake, recorder);
                break;
            }
            case IntrospectionRequestKind::RecorderStop:
                response = recorder_value_response_json(handshake, state.stop_recorder());
                break;
            case IntrospectionRequestKind::RecorderDrain: {
                const auto events = state.drain_recorder_events();
                response =
                    recorder_events_response_json(handshake, state.status().recorder, events);
                break;
            }
        }
    } else {
        response = error_response_json(handshake, "invalid FlowRT introspection request");
    }
    response.push_back('\n');
    (void)write_all(client_fd, response);
}

}  // namespace detail

/**
 * @brief 已启动的 introspection 服务。
 *
 * 该对象拥有 Unix socket listener 线程，并在析构时停止 listener、删除 socket 文件。
 */
class IntrospectionServer {
   public:
    IntrospectionServer() = default;
    IntrospectionServer(const IntrospectionServer &) = delete;
    auto operator=(const IntrospectionServer &) -> IntrospectionServer & = delete;

    IntrospectionServer(IntrospectionServer &&other) noexcept
        : path_(std::move(other.path_)),
          handle_(std::move(other.handle_)),
          stop_(std::move(other.stop_)) {
        other.path_.clear();
    }

    auto operator=(IntrospectionServer &&other) noexcept -> IntrospectionServer & {
        if (this != std::addressof(other)) {
            stop();
            path_ = std::move(other.path_);
            handle_ = std::move(other.handle_);
            stop_ = std::move(other.stop_);
            other.path_.clear();
        }
        return *this;
    }

    ~IntrospectionServer() { stop(); }

    /**
     * @brief 返回服务 socket 路径。
     */
    const std::filesystem::path &path() const noexcept { return path_; }

   private:
    friend std::optional<IntrospectionServer> spawn_status_server_at(
        std::filesystem::path path, IntrospectionHandshake handshake, IntrospectionState state);

    IntrospectionServer(std::filesystem::path path, std::thread handle,
                        std::shared_ptr<std::atomic_bool> stop)
        : path_(std::move(path)), handle_(std::move(handle)), stop_(std::move(stop)) {}

    void stop() noexcept {
        if (stop_) {
            stop_->store(true, std::memory_order_relaxed);
        }
        if (!path_.empty()) {
            std::error_code ignored;
            std::filesystem::remove(path_, ignored);
        }
        if (handle_.joinable()) {
            handle_.join();
        }
        stop_.reset();
        path_.clear();
    }

    std::filesystem::path path_;
    std::thread handle_;
    std::shared_ptr<std::atomic_bool> stop_;
};

/**
 * @brief 在指定路径启动最小 introspection status 服务，主要用于测试和后续 generated shell 接入。
 */
inline std::optional<IntrospectionServer> spawn_status_server_at(std::filesystem::path path,
                                                                 IntrospectionHandshake handshake,
                                                                 IntrospectionState state) {
    std::error_code filesystem_error;
    if (const auto parent = path.parent_path(); !parent.empty()) {
        std::filesystem::create_directories(parent, filesystem_error);
        if (filesystem_error) {
            return std::nullopt;
        }
    }
    if (std::filesystem::exists(path, filesystem_error)) {
        if (detail::unix_socket_accepts_connection(path)) {
            return std::nullopt;
        }
        filesystem_error.clear();
        std::filesystem::remove(path, filesystem_error);
        if (filesystem_error) {
            return std::nullopt;
        }
    }

    const int listener_fd = ::socket(AF_UNIX, SOCK_STREAM, 0);
    if (listener_fd < 0) {
        return std::nullopt;
    }

    auto close_listener = [listener_fd]() { ::close(listener_fd); };
    sockaddr_un address{};
    address.sun_family = AF_UNIX;
    const auto path_string = path.string();
    if (path_string.size() >= sizeof(address.sun_path)) {
        close_listener();
        return std::nullopt;
    }
    std::snprintf(address.sun_path, sizeof(address.sun_path), "%s", path_string.c_str());

    if (::bind(listener_fd, reinterpret_cast<sockaddr *>(&address), sizeof(address)) != 0) {
        close_listener();
        return std::nullopt;
    }
    if (::listen(listener_fd, 16) != 0) {
        close_listener();
        std::filesystem::remove(path, filesystem_error);
        return std::nullopt;
    }
    const int flags = ::fcntl(listener_fd, F_GETFL, 0);
    if (flags < 0 || ::fcntl(listener_fd, F_SETFL, flags | O_NONBLOCK) != 0) {
        close_listener();
        std::filesystem::remove(path, filesystem_error);
        return std::nullopt;
    }

    auto stop = std::make_shared<std::atomic_bool>(false);
    auto active_clients = std::make_shared<std::atomic_size_t>(0U);
    auto active_observers = std::make_shared<std::atomic_size_t>(0U);
    auto thread_stop = stop;
    std::thread handle;
    try {
        handle = std::thread([listener_fd, thread_stop, handshake = std::move(handshake),
                              state = std::move(state), active_clients = std::move(active_clients),
                              active_observers = std::move(active_observers)]() mutable {
            while (!thread_stop->load(std::memory_order_relaxed)) {
                const int client_fd = ::accept(listener_fd, nullptr, nullptr);
                if (client_fd >= 0) {
                    detail::set_socket_timeout(client_fd);
                    auto permit = detail::try_acquire_introspection_client_permit(active_clients);
                    if (!permit.has_value()) {
                        auto response = detail::error_response_json(
                            handshake, "FlowRT introspection connection limit reached");
                        response.push_back('\n');
                        (void)detail::write_all(client_fd, response);
                        ::close(client_fd);
                        continue;
                    }
                    try {
                        std::thread([client_fd, handshake, state, thread_stop, active_observers,
                                     permit = std::move(permit)]() mutable {
                            detail::handle_introspection_connection(client_fd, handshake, state,
                                                                    thread_stop, std::move(permit),
                                                                    active_observers);
                            ::close(client_fd);
                        }).detach();
                    } catch (...) {
                        ::close(client_fd);
                    }
                    continue;
                }
                if (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR) {
                    std::this_thread::sleep_for(std::chrono::milliseconds{10});
                    continue;
                }
                break;
            }
            ::close(listener_fd);
        });
    } catch (...) {
        close_listener();
        std::filesystem::remove(path, filesystem_error);
        return std::nullopt;
    }

    return IntrospectionServer{std::move(path), std::move(handle), std::move(stop)};
}

/**
 * @brief 用当前进程 PID 命名 socket 并启动最小 introspection status 服务。
 */
inline std::optional<IntrospectionServer> spawn_status_server(IntrospectionIdentity identity,
                                                              IntrospectionState state) {
    auto handshake = identity.handshake();
    auto path = runtime_socket_path_for_pid(handshake.pid);
    return spawn_status_server_at(std::move(path), std::move(handshake), std::move(state));
}

}  // namespace flowrt
