use flowrt::{
    BoundaryContext, FrameDescriptor, FrameDescriptorFields, FrameLease, FrameLeaseError,
    FrameLeaseStatus, FrameMetadata, IntrospectionRecorderStart, IntrospectionState,
    ResourceDescriptor,
};
use flowrt_record::DescriptorRecordPayload;

#[test]
fn frame_descriptor_carries_resource_identity_layout_and_metadata() {
    let mut metadata = FrameMetadata::new();
    metadata.insert("width".to_string(), "640".to_string());
    metadata.insert("height".to_string(), "480".to_string());
    metadata.insert("stride".to_string(), "1920".to_string());

    let descriptor = FrameDescriptor::new(
        ResourceDescriptor::new("camera_frames", "slot-7", 42),
        640 * 480 * 3,
        "rgb8",
        "row_major",
        metadata.clone(),
    )
    .unwrap();

    assert_eq!(descriptor.resource().resource_id(), "camera_frames");
    assert_eq!(descriptor.resource().slot(), "slot-7");
    assert_eq!(descriptor.resource().generation(), 42);
    assert_eq!(descriptor.size_bytes(), 921_600);
    assert_eq!(descriptor.format(), "rgb8");
    assert_eq!(descriptor.encoding(), "row_major");
    assert_eq!(descriptor.metadata().get("width"), Some(&"640".to_string()));
    assert_eq!(descriptor.metadata(), &metadata);
}

#[test]
fn fixed_frame_descriptor_fields_convert_to_runtime_descriptor() {
    assert_eq!(std::mem::size_of::<FrameDescriptorFields>(), 64);
    assert_eq!(std::mem::align_of::<FrameDescriptorFields>(), 8);
    assert_eq!(
        std::mem::offset_of!(FrameDescriptorFields, resource_id_hash),
        0
    );
    assert_eq!(std::mem::offset_of!(FrameDescriptorFields, slot), 8);
    assert_eq!(std::mem::offset_of!(FrameDescriptorFields, generation), 16);
    assert_eq!(std::mem::offset_of!(FrameDescriptorFields, flags), 60);

    let fields = FrameDescriptorFields {
        resource_id_hash: 0xCAFE,
        slot: 7,
        generation: 42,
        size_bytes: 921_600,
        timestamp_unix_ns: 1_700_000_000,
        width: 640,
        height: 480,
        stride_bytes: 1_920,
        format_id: 3,
        encoding_id: 9,
        flags: 1,
    };

    let descriptor = fields.to_descriptor().unwrap();

    assert_eq!(descriptor.resource().resource_id(), "51966");
    assert_eq!(descriptor.resource().slot(), "7");
    assert_eq!(descriptor.resource().generation(), 42);
    assert_eq!(descriptor.size_bytes(), 921_600);
    assert_eq!(descriptor.format(), "3");
    assert_eq!(descriptor.encoding(), "9");
    assert_eq!(
        descriptor.metadata().get("timestamp_unix_ns"),
        Some(&"1700000000".to_string())
    );
    assert_eq!(
        descriptor.metadata().get("stride_bytes"),
        Some(&"1920".to_string())
    );
}

#[test]
fn frame_descriptor_recorder_records_descriptor_event_without_payload_copy() {
    let descriptor = FrameDescriptor::new(
        ResourceDescriptor::new("camera_frames", "slot-7", 42),
        921_600,
        "rgb8",
        "row_major",
        FrameMetadata::from([
            ("height".to_string(), "480".to_string()),
            ("width".to_string(), "640".to_string()),
        ]),
    )
    .unwrap();
    let state = IntrospectionState::new();
    state.start_recorder(IntrospectionRecorderStart {
        output: None,
        filters: vec!["descriptor".to_string()],
        queue_depth: Some(4),
        package: "robot_demo".to_string(),
        process: "camera_proc".to_string(),
        runtime_pid: 42,
        selfdesc_hash: "abc123".to_string(),
    });

    let outcome = state.record_frame_descriptor_event(
        "camera.frame",
        &descriptor,
        FrameLeaseStatus::Acquired,
        false,
    );

    assert!(outcome.recorded);
    assert!(!outcome.dropped);
    let events = state.drain_recorder_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_kind.as_str(), "descriptor_event");
    assert_eq!(events[0].payload_schema, "flowrt.descriptor.frame.v1");
    let payload: DescriptorRecordPayload = serde_json::from_slice(&events[0].payload).unwrap();
    assert_eq!(payload.resource_id, "camera_frames");
    assert_eq!(payload.slot, "slot-7");
    assert_eq!(payload.generation, 42);
    assert_eq!(
        payload.status,
        flowrt_record::DescriptorRecordStatus::Acquired
    );
    assert!(!payload.payload_recording);
}

#[test]
fn boundary_context_records_fixed_frame_descriptor_fields_without_payload_copy() {
    let state = IntrospectionState::new();
    state.start_recorder(IntrospectionRecorderStart {
        output: None,
        filters: vec!["descriptor".to_string()],
        queue_depth: Some(4),
        package: "robot_demo".to_string(),
        process: "camera_proc".to_string(),
        runtime_pid: 42,
        selfdesc_hash: "abc123".to_string(),
    });
    let context = BoundaryContext::new("camera", "CameraDriver", state.clone());

    let outcome = context
        .record_frame_descriptor_fields_event(
            "camera.frame",
            FrameDescriptorFields {
                resource_id_hash: 0xCAFE,
                slot: 7,
                generation: 42,
                size_bytes: 921_600,
                timestamp_unix_ns: 1_700_000_000,
                width: 640,
                height: 480,
                stride_bytes: 1_920,
                format_id: 3,
                encoding_id: 9,
                flags: 1,
            },
            FrameLeaseStatus::Acquired,
            false,
        )
        .unwrap();

    assert!(outcome.recorded);
    assert!(!outcome.dropped);
    let events = state.drain_recorder_events();
    let payload: DescriptorRecordPayload = serde_json::from_slice(&events[0].payload).unwrap();
    assert_eq!(payload.resource_id, "51966");
    assert_eq!(payload.slot, "7");
    assert_eq!(payload.generation, 42);
    assert_eq!(payload.metadata.get("width"), Some(&"640".to_string()));
    assert!(!payload.payload_recording);
}

#[test]
fn frame_descriptor_fake_lease_reports_attached_acquired_released_and_errors() {
    let descriptor = FrameDescriptor::new(
        ResourceDescriptor::new("camera_frames", "slot-3", 8),
        16,
        "mask8",
        "row_major",
        FrameMetadata::new(),
    )
    .unwrap();
    let mut lease = FrameLease::attach(descriptor.clone(), 8);

    assert_eq!(lease.status(), FrameLeaseStatus::Attached);
    lease.acquire(8).unwrap();
    assert_eq!(lease.status(), FrameLeaseStatus::Acquired);
    lease.release().unwrap();
    assert_eq!(lease.status(), FrameLeaseStatus::Released);

    let error = lease
        .acquire(8)
        .expect_err("released leases cannot be reacquired");
    assert_eq!(error, FrameLeaseError::Released);
    assert_eq!(lease.status(), FrameLeaseStatus::Released);

    let mut mismatch = FrameLease::attach(descriptor, 9);
    assert_eq!(
        mismatch.acquire(8),
        Err(FrameLeaseError::GenerationMismatch {
            descriptor_generation: 8,
            current_generation: 9,
        })
    );
    assert_eq!(mismatch.status(), FrameLeaseStatus::GenerationMismatch);
}

#[test]
fn frame_descriptor_fake_lease_distinguishes_expired_and_generic_error_status() {
    let descriptor = FrameDescriptor::new(
        ResourceDescriptor::new("camera_frames", "slot-5", 11),
        32,
        "tensor_f32",
        "nchw",
        FrameMetadata::new(),
    )
    .unwrap();

    let mut expired = FrameLease::attach(descriptor.clone(), 11);
    expired.expire();
    assert_eq!(expired.acquire(11), Err(FrameLeaseError::Expired));
    assert_eq!(expired.status(), FrameLeaseStatus::Expired);

    let mut errored = FrameLease::attach(descriptor, 11);
    errored.fail("side channel unavailable");
    assert_eq!(
        errored.acquire(11),
        Err(FrameLeaseError::Error(
            "side channel unavailable".to_string()
        ))
    );
    assert_eq!(errored.status(), FrameLeaseStatus::Error);
    assert_eq!(errored.last_error(), Some("side channel unavailable"));
}
