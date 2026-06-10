#include <cassert>
#include <flowrt/runtime.hpp>
#include <string>

int main() {
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

    return 0;
}
