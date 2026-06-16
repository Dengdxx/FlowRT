use std::collections::BTreeMap;
use std::io::Cursor;

use flowrt_record::{
    DescriptorRecordPayload, DescriptorRecordStatus, FlowrtMcapWriter, PayloadEncoding,
    RECORD_SCHEMA_VERSION, RecordEntity, RecordEntityKind, RecordEnvelope, RecordError,
    RecordEventKind, ReplayTimelineEntry, read_replay_timeline,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn sample_envelope(kind: RecordEventKind) -> RecordEnvelope {
    RecordEnvelope {
        schema_version: RECORD_SCHEMA_VERSION,
        event_kind: kind,
        package: "demo_pkg".to_string(),
        process: "perception".to_string(),
        runtime_pid: 4242,
        selfdesc_hash: "sha256:abc123".to_string(),
        monotonic_ns: 100,
        sample_time_ns: None,
        wall_unix_ns: 1_700_000_000_000_000_000,
        sequence: 7,
        entity: RecordEntity {
            kind: RecordEntityKind::Channel,
            name: "camera.frame_to_detector.frame".to_string(),
            instance: Some("camera".to_string()),
            task: Some("capture".to_string()),
            type_name: Some("Frame".to_string()),
        },
        payload_encoding: PayloadEncoding::RawAbi,
        payload_schema: "Frame".to_string(),
        payload: vec![1, 2, 3, 4],
    }
}

#[test]
fn record_envelope_serializes_with_language_neutral_snake_case_fields() -> TestResult {
    let envelope = sample_envelope(RecordEventKind::ChannelSample);

    let value = serde_json::to_value(&envelope)?;

    assert_eq!(value["schema_version"], RECORD_SCHEMA_VERSION);
    assert_eq!(value["event_kind"], "channel_sample");
    assert_eq!(value["payload_encoding"], "raw_abi");
    assert_eq!(value["entity"]["kind"], "channel");
    assert_eq!(value["entity"]["name"], "camera.frame_to_detector.frame");
    assert_eq!(value["payload"], serde_json::json!([1, 2, 3, 4]));

    let parsed: RecordEnvelope = serde_json::from_value(value)?;
    assert_eq!(parsed, envelope);

    Ok(())
}

#[test]
fn record_event_kind_covers_all_current_record_scopes() {
    assert_eq!(
        RecordEventKind::ALL,
        [
            RecordEventKind::ChannelSample,
            RecordEventKind::DescriptorEvent,
            RecordEventKind::ParamEvent,
            RecordEventKind::ServiceEvent,
            RecordEventKind::OperationEvent,
            RecordEventKind::SchedulerEvent,
            RecordEventKind::DiagnosticsEvent,
            RecordEventKind::ClockEvent,
            RecordEventKind::RuntimeEvent,
        ]
    );
}

#[test]
fn read_replay_timeline_extracts_sorted_channel_samples() -> TestResult {
    let mut writer = FlowrtMcapWriter::new(Cursor::new(Vec::new()))?;
    let samples = writer.register_channel("samples", RecordEventKind::ChannelSample)?;
    let scheduler = writer.register_channel("scheduler", RecordEventKind::SchedulerEvent)?;

    // 乱序写入两个 channel sample，外加一个非 sample 事件验证过滤。
    let mut later = sample_envelope(RecordEventKind::ChannelSample);
    later.monotonic_ns = 20_000_000;
    later.sequence = 2;
    later.entity.name = "sensor.b".to_string();
    later.payload = vec![2, 2];
    let mut earlier = sample_envelope(RecordEventKind::ChannelSample);
    earlier.monotonic_ns = 5_000_000;
    earlier.sequence = 1;
    earlier.entity.name = "sensor.a".to_string();
    earlier.payload = vec![1];
    let mut sched = sample_envelope(RecordEventKind::SchedulerEvent);
    sched.monotonic_ns = 1_000_000;

    writer.write_event(samples, &later)?;
    writer.write_event(samples, &earlier)?;
    writer.write_event(scheduler, &sched)?;
    writer.flush()?;
    let bytes = writer.finish_into_inner()?.into_inner();

    let timeline = read_replay_timeline(&bytes)?;
    assert_eq!(
        timeline,
        vec![
            ReplayTimelineEntry {
                time_ms: 5,
                target: "sensor.a".to_string(),
                payload: vec![1],
                sample_time_ms: None,
            },
            ReplayTimelineEntry {
                time_ms: 20,
                target: "sensor.b".to_string(),
                payload: vec![2, 2],
                sample_time_ms: None,
            },
        ]
    );
    Ok(())
}

#[test]
fn read_replay_timeline_uses_sample_time_when_present() -> TestResult {
    let mut writer = FlowrtMcapWriter::new(Cursor::new(Vec::new()))?;
    let samples = writer.register_channel("samples", RecordEventKind::ChannelSample)?;
    // receive-time(monotonic) 顺序 a 早于 b，但 sample-time 相反 → 时间线按 sample-time 排序。
    let mut a = sample_envelope(RecordEventKind::ChannelSample);
    a.monotonic_ns = 5_000_000;
    a.sample_time_ns = Some(200_000_000);
    a.sequence = 1;
    a.entity.name = "sensor.a".to_string();
    a.payload = vec![1];
    let mut b = sample_envelope(RecordEventKind::ChannelSample);
    b.monotonic_ns = 9_000_000;
    b.sample_time_ns = Some(100_000_000);
    b.sequence = 2;
    b.entity.name = "sensor.b".to_string();
    b.payload = vec![2];
    writer.write_event(samples, &a)?;
    writer.write_event(samples, &b)?;
    writer.flush()?;
    let bytes = writer.finish_into_inner()?.into_inner();

    let timeline = read_replay_timeline(&bytes)?;
    assert_eq!(timeline.len(), 2);
    // 按 sample-time（event-time）排序：b(100ms) 在 a(200ms) 之前。
    assert_eq!(timeline[0].target, "sensor.b");
    assert_eq!(timeline[0].sample_time_ms, Some(100));
    assert_eq!(timeline[0].effective_time_ms(), 100);
    assert_eq!(timeline[1].target, "sensor.a");
    assert_eq!(timeline[1].sample_time_ms, Some(200));
    Ok(())
}

#[test]
fn descriptor_record_payload_serializes_descriptor_without_payload_bytes() -> TestResult {
    let descriptor = DescriptorRecordPayload {
        resource_id: "camera_frames".to_string(),
        slot: "slot-7".to_string(),
        generation: 42,
        size_bytes: 921_600,
        format: "rgb8".to_string(),
        encoding: "row_major".to_string(),
        metadata: BTreeMap::from([
            ("height".to_string(), "480".to_string()),
            ("width".to_string(), "640".to_string()),
        ]),
        status: DescriptorRecordStatus::Acquired,
        payload_recording: false,
    };
    let payload = serde_json::to_vec(&descriptor)?;
    let envelope = RecordEnvelope {
        event_kind: RecordEventKind::DescriptorEvent,
        entity: RecordEntity {
            kind: RecordEntityKind::Resource,
            name: "camera_frames".to_string(),
            instance: Some("camera".to_string()),
            task: Some("capture".to_string()),
            type_name: Some("FrameDescriptor".to_string()),
        },
        payload_encoding: PayloadEncoding::Json,
        payload_schema: "flowrt.descriptor.frame.v1".to_string(),
        payload,
        ..sample_envelope(RecordEventKind::DescriptorEvent)
    };

    let value = serde_json::to_value(&envelope)?;
    assert_eq!(value["event_kind"], "descriptor_event");
    assert_eq!(value["entity"]["kind"], "resource");
    assert_eq!(value["payload_encoding"], "json");

    let decoded: DescriptorRecordPayload = serde_json::from_slice(&envelope.payload)?;
    assert_eq!(decoded.resource_id, "camera_frames");
    assert_eq!(decoded.slot, "slot-7");
    assert_eq!(decoded.generation, 42);
    assert_eq!(decoded.size_bytes, 921_600);
    assert_eq!(decoded.format, "rgb8");
    assert_eq!(decoded.encoding, "row_major");
    assert_eq!(decoded.metadata.get("width"), Some(&"640".to_string()));
    assert_eq!(decoded.status, DescriptorRecordStatus::Acquired);
    assert!(!decoded.payload_recording);

    Ok(())
}

#[test]
fn mcap_writer_writes_minimal_file_with_flowrt_schema_and_event() -> TestResult {
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlowrtMcapWriter::new(cursor)?;
    let channel = writer.register_channel(
        "flowrt/record/channel_sample",
        RecordEventKind::ChannelSample,
    )?;

    writer.write_event(channel, &sample_envelope(RecordEventKind::ChannelSample))?;
    writer.flush()?;
    let cursor = writer.finish_into_inner()?;
    let bytes = cursor.into_inner();

    assert!(bytes.starts_with(flowrt_record::MCAP_MAGIC));
    assert!(bytes.ends_with(flowrt_record::MCAP_MAGIC));

    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("flowrt.record.v1"));
    assert!(text.contains("channel_sample"));
    assert!(text.contains("camera.frame_to_detector.frame"));

    Ok(())
}

#[test]
fn mcap_writer_rejects_sequence_values_that_do_not_fit_mcap_header() -> TestResult {
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlowrtMcapWriter::new(cursor)?;
    let channel =
        writer.register_channel("flowrt/record/runtime_event", RecordEventKind::RuntimeEvent)?;
    let mut envelope = sample_envelope(RecordEventKind::RuntimeEvent);
    envelope.sequence = u64::from(u32::MAX) + 1;

    let error = writer
        .write_event(channel, &envelope)
        .expect_err("sequence must be checked");

    assert!(matches!(
        error,
        RecordError::SequenceTooLarge(value) if value == u64::from(u32::MAX) + 1
    ));

    Ok(())
}

#[test]
fn mcap_writer_rejects_event_kind_mismatch_between_channel_and_envelope() -> TestResult {
    let cursor = Cursor::new(Vec::new());
    let mut writer = FlowrtMcapWriter::new(cursor)?;
    let channel =
        writer.register_channel("flowrt/record/runtime_event", RecordEventKind::RuntimeEvent)?;
    let envelope = sample_envelope(RecordEventKind::ChannelSample);

    let error = writer
        .write_event(channel, &envelope)
        .expect_err("event kind mismatch must be rejected");

    assert!(matches!(
        error,
        RecordError::EventKindMismatch {
            channel: RecordEventKind::RuntimeEvent,
            envelope: RecordEventKind::ChannelSample
        }
    ));

    Ok(())
}
