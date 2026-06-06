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
#define FLOWRT_BACKEND_HEALTH_UNSUPPORTED ((flowrt_backend_health_state_t)4U)

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

/* ── Service ABI ──────────────────────────────────────────────────────────── */

typedef uint16_t flowrt_service_error_t;
#define FLOWRT_SERVICE_OK ((flowrt_service_error_t)0U)
#define FLOWRT_SERVICE_TIMEOUT ((flowrt_service_error_t)1U)
#define FLOWRT_SERVICE_UNAVAILABLE ((flowrt_service_error_t)2U)
#define FLOWRT_SERVICE_BUSY ((flowrt_service_error_t)3U)
#define FLOWRT_SERVICE_REJECTED ((flowrt_service_error_t)4U)
#define FLOWRT_SERVICE_CANCELLED ((flowrt_service_error_t)5U)
#define FLOWRT_SERVICE_DEADLINE_EXCEEDED ((flowrt_service_error_t)6U)
#define FLOWRT_SERVICE_PROTOCOL ((flowrt_service_error_t)7U)
#define FLOWRT_SERVICE_BACKEND ((flowrt_service_error_t)8U)
#define FLOWRT_SERVICE_WOULD_DEADLOCK ((flowrt_service_error_t)9U)
#define FLOWRT_SERVICE_HANDLER_ERROR ((flowrt_service_error_t)10U)

#define FLOWRT_SERVICE_FRAME_MAGIC UINT32_C(0x53525646)
#define FLOWRT_SERVICE_FRAME_VERSION UINT16_C(1)
#define FLOWRT_SERVICE_FRAME_HEADER_SIZE UINT32_C(80)

typedef struct flowrt_service_frame_header_t {
    uint32_t magic;
    uint16_t version;
    uint16_t error_code;
    uint64_t service_id;
    uint64_t session_id;
    uint64_t sequence;
    uint64_t correlation_id;
    uint64_t timeout_ms;
    uint64_t absolute_deadline_ms;
    uint64_t schema_hash;
    uint32_t payload_offset;
    uint32_t payload_len;
    uint32_t error_msg_offset;
    uint32_t error_msg_len;
} flowrt_service_frame_header_t;

#ifdef __cplusplus
}
#endif
