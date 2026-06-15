#pragma once

#include <algorithm>
#include <atomic>
#include <cerrno>
#include <cstdio>
#include <cstdlib>
#include <fcntl.h>
#include <filesystem>
#include <flowrt/introspection/model.hpp>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/un.h>
#include <unistd.h>
#include <utility>

namespace flowrt {

namespace detail {

inline bool write_all(int fd, std::string_view data) {
#if defined(MSG_NOSIGNAL)
    constexpr int send_flags = MSG_NOSIGNAL;
#else
    constexpr int send_flags = 0;
#endif
    std::size_t offset = 0;
    while (offset < data.size()) {
        const auto written = ::send(fd, data.data() + offset, data.size() - offset, send_flags);
        if (written < 0) {
            if (errno == EINTR) {
                continue;
            }
            return false;
        }
        if (written == 0) {
            return false;
        }
        offset += static_cast<std::size_t>(written);
    }
    return true;
}

inline void set_socket_timeout(int fd) {
    timeval timeout{};
    timeout.tv_sec = 1;
    timeout.tv_usec = 0;
    (void)::setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
    (void)::setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout));
}

enum class ReadLineStatus {
    Line,
    Closed,
    Timeout,
    Error,
};

struct ReadLineResult {
    ReadLineStatus status = ReadLineStatus::Closed;
    std::string line;
};

inline ReadLineResult read_line_result(int fd) {
    std::string line;
    char byte = '\0';
    while (line.size() < 65536U) {
        const auto received = ::read(fd, &byte, 1);
        if (received == 0) {
            break;
        }
        if (received < 0) {
            if (errno == EINTR) {
                continue;
            }
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                return ReadLineResult{ReadLineStatus::Timeout, {}};
            }
            return ReadLineResult{ReadLineStatus::Error, {}};
        }
        if (byte == '\n') {
            return ReadLineResult{ReadLineStatus::Line, std::move(line)};
        }
        line.push_back(byte);
    }
    if (!line.empty()) {
        return ReadLineResult{ReadLineStatus::Line, std::move(line)};
    }
    return ReadLineResult{ReadLineStatus::Closed, {}};
}

inline std::optional<std::string> read_line(int fd) {
    auto result = read_line_result(fd);
    if (result.status == ReadLineStatus::Line) {
        return std::move(result.line);
    }
    return std::nullopt;
}

}  // namespace detail

/**
 * @brief 返回当前用户 runtime socket 目录。
 *
 * 优先使用 `$XDG_RUNTIME_DIR/flowrt`；没有时 fallback 到 `/tmp/flowrt.<uid>`，避免不同用户
 * 的同名 PID socket 互相污染。
 */
inline std::filesystem::path runtime_socket_dir() {
    if (const char *runtime_dir = std::getenv("XDG_RUNTIME_DIR"); runtime_dir != nullptr) {
        return std::filesystem::path(runtime_dir) / "flowrt";
    }
    return std::filesystem::path("/tmp") /
           ("flowrt." + std::to_string(static_cast<unsigned int>(::getuid())));
}

/**
 * @brief 返回指定 PID 的默认 runtime socket 路径。
 */
inline std::filesystem::path runtime_socket_path_for_pid(std::uint32_t pid) {
    return runtime_socket_dir() / (std::to_string(pid) + ".sock");
}

namespace detail {

class IntrospectionClientPermit {
   public:
    explicit IntrospectionClientPermit(std::shared_ptr<std::atomic_size_t> active) noexcept
        : active_(std::move(active)) {}
    IntrospectionClientPermit(const IntrospectionClientPermit &) = delete;
    auto operator=(const IntrospectionClientPermit &) -> IntrospectionClientPermit & = delete;

    IntrospectionClientPermit(IntrospectionClientPermit &&other) noexcept
        : active_(std::move(other.active_)) {}

    auto operator=(IntrospectionClientPermit &&other) noexcept -> IntrospectionClientPermit & {
        if (this != std::addressof(other)) {
            release();
            active_ = std::move(other.active_);
        }
        return *this;
    }

    ~IntrospectionClientPermit() { release(); }

   private:
    void release() noexcept {
        if (active_) {
            active_->fetch_sub(1U, std::memory_order_acq_rel);
            active_.reset();
        }
    }

    std::shared_ptr<std::atomic_size_t> active_;
};

inline std::optional<IntrospectionClientPermit> try_acquire_introspection_client_permit(
    const std::shared_ptr<std::atomic_size_t> &active,
    std::size_t limit = MAX_INTROSPECTION_CLIENT_THREADS) {
    limit = std::max<std::size_t>(1U, limit);
    auto current = active->load(std::memory_order_acquire);
    while (true) {
        if (current >= limit) {
            return std::nullopt;
        }
        if (active->compare_exchange_weak(current, current + 1U, std::memory_order_acq_rel,
                                          std::memory_order_acquire)) {
            return IntrospectionClientPermit{active};
        }
    }
}

inline bool unix_socket_accepts_connection(const std::filesystem::path &path) noexcept {
    const int fd = ::socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0) {
        return false;
    }

    sockaddr_un address{};
    address.sun_family = AF_UNIX;
    const auto path_string = path.string();
    if (path_string.size() >= sizeof(address.sun_path)) {
        ::close(fd);
        return false;
    }
    std::snprintf(address.sun_path, sizeof(address.sun_path), "%s", path_string.c_str());
    const bool connected =
        ::connect(fd, reinterpret_cast<sockaddr *>(&address), sizeof(address)) == 0;
    ::close(fd);
    return connected;
}

}  // namespace detail

}  // namespace flowrt
