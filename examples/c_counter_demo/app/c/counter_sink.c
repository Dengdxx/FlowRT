#include "flowrt_app/c_components.h"

#include <stddef.h>
#include <stdint.h>
#include <string.h>

#ifndef FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0
#error "FlowRT C component callback ABI v0 is required"
#endif
#ifndef FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1
#error "FlowRT C component task timing ABI is required"
#endif

_Static_assert((FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0 & UINT64_C(1)) == UINT64_C(1),
               "FlowRT C component callback ABI v0 feature bit is required");
_Static_assert((FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1 & UINT64_C(2)) == UINT64_C(2),
               "FlowRT C component task timing feature bit is required");

typedef struct Count {
    uint32_t value;
} Count;

static flowrt_status_t counter_sink_run_on_message(void *user_data,
                                                   const flowrt_c_component_context_t *context,
                                                   const flowrt_c_input_array_view_t *inputs,
                                                   flowrt_c_output_array_view_t *outputs) {
    (void)user_data;
    (void)context;
    (void)outputs;
    if (inputs == NULL || inputs->data == NULL || inputs->len != 1U) {
        return FLOWRT_STATUS_ERROR;
    }

    const flowrt_c_input_view_t *input = &inputs->data[0];
    if (input->present == 0U) {
        return FLOWRT_STATUS_RETRY;
    }
    if (input->payload.data == NULL || input->payload.len != sizeof(Count)) {
        return FLOWRT_STATUS_ERROR;
    }

    Count count = {0U};
    memcpy(&count, input->payload.data, sizeof(count));
    return count.value == 0U ? FLOWRT_STATUS_ERROR : FLOWRT_STATUS_OK;
}

const flowrt_c_component_callback_table_t *flowrt_app_counter_sink_callbacks(void) {
    static const flowrt_c_component_callback_table_t callbacks = {
        .size = (uint32_t)sizeof(flowrt_c_component_callback_table_t),
        .version_major = FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MAJOR,
        .version_minor = FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MINOR,
        .reserved0 = 0U,
        .feature_flags = FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0 |
                         FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1,
        .user_data = NULL,
        .on_init = NULL,
        .on_start = NULL,
        .on_stop = NULL,
        .on_shutdown = NULL,
        .run_periodic = NULL,
        .run_on_message = counter_sink_run_on_message,
        .run_startup = NULL,
        .run_shutdown = NULL,
        .reserved = {0U},
    };
    return &callbacks;
}
