use std::io::Cursor;

use flowrt_record::{
    FlowrtMcapWriter, PayloadEncoding, RECORD_SCHEMA_VERSION, RecordEntity, RecordEntityKind,
    RecordEnvelope, RecordError, RecordEventKind,
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
fn record_event_kind_covers_all_v0_6_record_scopes() {
    assert_eq!(
        RecordEventKind::ALL,
        [
            RecordEventKind::ChannelSample,
            RecordEventKind::ParamEvent,
            RecordEventKind::ServiceEvent,
            RecordEventKind::OperationEvent,
            RecordEventKind::SchedulerEvent,
            RecordEventKind::ClockEvent,
            RecordEventKind::RuntimeEvent,
        ]
    );
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
