#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * FlowRT C ABI 基础边界。
 *
 * 该 header 只定义跨语言可共享的 POD 类型和值编码，不暴露 C++/Rust runtime 对象、
 * backend SDK 句柄或所有权语义。所有 view 都是借用视图；调用方如需跨调用保存数据，
 * 必须自行复制。
 */

#define FLOWRT_ABI_VERSION_MAJOR UINT32_C(0)
#define FLOWRT_ABI_VERSION_MINOR UINT32_C(1)

typedef uint32_t flowrt_status_t;
#define FLOWRT_STATUS_OK ((flowrt_status_t)0U)
#define FLOWRT_STATUS_RETRY ((flowrt_status_t)1U)
#define FLOWRT_STATUS_ERROR ((flowrt_status_t)2U)

typedef uint32_t flowrt_backend_kind_t;
#define FLOWRT_BACKEND_INPROC ((flowrt_backend_kind_t)0U)
#define FLOWRT_BACKEND_IOX2 ((flowrt_backend_kind_t)1U)
#define FLOWRT_BACKEND_ZENOH ((flowrt_backend_kind_t)2U)

typedef uint32_t flowrt_backend_health_state_t;
#define FLOWRT_BACKEND_HEALTH_READY ((flowrt_backend_health_state_t)0U)
#define FLOWRT_BACKEND_HEALTH_DEGRADED ((flowrt_backend_health_state_t)1U)
#define FLOWRT_BACKEND_HEALTH_RECONNECTING ((flowrt_backend_health_state_t)2U)
#define FLOWRT_BACKEND_HEALTH_FAILED ((flowrt_backend_health_state_t)3U)

typedef struct flowrt_string_view_t {
    const char *data;
    size_t len;
} flowrt_string_view_t;

typedef struct flowrt_bytes_view_t {
    const uint8_t *data;
    size_t len;
} flowrt_bytes_view_t;

typedef struct flowrt_reconnect_policy_t {
    uint64_t initial_delay_ms;
    uint64_t max_delay_ms;
    uint32_t max_attempts;
    uint8_t has_max_attempts;
    uint8_t reserved[3];
} flowrt_reconnect_policy_t;

typedef struct flowrt_backend_health_snapshot_t {
    flowrt_backend_health_state_t state;
    uint32_t attempt;
    uint64_t next_retry_unix_ms;
    flowrt_string_view_t last_error;
    uint8_t has_next_retry_unix_ms;
    uint8_t recoverable;
    uint8_t reserved[6];
} flowrt_backend_health_snapshot_t;

#ifdef __cplusplus
}
#endif
