/// Operation core primitives C++ smoke 测试。
///
/// 覆盖 OperationId、policy、状态转换、cooperative cancel、进度事件和健康计数。

#include <cassert>
#include <chrono>
#include <cstdint>
#include <flowrt/operation.hpp>

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

    const auto progress = flowrt::OperationProgress<int>{flowrt::OperationId{9U, 10U, 11U}, 3U, 42};
    assert(progress.id.operation_key == 9U);
    assert(progress.sequence == 3U);
    assert(progress.value == 42);
    flowrt::OperationProgressPublisher<int> publisher{flowrt::OperationId{7U, 8U, 9U}};
    publisher.publish(1);
    publisher.publish(2);
    assert(publisher.events().size() == 2U);
    assert(publisher.events()[0].sequence == 0U);
    assert(publisher.events()[1].sequence == 1U);
    assert(publisher.drain().size() == 2U);
    assert(publisher.events().empty());

    const auto ack = flowrt::OperationStartAck::accepted_ack(id);
    assert(ack.accepted);
    assert(ack.id == id);
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
