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

typedef struct CounterSourceState {
    uint32_t value;
} CounterSourceState;

static flowrt_status_t counter_source_run_periodic(void *user_data,
                                                   const flowrt_c_component_context_t *context,
                                                   const flowrt_c_input_array_view_t *inputs,
                                                   flowrt_c_output_array_view_t *outputs) {
    (void)context;
    (void)inputs;
    CounterSourceState *state = (CounterSourceState *)user_data;
    if (state == NULL || outputs == NULL || outputs->data == NULL || outputs->len != 1U) {
        return FLOWRT_STATUS_ERROR;
    }

    flowrt_c_output_slot_t *slot = &outputs->data[0];
    if (slot->data == NULL || slot->capacity < sizeof(Count) || slot->size_bytes != sizeof(Count)) {
        return FLOWRT_STATUS_ERROR;
    }

    const Count count = {.value = ++state->value};
    memset(slot->data, 0, slot->capacity);
    memcpy(slot->data, &count, sizeof(count));
    slot->written_len = sizeof(count);
    slot->status = FLOWRT_C_OUTPUT_WRITTEN;
    return FLOWRT_STATUS_OK;
}

static CounterSourceState counter_source_state = {0U};

const flowrt_c_component_callback_table_t *flowrt_app_counter_source_callbacks(void) {
    static const flowrt_c_component_callback_table_t callbacks = {
        .size = (uint32_t)sizeof(flowrt_c_component_callback_table_t),
        .version_major = FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MAJOR,
        .version_minor = FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MINOR,
        .reserved0 = 0U,
        .feature_flags = FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0 |
                         FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1,
        .user_data = &counter_source_state,
        .on_init = NULL,
        .on_start = NULL,
        .on_stop = NULL,
        .on_shutdown = NULL,
        .run_periodic = counter_source_run_periodic,
        .run_on_message = NULL,
        .run_startup = NULL,
        .run_shutdown = NULL,
        .reserved = {0U},
    };
    return &callbacks;
}
