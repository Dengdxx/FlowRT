/// Operation core primitives C++ smoke 测试。
///
/// 覆盖 OperationId、policy、状态转换、cooperative cancel、进度事件和健康计数。

#include <algorithm>
#include <array>
#include <cassert>
#include <chrono>
#include <cstdint>
#include <flowrt/operation.hpp>
#include <flowrt/wire.hpp>
#include <optional>
#include <span>
#include <vector>

struct TinyGoal {
    std::uint32_t value = 0;

    friend constexpr bool operator==(const TinyGoal &, const TinyGoal &) noexcept = default;

    static constexpr std::size_t wire_size() noexcept { return sizeof(std::uint32_t); }

    void encode_wire(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(wire_size(), output.size());
        flowrt::write_wire_le(output, 0U, value);
    }

    static TinyGoal decode_wire(std::span<const std::uint8_t> input) {
        flowrt::ensure_wire_size(wire_size(), input.size());
        return TinyGoal{.value = flowrt::read_wire_le<std::uint32_t>(input, 0U)};
    }
};

static_assert(flowrt::CanonicalTransportMessage<flowrt::OperationId>);
static_assert(flowrt::CanonicalTransportMessage<flowrt::OperationOwner>);
static_assert(flowrt::CanonicalTransportMessage<flowrt::OperationStartAck>);
static_assert(flowrt::CanonicalTransportMessage<flowrt::OperationStatusSnapshot>);
static_assert(flowrt::CanonicalTransportMessage<flowrt::OperationStartRequest<TinyGoal>>);

template <flowrt::CanonicalTransportMessage T>
T frame_roundtrip(const T &value) {
    std::vector<std::uint8_t> frame(flowrt::detail::encoded_frame_size(value));
    flowrt::detail::encode_frame(value, std::span<std::uint8_t>{frame});
    return flowrt::detail::decode_frame<T>(std::span<const std::uint8_t>{frame});
}

int main() {
    const flowrt::OperationId id{0xAAU, 1U, 0xBBU};
    assert(id.operation_key == 0xAAU);
    assert(id.client_id == 1U);
    assert(id.sequence == 0xBBU);

    auto policy = flowrt::OperationPolicy::make(std::chrono::milliseconds{1000},
                                                flowrt::OperationConcurrencyPolicy::Reject,
                                                flowrt::OperationPreemptPolicy::Reject, 8U, 1U);
    assert(policy.has_value());
    assert(!flowrt::OperationPolicy::make(std::chrono::milliseconds{0},
                                          flowrt::OperationConcurrencyPolicy::Reject,
                                          flowrt::OperationPreemptPolicy::Reject, 8U, 1U)
                .has_value());
    assert(!flowrt::OperationPolicy::make(std::chrono::milliseconds{1000},
                                          flowrt::OperationConcurrencyPolicy::Reject,
                                          flowrt::OperationPreemptPolicy::Reject, 0U, 1U)
                .has_value());
    assert(!flowrt::OperationPolicy::make(std::chrono::milliseconds{1000},
                                          flowrt::OperationConcurrencyPolicy::Queue,
                                          flowrt::OperationPreemptPolicy::CancelRunning, 8U, 0U)
                .has_value());
    flowrt::OperationLifecycle invalid_policy{
        flowrt::OperationId{0U, 0U, 0U},
        flowrt::OperationPolicy{
            .timeout = std::chrono::milliseconds{0},
            .concurrency = flowrt::OperationConcurrencyPolicy::Reject,
            .preempt = flowrt::OperationPreemptPolicy::Reject,
            .queue_depth = 8U,
            .max_in_flight = 1U,
        },
    };
    assert(invalid_policy.transition(flowrt::OperationState::Running) ==
           flowrt::OperationError::InvalidPolicy);

    flowrt::OperationLifecycle lifecycle{id, *policy};
    assert(lifecycle.state() == flowrt::OperationState::Starting);
    assert(lifecycle.transition(flowrt::OperationState::Running) == flowrt::OperationError::Ok);
    assert(lifecycle.transition(flowrt::OperationState::Succeeded) == flowrt::OperationError::Ok);
    assert(lifecycle.transition(flowrt::OperationState::Failed) ==
           flowrt::OperationError::InvalidTransition);
    auto snapshot = lifecycle.snapshot();
    assert(snapshot.state == flowrt::OperationState::Succeeded);
    assert(snapshot.health.started == 1U);
    assert(snapshot.health.succeeded == 1U);

    flowrt::OperationLifecycle canceling{flowrt::OperationId{1U, 2U, 3U}, *policy};
    auto token = canceling.cancel_token();
    auto clone = token;
    assert(!token.is_canceled());
    assert(canceling.transition(flowrt::OperationState::Running) == flowrt::OperationError::Ok);
    assert(canceling.request_cancel() == flowrt::OperationError::Ok);
    assert(token.is_canceled());
    assert(clone.is_canceled());
    assert(canceling.state() == flowrt::OperationState::CancelRequested);
    assert(canceling.transition(flowrt::OperationState::Cancelled) == flowrt::OperationError::Ok);
    snapshot = canceling.snapshot();
    assert(snapshot.cancel_requested);
    assert(snapshot.health.canceled == 1U);

    flowrt::OperationLifecycle timed{flowrt::OperationId{1U, 3U, 3U}, *policy};
    assert(timed.transition(flowrt::OperationState::Running) == flowrt::OperationError::Ok);
    assert(timed.transition(flowrt::OperationState::TimedOut) == flowrt::OperationError::Ok);
    assert(timed.snapshot().health.timeout == 1U);

    flowrt::OperationControl control{99U, *policy};
    const flowrt::OperationOwner owner_a{.scope_key = 10U, .owner_key = 20U};
    const flowrt::OperationOwner owner_b{.scope_key = 10U, .owner_key = 30U};
    assert(control.snapshot().state == flowrt::OperationState::Idle);
    const auto started = control.start(owner_a, 100U);
    assert(started.has_value());
    assert(started->owner == owner_a);
    assert(started->deadline_ms == 1100U);
    assert(control.snapshot().state == flowrt::OperationState::Starting);
    assert(control.mark_running(started->id) == flowrt::OperationControlError::Ok);
    const auto second_owner = control.start(owner_b, 101U);
    assert(!second_owner.has_value());
    assert(second_owner.error() == flowrt::OperationControlError::OwnerConflict);
    assert(control.request_cancel(flowrt::OperationId{99U, owner_a.owner_key, 77U}, owner_a) ==
           flowrt::OperationControlError::StaleInvocation);
    assert(!control.check_deadline(1099U));
    assert(control.check_deadline(1100U));
    assert(control.snapshot().state == flowrt::OperationState::TimedOut);
    assert(control.snapshot().cancel_requested);
    assert(control.cancel_token().has_value());
    assert(control.cancel_token()->is_canceled());

    auto queue_policy = flowrt::OperationPolicy::make(
        std::chrono::milliseconds{50}, flowrt::OperationConcurrencyPolicy::Queue,
        flowrt::OperationPreemptPolicy::Reject, 2U, 1U);
    assert(queue_policy.has_value());
    flowrt::OperationControl queue_control{99U, *queue_policy};
    const auto queued_first = queue_control.start(owner_a, 100U);
    assert(queued_first.has_value());
    assert(queue_control.mark_running(queued_first->id) == flowrt::OperationControlError::Ok);
    const auto queued_second = queue_control.start(owner_a, 101U);
    assert(queued_second.has_value());
    assert(queue_control.queued_len() == 1U);
    assert(queue_control.status(queued_second->id)->state == flowrt::OperationState::Starting);
    assert(queue_control.mark_running(queued_second->id) ==
           flowrt::OperationControlError::StaleInvocation);
    assert(queue_control.complete_at(queued_first->id, flowrt::OperationState::Succeeded, 110U) ==
           flowrt::OperationControlError::Ok);
    assert(queue_control.queued_len() == 0U);
    assert(queue_control.snapshot().id == queued_second->id);
    assert(queue_control.snapshot().state == flowrt::OperationState::Starting);
    assert(queue_control.mark_running(queued_second->id) == flowrt::OperationControlError::Ok);

    auto queue_timeout_policy = flowrt::OperationPolicy::make(
        std::chrono::milliseconds{50}, flowrt::OperationConcurrencyPolicy::Queue,
        flowrt::OperationPreemptPolicy::Reject, 2U, 1U, std::chrono::milliseconds{100});
    assert(queue_timeout_policy.has_value());
    flowrt::OperationControl queue_timeout_control{99U, *queue_timeout_policy};
    const auto queue_timeout_first = queue_timeout_control.start_with_timeout(
        owner_a, 100U, std::chrono::milliseconds{200});
    assert(queue_timeout_first.has_value());
    assert(queue_timeout_control.mark_running(queue_timeout_first->id) ==
           flowrt::OperationControlError::Ok);
    const auto queue_timeout_second = queue_timeout_control.start_with_timeout(
        owner_a, 101U, std::chrono::milliseconds{10});
    assert(queue_timeout_second.has_value());
    assert(queue_timeout_control.queued_len() == 1U);
    assert(queue_timeout_control.check_deadline(111U));
    assert(queue_timeout_control.queued_len() == 0U);
    assert(queue_timeout_control.snapshot().id == queue_timeout_first->id);
    assert(queue_timeout_control.snapshot().state == flowrt::OperationState::Running);
    assert(queue_timeout_control.status(queue_timeout_second->id)->state ==
           flowrt::OperationState::TimedOut);
    assert(!queue_timeout_control.ready_to_run(queue_timeout_second->id));

    auto preempt_policy = flowrt::OperationPolicy::make(
        std::chrono::milliseconds{50}, flowrt::OperationConcurrencyPolicy::Reject,
        flowrt::OperationPreemptPolicy::CancelRunning, 8U, 1U);
    assert(preempt_policy.has_value());
    flowrt::OperationControl preempt_control{99U, *preempt_policy};
    const auto preempt_first = preempt_control.start(owner_a, 100U);
    assert(preempt_first.has_value());
    assert(preempt_control.mark_running(preempt_first->id) == flowrt::OperationControlError::Ok);
    const auto first_cancel = preempt_control.cancel_token();
    assert(first_cancel.has_value());
    const auto preempt_second = preempt_control.start(owner_a, 101U);
    assert(preempt_second.has_value());
    assert(first_cancel->is_canceled());
    assert(preempt_control.status(preempt_first->id)->state ==
           flowrt::OperationState::CancelRequested);
    assert(preempt_control.snapshot().id == preempt_second->id);
    assert(preempt_control.snapshot().state == flowrt::OperationState::Starting);
    assert(preempt_control.snapshot().health.preempted == 1U);
    assert(preempt_control.mark_running(preempt_second->id) == flowrt::OperationControlError::Ok);

    auto multi_policy = flowrt::OperationPolicy::make(
        std::chrono::milliseconds{50}, flowrt::OperationConcurrencyPolicy::Queue,
        flowrt::OperationPreemptPolicy::Reject, 2U, 2U);
    assert(multi_policy.has_value());
    flowrt::OperationControl multi_control{99U, *multi_policy};
    const auto multi_first = multi_control.start(owner_a, 100U);
    const auto multi_second = multi_control.start(owner_a, 101U);
    const auto multi_third = multi_control.start(owner_a, 102U);
    assert(multi_first.has_value());
    assert(multi_second.has_value());
    assert(multi_third.has_value());
    assert(multi_control.ready_to_run(multi_first->id));
    assert(multi_control.ready_to_run(multi_second->id));
    assert(!multi_control.ready_to_run(multi_third->id));
    assert(multi_control.queued_len() == 1U);
    assert(multi_control.mark_running(multi_first->id) == flowrt::OperationControlError::Ok);
    assert(multi_control.mark_running(multi_second->id) == flowrt::OperationControlError::Ok);
    assert(multi_control.complete_at(multi_first->id, flowrt::OperationState::Succeeded, 110U) ==
           flowrt::OperationControlError::Ok);
    assert(multi_control.ready_to_run(multi_third->id));
    assert(multi_control.queued_len() == 0U);
    assert(multi_control.mark_running(multi_third->id) == flowrt::OperationControlError::Ok);

    auto retention_policy = flowrt::OperationPolicy::make(
        std::chrono::milliseconds{50}, flowrt::OperationConcurrencyPolicy::Reject,
        flowrt::OperationPreemptPolicy::Reject, 8U, 1U, std::chrono::milliseconds{20});
    assert(retention_policy.has_value());
    flowrt::OperationControl retention_control{99U, *retention_policy};
    const auto retained = retention_control.start(owner_a, 100U);
    assert(retained.has_value());
    assert(retention_control.mark_running(retained->id) == flowrt::OperationControlError::Ok);
    assert(retention_control.complete_at(retained->id, flowrt::OperationState::Succeeded, 110U) ==
           flowrt::OperationControlError::Ok);
    assert(retention_control.status(retained->id)->state == flowrt::OperationState::Succeeded);
    retention_control.evict_retained_results(129U);
    assert(retention_control.status(retained->id)->state == flowrt::OperationState::Succeeded);
    retention_control.evict_retained_results(131U);
    const auto evicted = retention_control.status(retained->id);
    assert(!evicted.has_value());
    assert(evicted.error() == flowrt::OperationControlError::StaleInvocation);

    const auto progress = flowrt::OperationProgress<int>{flowrt::OperationId{9U, 10U, 11U}, 3U, 42};
    assert(progress.id.operation_key == 9U);
    assert(progress.sequence == 3U);
    assert(progress.value == 42);
    flowrt::OperationProgressPublisher<TinyGoal> publisher{flowrt::OperationId{7U, 8U, 9U}};
    publisher.publish(TinyGoal{.value = 1U});
    publisher.publish(TinyGoal{.value = 2U});
    assert(publisher.events().size() == 2U);
    assert(publisher.events()[0].sequence == 0U);
    assert(publisher.events()[1].sequence == 1U);
    assert(publisher.drain().size() == 2U);
    assert(publisher.events().empty());

    std::optional<std::vector<std::uint8_t>> progress_payload;
    flowrt::OperationProgressPublisher<TinyGoal> hooked_publisher{
        flowrt::OperationId{7U, 8U, 10U},
        [&progress_payload](flowrt::OperationId, std::uint64_t sequence,
                            std::optional<std::vector<std::uint8_t>> payload) {
            assert(sequence == 0U);
            progress_payload = std::move(payload);
        }};
    hooked_publisher.publish(TinyGoal{.value = 0x01020304U});
    assert(progress_payload.has_value());
    assert(progress_payload->size() == TinyGoal::wire_size());
    assert((*progress_payload)[0] == 0x04U);
    assert((*progress_payload)[1] == 0x03U);
    assert((*progress_payload)[2] == 0x02U);
    assert((*progress_payload)[3] == 0x01U);

    auto payload_control = flowrt::OperationControl{77U, *retention_policy};
    const auto payload_started = payload_control.start(owner_a, 200U);
    assert(payload_started.has_value());
    assert(payload_control.mark_running(payload_started->id) == flowrt::OperationControlError::Ok);
    std::vector<std::uint8_t> result_payload{9U, 8U, 7U, 6U};
    payload_control.publish_progress_with_payload(payload_started->id, 4U, result_payload);
    assert(payload_control.complete_with_payload(payload_started->id,
                                                 flowrt::OperationState::Succeeded,
                                                 result_payload) ==
           flowrt::OperationControlError::Ok);
    const auto runtime_events = payload_control.drain_events();
    const auto progress_event =
        std::find_if(runtime_events.begin(), runtime_events.end(), [](const auto &event) {
            return event.kind == flowrt::OperationRuntimeEventKind::Progress;
        });
    assert(progress_event != runtime_events.end());
    assert(progress_event->payload.has_value());
    assert(*progress_event->payload == result_payload);
    const auto result_event =
        std::find_if(runtime_events.begin(), runtime_events.end(), [](const auto &event) {
            return event.kind == flowrt::OperationRuntimeEventKind::Result;
        });
    assert(result_event != runtime_events.end());
    assert(result_event->payload.has_value());
    assert(*result_event->payload == result_payload);

    const auto ack = flowrt::OperationStartAck::accepted_ack(id);
    assert(ack.accepted);
    assert(ack.id == id);
    const flowrt::OperationOwner owner{.scope_key = 0x0A0B0C0D0E0F1011U,
                                       .owner_key = 0x2122232425262728U};
    const auto ack_with_authority =
        flowrt::OperationStartAck::accepted_with_authority(id, owner, 123456U);
    const auto ack_decoded = frame_roundtrip(ack_with_authority);
    assert(ack_decoded.id == ack_with_authority.id);
    assert(ack_decoded.owner == ack_with_authority.owner);
    assert(ack_decoded.deadline_ms == ack_with_authority.deadline_ms);
    assert(ack_decoded.accepted == ack_with_authority.accepted);

    const auto status = flowrt::OperationStatusSnapshot{
        .id = id,
        .owner = owner,
        .state = flowrt::OperationState::CancelRequested,
        .cancel_requested = true,
        .deadline_ms = 123456U,
        .health =
            flowrt::OperationHealthSnapshot{
                .started = 1U,
                .succeeded = 2U,
                .failed = 3U,
                .canceled = 4U,
                .timeout = 5U,
                .preempted = 6U,
            },
    };
    const auto status_decoded = frame_roundtrip(status);
    assert(status_decoded.id == status.id);
    assert(status_decoded.owner == status.owner);
    assert(status_decoded.state == status.state);
    assert(status_decoded.cancel_requested == status.cancel_requested);
    assert(status_decoded.deadline_ms == status.deadline_ms);
    assert(status_decoded.health.started == status.health.started);
    assert(status_decoded.health.succeeded == status.health.succeeded);
    assert(status_decoded.health.failed == status.health.failed);
    assert(status_decoded.health.canceled == status.health.canceled);
    assert(status_decoded.health.timeout == status.health.timeout);
    assert(status_decoded.health.preempted == status.health.preempted);

    const auto start = flowrt::OperationStartRequest<TinyGoal>{
        .goal = TinyGoal{.value = 42U},
        .owner = owner,
        .timeout = std::chrono::milliseconds{250},
    };
    const auto start_decoded = frame_roundtrip(start);
    assert(start_decoded.goal == start.goal);
    assert(start_decoded.owner == start.owner);
    assert(start_decoded.timeout == start.timeout);
    assert(flowrt::operation_client_error_from_service_error(flowrt::ServiceError::Backend) ==
           flowrt::OperationClientError::Backend);
    assert(flowrt::operation_client_error_from_service_error(flowrt::ServiceError::WouldDeadlock) ==
           flowrt::OperationClientError::WouldDeadlock);
    const auto client_ok = flowrt::OperationClientResult<int>::ok(12);
    assert(client_ok.is_ok());
    assert(client_ok.value().has_value());
    assert(*client_ok.value() == 12);
    const auto client_err =
        flowrt::OperationClientResult<int>::err(flowrt::OperationClientError::Backend);
    assert(client_err.is_err());
    assert(client_err.error_code() == flowrt::OperationClientError::Backend);
    auto handler_result = flowrt::OperationHandlerResult<int>::succeeded(5);
    assert(handler_result.kind() == flowrt::OperationHandlerResult<int>::Kind::Succeeded);
    assert(handler_result.value().has_value());
    assert(*handler_result.value() == 5);
    assert(flowrt::OperationHandlerResult<int>::failed().kind() ==
           flowrt::OperationHandlerResult<int>::Kind::Failed);

    flowrt::OperationHealthCounters counters;
    counters.record_state(flowrt::OperationState::Running);
    counters.record_state(flowrt::OperationState::Failed);
    counters.record_state(flowrt::OperationState::Cancelled);
    counters.record_state(flowrt::OperationState::TimedOut);
    const auto health = counters.snapshot();
    assert(health.started == 1U);
    assert(health.failed == 1U);
    assert(health.canceled == 1U);
    assert(health.timeout == 1U);

    assert(flowrt::to_string(flowrt::OperationState::CancelRequested) == "cancel_requested");
    assert(flowrt::to_string(flowrt::OperationState::TimedOut) == "timed_out");
    assert(flowrt::to_string(flowrt::OperationState::Cancelled) == "cancelled");
    assert(flowrt::to_string(flowrt::OperationError::InvalidTransition) == "InvalidTransition");
}
