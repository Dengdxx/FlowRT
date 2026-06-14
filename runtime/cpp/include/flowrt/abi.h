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
#define FLOWRT_ABI_VERSION_MINOR UINT32_C(2)

#define FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MAJOR UINT32_C(0)
#define FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MINOR UINT32_C(2)
#define FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0 UINT64_C(1)
#define FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1 UINT64_C(2)

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

typedef struct flowrt_u128_t {
    uint64_t lo;
    uint64_t hi;
} flowrt_u128_t;

typedef struct flowrt_i128_t {
    uint64_t lo;
    uint64_t hi;
} flowrt_i128_t;

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

typedef uint32_t flowrt_frame_lease_status_t;
#define FLOWRT_FRAME_LEASE_ATTACHED ((flowrt_frame_lease_status_t)0U)
#define FLOWRT_FRAME_LEASE_ACQUIRED ((flowrt_frame_lease_status_t)1U)
#define FLOWRT_FRAME_LEASE_RELEASED ((flowrt_frame_lease_status_t)2U)
#define FLOWRT_FRAME_LEASE_EXPIRED ((flowrt_frame_lease_status_t)3U)
#define FLOWRT_FRAME_LEASE_GENERATION_MISMATCH ((flowrt_frame_lease_status_t)4U)
#define FLOWRT_FRAME_LEASE_ERROR ((flowrt_frame_lease_status_t)5U)

typedef struct flowrt_resource_descriptor_t {
    flowrt_string_view_t resource_id;
    flowrt_string_view_t slot;
    uint64_t generation;
} flowrt_resource_descriptor_t;

typedef struct flowrt_frame_descriptor_t {
    flowrt_resource_descriptor_t resource;
    uint64_t size_bytes;
    flowrt_string_view_t format;
    flowrt_string_view_t encoding;
    flowrt_string_view_t metadata_json;
} flowrt_frame_descriptor_t;

/* ── 后续语言边界 view ────────────────────────────────────────────────────── */

typedef uint32_t flowrt_frame_encoding_t;
#define FLOWRT_FRAME_ENCODING_FIXED_PLAIN ((flowrt_frame_encoding_t)0U)
#define FLOWRT_FRAME_ENCODING_CANONICAL_FRAME_V1 ((flowrt_frame_encoding_t)1U)

typedef struct flowrt_frame_view_t {
    flowrt_string_view_t channel_name;
    flowrt_string_view_t message_type;
    uint64_t schema_hash;
    flowrt_frame_encoding_t encoding;
    uint32_t flags;
    flowrt_bytes_view_t frame;
    flowrt_bytes_view_t header;
    flowrt_bytes_view_t tail;
    uint64_t source_time_ms;
    uint64_t published_at_ms;
    uint64_t revision;
    uint8_t has_source_time_ms;
    uint8_t has_published_at_ms;
    uint8_t has_revision;
    uint8_t reserved[5];
} flowrt_frame_view_t;

typedef uint32_t flowrt_params_update_status_t;
#define FLOWRT_PARAMS_UPDATE_ACCEPTED ((flowrt_params_update_status_t)0U)
#define FLOWRT_PARAMS_UPDATE_APPLIED ((flowrt_params_update_status_t)1U)
#define FLOWRT_PARAMS_UPDATE_REJECTED ((flowrt_params_update_status_t)2U)
#define FLOWRT_PARAMS_UPDATE_PARTIAL ((flowrt_params_update_status_t)3U)
#define FLOWRT_PARAMS_UPDATE_UNSUPPORTED ((flowrt_params_update_status_t)4U)
#define FLOWRT_PARAMS_UPDATE_ERROR ((flowrt_params_update_status_t)5U)

typedef struct flowrt_param_view_t {
    flowrt_string_view_t instance_name;
    flowrt_string_view_t param_name;
    flowrt_string_view_t type_name;
    flowrt_string_view_t update_policy;
    flowrt_string_view_t current_json;
    flowrt_string_view_t pending_json;
    flowrt_string_view_t min_json;
    flowrt_string_view_t max_json;
    flowrt_string_view_t choices_json;
    uint64_t schema_hash;
    uint64_t revision;
    uint8_t mutable_at_runtime;
    uint8_t has_pending;
    uint8_t has_min;
    uint8_t has_max;
    uint8_t reserved[4];
} flowrt_param_view_t;

typedef struct flowrt_params_view_t {
    const flowrt_param_view_t *data;
    size_t len;
    uint64_t revision;
    uint64_t applied_unix_ms;
    uint8_t has_applied_unix_ms;
    uint8_t reserved[7];
} flowrt_params_view_t;

typedef struct flowrt_params_update_result_t {
    flowrt_params_update_status_t status;
    uint32_t applied_count;
    uint32_t rejected_count;
    uint32_t reserved0;
    uint64_t revision;
    uint64_t error_index;
    uint8_t has_error_index;
    uint8_t reserved[7];
    flowrt_string_view_t message;
} flowrt_params_update_result_t;

typedef uint32_t flowrt_operation_state_t;
#define FLOWRT_OPERATION_STATE_IDLE ((flowrt_operation_state_t)0U)
#define FLOWRT_OPERATION_STATE_STARTING ((flowrt_operation_state_t)1U)
#define FLOWRT_OPERATION_STATE_RUNNING ((flowrt_operation_state_t)2U)
#define FLOWRT_OPERATION_STATE_CANCEL_REQUESTED ((flowrt_operation_state_t)3U)
#define FLOWRT_OPERATION_STATE_SUCCEEDED ((flowrt_operation_state_t)4U)
#define FLOWRT_OPERATION_STATE_FAILED ((flowrt_operation_state_t)5U)
#define FLOWRT_OPERATION_STATE_CANCELLED ((flowrt_operation_state_t)6U)
#define FLOWRT_OPERATION_STATE_TIMED_OUT ((flowrt_operation_state_t)7U)

typedef struct flowrt_operation_id_t {
    uint64_t operation_key;
    uint64_t client_id;
    uint64_t sequence;
} flowrt_operation_id_t;

typedef struct flowrt_operation_id_array_view_t {
    const flowrt_operation_id_t *data;
    size_t len;
} flowrt_operation_id_array_view_t;

typedef struct flowrt_operation_status_view_t {
    flowrt_string_view_t operation_name;
    flowrt_operation_id_array_view_t current_operation_ids;
    uint64_t running;
    uint64_t queued;
    uint64_t total_started;
    uint64_t succeeded_count;
    uint64_t failed_count;
    uint64_t canceled_count;
    uint64_t timeout_count;
    uint64_t preempted_count;
    uint64_t last_transition_ms;
    uint8_t ready;
    uint8_t has_last_transition_ms;
    uint8_t reserved[6];
} flowrt_operation_status_view_t;

typedef struct flowrt_operation_progress_view_t {
    flowrt_string_view_t operation_name;
    flowrt_operation_id_t id;
    uint64_t sequence;
    flowrt_frame_view_t progress;
    uint64_t published_at_ms;
    uint8_t has_published_at_ms;
    uint8_t reserved[7];
} flowrt_operation_progress_view_t;

typedef struct flowrt_operation_result_summary_view_t {
    flowrt_string_view_t operation_name;
    flowrt_operation_id_t id;
    flowrt_operation_state_t state;
    uint8_t has_result;
    uint8_t has_error_message;
    uint8_t has_completed_unix_ms;
    uint8_t reserved0;
    uint64_t completed_unix_ms;
    flowrt_frame_view_t result;
    flowrt_string_view_t error_message;
} flowrt_operation_result_summary_view_t;

typedef uint32_t flowrt_diagnostic_severity_t;
#define FLOWRT_DIAGNOSTIC_INFO ((flowrt_diagnostic_severity_t)0U)
#define FLOWRT_DIAGNOSTIC_WARN ((flowrt_diagnostic_severity_t)1U)
#define FLOWRT_DIAGNOSTIC_ERROR ((flowrt_diagnostic_severity_t)2U)

typedef uint32_t flowrt_resource_health_state_t;
#define FLOWRT_RESOURCE_HEALTH_UNKNOWN ((flowrt_resource_health_state_t)0U)
#define FLOWRT_RESOURCE_HEALTH_READY ((flowrt_resource_health_state_t)1U)
#define FLOWRT_RESOURCE_HEALTH_DEGRADED ((flowrt_resource_health_state_t)2U)
#define FLOWRT_RESOURCE_HEALTH_FAILED ((flowrt_resource_health_state_t)3U)
#define FLOWRT_RESOURCE_HEALTH_UNAVAILABLE ((flowrt_resource_health_state_t)4U)

typedef struct flowrt_diagnostic_view_t {
    flowrt_string_view_t source;
    flowrt_string_view_t code;
    flowrt_string_view_t message;
    flowrt_diagnostic_severity_t severity;
    uint32_t reserved0;
    uint64_t timestamp_unix_ms;
    uint8_t has_timestamp_unix_ms;
    uint8_t reserved[7];
} flowrt_diagnostic_view_t;

typedef struct flowrt_resource_health_snapshot_t {
    flowrt_string_view_t name;
    flowrt_string_view_t capability;
    flowrt_resource_health_state_t state;
    uint8_t ready;
    uint8_t required;
    uint8_t has_updated_unix_ms;
    uint8_t has_generation;
    uint64_t updated_unix_ms;
    uint64_t generation;
    flowrt_string_view_t message;
    flowrt_string_view_t last_error;
} flowrt_resource_health_snapshot_t;

typedef struct flowrt_diagnostic_array_view_t {
    const flowrt_diagnostic_view_t *data;
    size_t len;
} flowrt_diagnostic_array_view_t;

typedef struct flowrt_resource_health_array_view_t {
    const flowrt_resource_health_snapshot_t *data;
    size_t len;
} flowrt_resource_health_array_view_t;

typedef struct flowrt_diagnostics_snapshot_t {
    flowrt_string_view_t package_name;
    flowrt_string_view_t process_name;
    flowrt_diagnostic_array_view_t diagnostics;
    flowrt_resource_health_array_view_t resources;
    uint64_t generated_unix_ms;
    uint8_t healthy;
    uint8_t has_generated_unix_ms;
    uint8_t reserved[6];
} flowrt_diagnostics_snapshot_t;

/* ── C component callback ABI ─────────────────────────────────────────────── */

/*
 * C component callback ABI 只描述 FlowRT runtime shell 与已编入 app binary 的 C
 * component 之间的调用边界。所有名称和 payload 都是借用视图；callback 不接管
 * 输入 payload、输出 slot 或 user_data 的所有权。函数指针可以按 C 语言惯例传
 * NULL；adapter 必须在调用前校验 size、version、feature_flags 和必填 callback。
 * 当前 callback table 必须同时设置 FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0 和
 * FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1，未识别的 feature bit 必须被 adapter 拒绝。
 */

typedef uint32_t flowrt_c_output_status_t;
#define FLOWRT_C_OUTPUT_UNWRITTEN ((flowrt_c_output_status_t)0U)
#define FLOWRT_C_OUTPUT_WRITTEN ((flowrt_c_output_status_t)1U)
#define FLOWRT_C_OUTPUT_TRUNCATED ((flowrt_c_output_status_t)2U)
#define FLOWRT_C_OUTPUT_ERROR ((flowrt_c_output_status_t)3U)

typedef uint32_t flowrt_c_clock_source_t;
#define FLOWRT_C_CLOCK_SOURCE_RUNTIME ((flowrt_c_clock_source_t)0U)
#define FLOWRT_C_CLOCK_SOURCE_REPLAY ((flowrt_c_clock_source_t)1U)

typedef struct flowrt_c_task_timing_t {
    uint64_t step;
    flowrt_string_view_t task_name;
    flowrt_string_view_t trigger;
    flowrt_c_clock_source_t clock_source;
    uint32_t reserved0;
    uint64_t scheduled_time_ms;
    uint64_t observed_time_ms;
    uint64_t scheduled_delta_ms;
    uint64_t observed_delta_ms;
    uint64_t period_ms;
    uint64_t deadline_ms;
    uint64_t lateness_ms;
    uint64_t missed_periods;
    uint8_t has_period_ms;
    uint8_t has_deadline_ms;
    uint8_t deadline_missed;
    uint8_t overrun;
    uint8_t reserved[4];
} flowrt_c_task_timing_t;

typedef struct flowrt_c_component_context_t {
    flowrt_string_view_t component_name;
    flowrt_string_view_t instance_name;
    flowrt_string_view_t task_name;
    flowrt_string_view_t lane_name;
    uint64_t step;
    uint64_t tick_time_ms;
    uint64_t deadline_ms;
    uint8_t has_deadline_ms;
    uint8_t has_timing;
    uint8_t reserved[6];
    flowrt_c_task_timing_t timing;
} flowrt_c_component_context_t;

typedef struct flowrt_c_input_view_t {
    flowrt_string_view_t name;
    flowrt_string_view_t type_name;
    uint64_t schema_hash;
    uint64_t size_bytes;
    flowrt_bytes_view_t payload;
    uint64_t source_time_ms;
    uint64_t revision;
    uint8_t present;
    uint8_t stale;
    uint8_t reserved[6];
} flowrt_c_input_view_t;

typedef struct flowrt_c_output_slot_t {
    flowrt_string_view_t name;
    flowrt_string_view_t type_name;
    uint64_t schema_hash;
    uint64_t size_bytes;
    uint8_t *data;
    size_t capacity;
    size_t written_len;
    flowrt_c_output_status_t status;
    uint8_t reserved[4];
} flowrt_c_output_slot_t;

typedef struct flowrt_c_input_array_view_t {
    const flowrt_c_input_view_t *data;
    size_t len;
} flowrt_c_input_array_view_t;

typedef struct flowrt_c_output_array_view_t {
    flowrt_c_output_slot_t *data;
    size_t len;
} flowrt_c_output_array_view_t;

typedef flowrt_status_t (*flowrt_c_lifecycle_callback_t)(
    void *user_data, const flowrt_c_component_context_t *context);

typedef flowrt_status_t (*flowrt_c_task_callback_t)(void *user_data,
                                                    const flowrt_c_component_context_t *context,
                                                    const flowrt_c_input_array_view_t *inputs,
                                                    flowrt_c_output_array_view_t *outputs);

typedef struct flowrt_c_component_callback_table_t {
    uint32_t size;
    uint32_t version_major;
    uint32_t version_minor;
    uint32_t reserved0;
    uint64_t feature_flags;
    void *user_data;
    flowrt_c_lifecycle_callback_t on_init;
    flowrt_c_lifecycle_callback_t on_start;
    flowrt_c_lifecycle_callback_t on_stop;
    flowrt_c_lifecycle_callback_t on_shutdown;
    flowrt_c_task_callback_t run_periodic;
    flowrt_c_task_callback_t run_on_message;
    flowrt_c_task_callback_t run_startup;
    flowrt_c_task_callback_t run_shutdown;
    uint64_t reserved[8];
} flowrt_c_component_callback_table_t;

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
