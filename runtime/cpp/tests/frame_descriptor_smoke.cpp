#include <cassert>
#include <cstddef>
#include <flowrt/runtime.hpp>
#include <optional>
#include <string>
#include <type_traits>

int main() {
    static_assert(std::is_standard_layout_v<flowrt::FrameDescriptorFields>);
    static_assert(std::is_trivially_copyable_v<flowrt::FrameDescriptorFields>);
    static_assert(sizeof(flowrt::FrameDescriptorFields) == 64U);
    static_assert(alignof(flowrt::FrameDescriptorFields) == alignof(std::uint64_t));
    static_assert(offsetof(flowrt::FrameDescriptorFields, resource_id_hash) == 0U);
    static_assert(offsetof(flowrt::FrameDescriptorFields, slot) == 8U);
    static_assert(offsetof(flowrt::FrameDescriptorFields, generation) == 16U);
    static_assert(offsetof(flowrt::FrameDescriptorFields, flags) == 60U);

    const flowrt::FrameDescriptorFields fields{
        .resource_id_hash = 0xCAFEU,
        .slot = 7U,
        .generation = 42U,
        .size_bytes = 921600U,
        .timestamp_unix_ns = 1700000000U,
        .width = 640U,
        .height = 480U,
        .stride_bytes = 1920U,
        .format_id = 3U,
        .encoding_id = 9U,
        .flags = 1U,
    };
    const auto fixed_descriptor = flowrt::FrameDescriptor::from_fields(fields);
    assert(fixed_descriptor.resource().resource_id == "51966");
    assert(fixed_descriptor.resource().slot == "7");
    assert(fixed_descriptor.resource().generation == 42U);
    assert(fixed_descriptor.size_bytes() == 921600U);
    assert(fixed_descriptor.format() == "3");
    assert(fixed_descriptor.encoding() == "9");
    assert(fixed_descriptor.metadata().at("timestamp_unix_ns") == "1700000000");
    assert(fixed_descriptor.metadata().at("stride_bytes") == "1920");

    flowrt::FrameMetadata metadata;
    metadata.insert_or_assign("width", "640");
    metadata.insert_or_assign("height", "480");

    auto descriptor = flowrt::FrameDescriptor::make(
        flowrt::ResourceDescriptor{"camera_frames", "slot-7", 42U}, 640U * 480U * 3U,
        "rgb8", "row_major", metadata);

    assert(descriptor.resource().resource_id == "camera_frames");
    assert(descriptor.resource().slot == "slot-7");
    assert(descriptor.resource().generation == 42U);
    assert(descriptor.size_bytes() == 921600U);
    assert(descriptor.format() == "rgb8");
    assert(descriptor.encoding() == "row_major");
    assert(descriptor.metadata().at("width") == "640");

    flowrt::FrameLease lease{descriptor, 42U};
    assert(lease.status() == flowrt::FrameLeaseStatus::Attached);
    assert(lease.acquire(42U) == flowrt::FrameLeaseError::None);
    assert(lease.status() == flowrt::FrameLeaseStatus::Acquired);
    assert(lease.release() == flowrt::FrameLeaseError::None);
    assert(lease.status() == flowrt::FrameLeaseStatus::Released);
    assert(lease.acquire(42U) == flowrt::FrameLeaseError::Released);

    flowrt::FrameLease mismatch{descriptor, 43U};
    assert(mismatch.acquire(42U) == flowrt::FrameLeaseError::GenerationMismatch);
    assert(mismatch.status() == flowrt::FrameLeaseStatus::GenerationMismatch);

    flowrt::FrameLease expired{descriptor, 42U};
    expired.expire();
    assert(expired.acquire(42U) == flowrt::FrameLeaseError::Expired);
    assert(expired.status() == flowrt::FrameLeaseStatus::Expired);

    flowrt::FrameLease errored{descriptor, 42U};
    errored.fail("side channel unavailable");
    assert(errored.acquire(42U) == flowrt::FrameLeaseError::Error);
    assert(errored.status() == flowrt::FrameLeaseStatus::Error);
    assert(errored.last_error() == std::string{"side channel unavailable"});

    flowrt::IntrospectionState state;
    state.start_recorder(flowrt::IntrospectionRecorderStart{
        .output = std::nullopt,
        .filters = {"descriptor"},
        .queue_depth = 4,
        .package = "robot_demo",
        .process = "camera_proc",
        .runtime_pid = 42,
        .self_description_hash = "abc123",
    });
    flowrt::BoundaryContext context{
        "camera",
        "CameraDriver",
        {},
        [](flowrt::BoundaryStatus) {},
        [&state](std::string_view name, const flowrt::FrameDescriptor &descriptor,
                 flowrt::FrameLeaseStatus status, bool payload_recording,
                 std::optional<flowrt::FramePayloadArtifact> artifact) {
            const auto record = artifact.has_value()
                                    ? state.record_frame_descriptor_payload_event(
                                          name, descriptor, status, std::move(*artifact))
                                    : state.record_frame_descriptor_event(
                                          name, descriptor, status, payload_recording);
            return flowrt::BoundaryRecordOutcome{.recorded = record.recorded,
                                                 .dropped = record.dropped};
        }};
    const auto outcome = context.record_frame_descriptor_fields_event(
        "camera.frame", fields, flowrt::FrameLeaseStatus::Acquired, false);
    assert(outcome.recorded);
    assert(!outcome.dropped);
    const auto events = state.drain_recorder_events();
    assert(events.size() == 1U);
    const std::string payload{events.front().payload.begin(), events.front().payload.end()};
    assert(payload.find(R"("resource_id":"51966")") != std::string::npos);
    assert(payload.find(R"("slot":"7")") != std::string::npos);
    assert(payload.find(R"("width":"640")") != std::string::npos);
    assert(payload.find(R"("payload_recording":false)") != std::string::npos);

    const auto payload_outcome = context.record_frame_descriptor_fields_payload_event(
        "camera.frame", fields, flowrt::FrameLeaseStatus::Acquired,
        flowrt::FramePayloadArtifact{.artifact_ref = "artifact://camera/slot-7/42",
                                     .content_hash = "sha256:0123456789abcdef",
                                     .size_bytes = 921600U});
    assert(payload_outcome.recorded);
    assert(!payload_outcome.dropped);
    const auto payload_events = state.drain_recorder_events();
    assert(payload_events.size() == 1U);
    const std::string payload_artifact_json{payload_events.front().payload.begin(),
                                            payload_events.front().payload.end()};
    assert(payload_artifact_json.find(R"("payload_recording":true)") != std::string::npos);
    assert(payload_artifact_json.find(R"("payload_artifact":{)") != std::string::npos);
    assert(payload_artifact_json.find(R"("artifact_ref":"artifact://camera/slot-7/42")") !=
           std::string::npos);
    assert(payload_artifact_json.find(R"("content_hash":"sha256:0123456789abcdef")") !=
           std::string::npos);
    assert(payload_artifact_json.find(R"("size_bytes":921600)") != std::string::npos);

    return 0;
}
