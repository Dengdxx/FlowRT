// FlowRT 管理参考模板（C）。可删除重建；复制到用户 app/ 后再修改。

#include "flowrt_app/c_components.h"

#include <stddef.h>
#include <stdint.h>
#include <string.h>

static flowrt_status_t controller_on_init(void *user_data,
                                          const flowrt_c_component_context_t *context) {
    (void)user_data;
    (void)context;
    return FLOWRT_STATUS_OK;
}

static flowrt_status_t controller_on_start(void *user_data,
                                          const flowrt_c_component_context_t *context) {
    (void)user_data;
    (void)context;
    return FLOWRT_STATUS_OK;
}

static flowrt_status_t controller_on_stop(void *user_data,
                                          const flowrt_c_component_context_t *context) {
    (void)user_data;
    (void)context;
    return FLOWRT_STATUS_OK;
}

static flowrt_status_t controller_on_shutdown(void *user_data,
                                          const flowrt_c_component_context_t *context) {
    (void)user_data;
    (void)context;
    return FLOWRT_STATUS_OK;
}

static flowrt_status_t controller_run_periodic(void *user_data,
                                             const flowrt_c_component_context_t *context,
                                             const flowrt_c_input_array_view_t *inputs,
                                             flowrt_c_output_array_view_t *outputs) {
    (void)user_data;
    (void)context;
    (void)inputs;
    if (outputs == NULL || (outputs->len > 0U && outputs->data == NULL)) {
        return FLOWRT_STATUS_ERROR;
    }
    for (size_t index = 0U; index < outputs->len; ++index) {
        flowrt_c_output_slot_t *slot = &outputs->data[index];
        if (slot->data == NULL || slot->capacity < slot->size_bytes) {
            return FLOWRT_STATUS_ERROR;
        }
        memset(slot->data, 0, slot->size_bytes);
        slot->written_len = slot->size_bytes;
        slot->status = FLOWRT_C_OUTPUT_WRITTEN;
    }
    return FLOWRT_STATUS_OK;
}

const flowrt_c_component_callback_table_t *flowrt_app_controller_callbacks(void) {
    static const flowrt_c_component_callback_table_t callbacks = {
        .size = (uint32_t)sizeof(flowrt_c_component_callback_table_t),
        .version_major = FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MAJOR,
        .version_minor = FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MINOR,
        .reserved0 = 0U,
        .feature_flags = FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0 |
                         FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1,
        .user_data = NULL,
        .on_init = controller_on_init,
        .on_start = controller_on_start,
        .on_stop = controller_on_stop,
        .on_shutdown = controller_on_shutdown,
        .run_periodic = controller_run_periodic,
        .run_on_message = NULL,
        .run_startup = NULL,
        .run_shutdown = NULL,
        .reserved = {0U},
    };
    return &callbacks;
}
