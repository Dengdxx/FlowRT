use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::boundary_pub::{
    BoundaryPublishTarget, ensure_boundary_publish_endpoint, ensure_island_boundary_publish_mode,
};
use crate::introspection::{
    LOCAL_INTROSPECTION_TIMEOUT, ensure_handshake_hash, load_self_description_with_hash,
    select_echo_socket,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ReplayOutputFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ReplayOptions<'a> {
    pub(crate) file: &'a Path,
    pub(crate) image: &'a Path,
    pub(crate) socket: Option<&'a Path>,
    pub(crate) speed: f64,
    pub(crate) as_fast_as_possible: bool,
    pub(crate) verify_operation_observations: bool,
    pub(crate) format: ReplayOutputFormat,
}

#[derive(Debug, Clone)]
struct ReplayEvent {
    boundary: String,
    payload: String,
    at_ms: u64,
    source: ReplayEventSource,
    order: usize,
}

#[derive(Debug, Clone)]
enum ReplayEventSource {
    JsonArrayIndex(usize),
    JsonLine(usize),
}

impl ReplayEventSource {
    fn describe(&self, path: &Path) -> String {
        match self {
            Self::JsonArrayIndex(index) => format!("{} array index {index}", path.display()),
            Self::JsonLine(line) => format!("{} line {line}", path.display()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawReplayEvent {
    boundary: String,
    payload: Value,
    #[serde(default)]
    at_ms: Option<u64>,
    #[serde(default)]
    dt_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct NormalizedOperationObservation {
    operation: String,
    operation_id: String,
    kind: String,
    sequence: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    progress_sequence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Debug, Serialize)]
struct OperationObservationMismatch {
    operation: String,
    recorded_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    replay_id: Option<String>,
    kind: String,
    sequence: u64,
    reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected: Option<NormalizedOperationObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    observed: Option<NormalizedOperationObservation>,
}

#[derive(Debug, Serialize)]
struct OperationObservationVerificationReport {
    expected: usize,
    observed: usize,
    matched: usize,
    missing: usize,
    extra: usize,
    mismatched: usize,
    id_map: BTreeMap<String, String>,
    mismatches: Vec<OperationObservationMismatch>,
}

impl OperationObservationVerificationReport {
    fn is_success(&self) -> bool {
        self.missing == 0 && self.extra == 0 && self.mismatched == 0
    }

    fn summary_fields(&self) -> String {
        format!(
            "expected={} observed={} matched={} missing={} extra={} mismatched={}",
            self.expected, self.observed, self.matched, self.missing, self.extra, self.mismatched
        )
    }
}

struct OperationReplayOutput<'a> {
    file: &'a Path,
    operation_commands: usize,
    operations: usize,
    duration_ms: u64,
    speed: f64,
    mode: &'a str,
    verification: Option<&'a OperationObservationVerificationReport>,
}

#[cfg(test)]
pub(crate) fn replay_fixture(
    file: &Path,
    image: &Path,
    socket: Option<&Path>,
    speed: f64,
    as_fast_as_possible: bool,
) -> Result<String> {
    replay_fixture_with_options(ReplayOptions {
        file,
        image,
        socket,
        speed,
        as_fast_as_possible,
        verify_operation_observations: false,
        format: ReplayOutputFormat::Text,
    })
}

pub(crate) fn replay_fixture_with_options(options: ReplayOptions<'_>) -> Result<String> {
    let file = options.file;
    let image = options.image;
    let socket = options.socket;
    let speed = options.speed;
    let as_fast_as_possible = options.as_fast_as_possible;
    if speed <= 0.0 || !speed.is_finite() {
        anyhow::bail!("flowrt replay --speed must be a finite positive number");
    }
    if is_mcap_path(file) {
        return replay_operation_commands_from_mcap(options);
    }
    if options.verify_operation_observations {
        anyhow::bail!(
            "flowrt replay --verify-operation-observations requires an MCAP Operation recording"
        );
    }
    let mut events = replay_events_from_file(file)?;
    if events.is_empty() {
        anyhow::bail!(
            "flowrt replay `{}` does not contain any events",
            file.display()
        );
    }
    let (self_description, _) = load_self_description_with_hash(image)?;
    ensure_island_boundary_publish_mode(&self_description, "flowrt replay")?;
    for event in &events {
        ensure_boundary_publish_endpoint(&self_description, &event.boundary, "flowrt replay")
            .with_context(|| {
                format!(
                    "invalid replay boundary `{}` from {}",
                    event.boundary,
                    event.source.describe(file)
                )
            })?;
    }
    events.sort_by_key(|event| (event.at_ms, event.order));

    let target = BoundaryPublishTarget::open_for_command(image, socket, "flowrt replay")?;
    let start = events.first().map_or(0, |event| event.at_ms);
    let end = events.last().map_or(start, |event| event.at_ms);
    let replay_started = Instant::now();
    let mut sent = 0usize;
    let mut boundaries = BTreeSet::new();

    for event in &events {
        if !as_fast_as_possible {
            pace_replay_event(
                scaled_delay(event.at_ms.saturating_sub(start), speed),
                replay_started,
            );
        }
        target
            .publish_for_command(
                &event.boundary,
                &event.payload,
                Some(event.at_ms),
                "flowrt replay",
            )
            .with_context(|| {
                format!(
                    "failed to replay boundary `{}` from {}",
                    event.boundary,
                    event.source.describe(file)
                )
            })?;
        boundaries.insert(event.boundary.clone());
        sent += 1;
    }

    format_boundary_replay_output(
        file,
        sent,
        boundaries.len(),
        end,
        speed,
        replay_mode(as_fast_as_possible),
        options.format,
    )
}

fn replay_operation_commands_from_mcap(options: ReplayOptions<'_>) -> Result<String> {
    let file = options.file;
    let image = options.image;
    let socket = options.socket;
    let speed = options.speed;
    let as_fast_as_possible = options.as_fast_as_possible;
    let commands =
        flowrt_record::read_operation_command_timeline_from_path(file).with_context(|| {
            format!(
                "failed to read operation command replay `{}`",
                file.display()
            )
        })?;
    if commands.is_empty() {
        anyhow::bail!(
            "flowrt replay `{}` does not contain any Operation command events",
            file.display()
        );
    }
    let expected_observations = if options.verify_operation_observations {
        let trace =
            flowrt_record::read_operation_observation_trace_from_path(file).with_context(|| {
                format!(
                    "failed to read operation observation verification trace `{}`",
                    file.display()
                )
            })?;
        if trace.is_empty() {
            anyhow::bail!(
                "flowrt replay `{}` does not contain any Operation observation events; recording only has command timeline",
                file.display()
            );
        }
        Some(trace)
    } else {
        None
    };
    let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
    for command in &commands {
        ensure_operation_endpoint(&self_description, &command.operation)?;
    }
    let socket = select_echo_socket(socket, &self_description_hash)?;
    let start = commands.first().map_or(0, |event| event.time_ms);
    let end = commands.last().map_or(start, |event| event.time_ms);
    let replay_started = Instant::now();
    let mut operations = BTreeSet::new();
    let mut id_map = BTreeMap::new();

    for command in &commands {
        if !as_fast_as_possible {
            pace_replay_event(
                scaled_delay(command.time_ms.saturating_sub(start), speed),
                replay_started,
            );
        }
        operations.insert(command.operation.clone());
        match command.command {
            flowrt_record::OperationCommandKind::Start => {
                let payload = command.goal_payload.clone().ok_or_else(|| {
                    anyhow::anyhow!(
                        "operation start command `{}` is missing goal payload",
                        command.operation_id
                    )
                })?;
                let response = flowrt::request_operation_start_with_timeout(
                    &socket,
                    &command.operation,
                    payload,
                    command.timeout_ms,
                    command.owner.clone(),
                    LOCAL_INTROSPECTION_TIMEOUT,
                )
                .with_context(|| {
                    format!(
                        "failed to replay operation start `{}` from `{}`",
                        command.operation,
                        file.display()
                    )
                })?;
                match response {
                    flowrt::IntrospectionResponse::OperationStarted { handshake, started } => {
                        ensure_handshake_hash(&handshake, &self_description_hash, &socket)?;
                        id_map.insert(command.operation_id.clone(), started.operation_id);
                    }
                    flowrt::IntrospectionResponse::Error { message, .. } => {
                        anyhow::bail!(
                            "runtime rejected replay operation start `{}`: {message}",
                            command.operation
                        );
                    }
                    _ => anyhow::bail!(
                        "runtime socket `{}` returned an unexpected operation start response",
                        socket.display()
                    ),
                }
            }
            flowrt_record::OperationCommandKind::Cancel => {
                let actual_id = id_map
                    .get(&command.operation_id)
                    .map(String::as_str)
                    .unwrap_or(&command.operation_id);
                let response = flowrt::request_operation_cancel_with_timeout(
                    &socket,
                    actual_id,
                    LOCAL_INTROSPECTION_TIMEOUT,
                )
                .with_context(|| {
                    format!(
                        "failed to replay operation cancel `{}` from `{}`",
                        command.operation_id,
                        file.display()
                    )
                })?;
                match response {
                    flowrt::IntrospectionResponse::OperationValue { handshake, .. } => {
                        ensure_handshake_hash(&handshake, &self_description_hash, &socket)?;
                    }
                    flowrt::IntrospectionResponse::Error { message, .. } => {
                        anyhow::bail!(
                            "runtime rejected replay operation cancel `{}`: {message}",
                            command.operation_id
                        );
                    }
                    _ => anyhow::bail!(
                        "runtime socket `{}` returned an unexpected operation cancel response",
                        socket.display()
                    ),
                }
            }
        }
    }

    let verification = expected_observations
        .as_deref()
        .map(|expected| {
            verify_operation_observations(&socket, &self_description_hash, expected, &id_map)
        })
        .transpose()?;
    if let Some(report) = &verification
        && !report.is_success()
    {
        let output = format_operation_replay_output(
            OperationReplayOutput {
                file,
                operation_commands: commands.len(),
                operations: operations.len(),
                duration_ms: end,
                speed,
                mode: replay_mode(as_fast_as_possible),
                verification: verification.as_ref(),
            },
            options.format,
        )?;
        anyhow::bail!("{output}");
    }

    format_operation_replay_output(
        OperationReplayOutput {
            file,
            operation_commands: commands.len(),
            operations: operations.len(),
            duration_ms: end,
            speed,
            mode: replay_mode(as_fast_as_possible),
            verification: verification.as_ref(),
        },
        options.format,
    )
}

fn verify_operation_observations(
    socket: &Path,
    self_description_hash: &str,
    expected: &[flowrt_record::OperationObservationReplayEntry],
    id_map: &BTreeMap<String, String>,
) -> Result<OperationObservationVerificationReport> {
    let mut expected_by_recorded_id: BTreeMap<
        String,
        Vec<&flowrt_record::OperationObservationReplayEntry>,
    > = BTreeMap::new();
    for entry in expected {
        expected_by_recorded_id
            .entry(entry.operation_id.clone())
            .or_default()
            .push(entry);
    }

    let mut report = OperationObservationVerificationReport {
        expected: expected.len(),
        observed: 0,
        matched: 0,
        missing: 0,
        extra: 0,
        mismatched: 0,
        id_map: id_map.clone(),
        mismatches: Vec::new(),
    };

    for (recorded_id, expected_entries) in expected_by_recorded_id {
        let Some(replay_id) = id_map.get(&recorded_id) else {
            for expected_entry in expected_entries {
                report.missing += 1;
                report.mismatches.push(OperationObservationMismatch {
                    operation: expected_entry.operation.clone(),
                    recorded_id: recorded_id.clone(),
                    replay_id: None,
                    kind: expected_entry.kind.as_str().to_string(),
                    sequence: expected_entry.sequence,
                    reason: "recorded operation id has no replay invocation mapping".to_string(),
                    expected: Some(normalize_recorded_observation(expected_entry, &recorded_id)),
                    observed: None,
                });
            }
            continue;
        };
        let observed = collect_observed_operation_trace(socket, self_description_hash, replay_id)?;
        report.observed += observed.len();
        let expected_normalized = expected_entries
            .iter()
            .map(|entry| normalize_recorded_observation(entry, replay_id))
            .collect::<Vec<_>>();
        compare_operation_observation_trace(
            &mut report,
            &recorded_id,
            replay_id,
            &expected_normalized,
            &observed,
        );
    }

    Ok(report)
}

fn compare_operation_observation_trace(
    report: &mut OperationObservationVerificationReport,
    recorded_id: &str,
    replay_id: &str,
    expected: &[NormalizedOperationObservation],
    observed: &[NormalizedOperationObservation],
) {
    for index in 0..expected.len().max(observed.len()) {
        match (expected.get(index), observed.get(index)) {
            (Some(expected), Some(observed)) => {
                if expected == observed {
                    report.matched += 1;
                } else {
                    report.mismatched += 1;
                    report.mismatches.push(OperationObservationMismatch {
                        operation: expected.operation.clone(),
                        recorded_id: recorded_id.to_string(),
                        replay_id: Some(replay_id.to_string()),
                        kind: expected.kind.clone(),
                        sequence: expected.sequence,
                        reason: operation_observation_mismatch_reason(expected, observed),
                        expected: Some(expected.clone()),
                        observed: Some(observed.clone()),
                    });
                }
            }
            (Some(expected), None) => {
                report.missing += 1;
                report.mismatches.push(OperationObservationMismatch {
                    operation: expected.operation.clone(),
                    recorded_id: recorded_id.to_string(),
                    replay_id: Some(replay_id.to_string()),
                    kind: expected.kind.clone(),
                    sequence: expected.sequence,
                    reason: "expected observation event was not observed during replay".to_string(),
                    expected: Some(expected.clone()),
                    observed: None,
                });
            }
            (None, Some(observed)) => {
                report.extra += 1;
                report.mismatches.push(OperationObservationMismatch {
                    operation: observed.operation.clone(),
                    recorded_id: recorded_id.to_string(),
                    replay_id: Some(replay_id.to_string()),
                    kind: observed.kind.clone(),
                    sequence: observed.sequence,
                    reason: "replay emitted an extra observation event".to_string(),
                    expected: None,
                    observed: Some(observed.clone()),
                });
            }
            (None, None) => {}
        }
    }
}

fn operation_observation_mismatch_reason(
    expected: &NormalizedOperationObservation,
    observed: &NormalizedOperationObservation,
) -> String {
    let mut reasons = Vec::new();
    if expected.operation != observed.operation {
        reasons.push("operation");
    }
    if expected.operation_id != observed.operation_id {
        reasons.push("operation_id");
    }
    if expected.kind != observed.kind {
        reasons.push("kind");
    }
    if expected.sequence != observed.sequence {
        reasons.push("sequence");
    }
    if expected.state != observed.state {
        reasons.push("state");
    }
    if expected.progress_sequence != observed.progress_sequence {
        reasons.push("progress_sequence");
    }
    if expected.payload != observed.payload {
        reasons.push("payload");
    }
    if expected.message != observed.message {
        reasons.push("message");
    }
    format!("observation fields differ: {}", reasons.join(", "))
}

fn collect_observed_operation_trace(
    socket: &Path,
    self_description_hash: &str,
    replay_id: &str,
) -> Result<Vec<NormalizedOperationObservation>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut cursor = 0;
    let mut observed = Vec::new();
    loop {
        let response = flowrt::request_operation_observe_with_timeout(
            socket,
            replay_id,
            cursor,
            Some(64),
            LOCAL_INTROSPECTION_TIMEOUT,
        );
        match response {
            Ok(flowrt::IntrospectionResponse::OperationEvents {
                handshake,
                events,
                next_sequence,
                terminal,
                ..
            }) => {
                ensure_handshake_hash(&handshake, self_description_hash, socket)?;
                let event_count = events.len();
                observed.extend(events.into_iter().map(normalize_runtime_observation));
                cursor = next_sequence;
                if terminal && event_count < 64 {
                    break;
                }
                if Instant::now() >= deadline {
                    break;
                }
                if event_count == 0 {
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            Ok(flowrt::IntrospectionResponse::Error { message, .. }) => {
                if Instant::now() >= deadline {
                    anyhow::bail!(
                        "failed to observe replay operation `{}` from `{}`: {}",
                        replay_id,
                        socket.display(),
                        message
                    );
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(_) => {
                anyhow::bail!(
                    "runtime socket `{}` returned an unexpected operation observe response",
                    socket.display()
                );
            }
            Err(error) => {
                if Instant::now() >= deadline {
                    anyhow::bail!(
                        "failed to observe replay operation `{}` from `{}`: {}",
                        replay_id,
                        socket.display(),
                        error
                    );
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
    observed.sort_by_key(|event| event.sequence);
    Ok(observed)
}

fn normalize_recorded_observation(
    entry: &flowrt_record::OperationObservationReplayEntry,
    operation_id: &str,
) -> NormalizedOperationObservation {
    NormalizedOperationObservation {
        operation: entry.operation.clone(),
        operation_id: operation_id.to_string(),
        kind: entry.kind.as_str().to_string(),
        sequence: entry.sequence,
        state: entry.state.clone(),
        progress_sequence: entry.progress_sequence,
        payload: entry.payload.clone(),
        message: entry.message.clone(),
    }
}

fn normalize_runtime_observation(
    event: flowrt::IntrospectionOperationEvent,
) -> NormalizedOperationObservation {
    NormalizedOperationObservation {
        operation: event.operation,
        operation_id: event.operation_id,
        kind: event.kind,
        sequence: event.sequence,
        state: event.state,
        progress_sequence: event.progress_sequence,
        payload: event.payload,
        message: event.message,
    }
}

fn format_boundary_replay_output(
    file: &Path,
    events: usize,
    boundaries: usize,
    duration_ms: u64,
    speed: f64,
    mode: &str,
    format: ReplayOutputFormat,
) -> Result<String> {
    match format {
        ReplayOutputFormat::Text => Ok(format!(
            "replay source={} events={} boundaries={} duration_ms={} speed={} mode={}",
            file.display(),
            events,
            boundaries,
            duration_ms,
            speed,
            mode
        )),
        ReplayOutputFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
            "source": file.display().to_string(),
            "events": events,
            "boundaries": boundaries,
            "duration_ms": duration_ms,
            "speed": speed,
            "mode": mode,
        }))
        .context("failed to serialize replay JSON output"),
    }
}

fn format_operation_replay_output(
    summary: OperationReplayOutput<'_>,
    format: ReplayOutputFormat,
) -> Result<String> {
    match format {
        ReplayOutputFormat::Text => {
            let mut output = format!(
                "replay source={} operation_commands={} operations={} duration_ms={} speed={} mode={}",
                summary.file.display(),
                summary.operation_commands,
                summary.operations,
                summary.duration_ms,
                summary.speed,
                summary.mode
            );
            if let Some(report) = summary.verification {
                output.push_str(" operation_observation_verification ");
                output.push_str(&report.summary_fields());
            }
            Ok(output)
        }
        ReplayOutputFormat::Json => {
            let mut value = serde_json::json!({
                "source": summary.file.display().to_string(),
                "operation_commands": summary.operation_commands,
                "operations": summary.operations,
                "duration_ms": summary.duration_ms,
                "speed": summary.speed,
                "mode": summary.mode,
            });
            if let Some(report) = summary.verification {
                value["operation_observation_verification"] = serde_json::to_value(report)
                    .context("failed to serialize replay verification report")?;
            }
            serde_json::to_string_pretty(&value).context("failed to serialize replay JSON output")
        }
    }
}

fn replay_mode(as_fast_as_possible: bool) -> &'static str {
    if as_fast_as_possible {
        "as-fast-as-possible"
    } else {
        "paced"
    }
}

fn ensure_operation_endpoint(
    self_description: &flowrt_selfdesc::SelfDescription,
    operation_name: &str,
) -> Result<()> {
    if self_description
        .graphs
        .iter()
        .flat_map(|graph| graph.operations.iter())
        .any(|operation| operation.name == operation_name)
    {
        return Ok(());
    }
    anyhow::bail!("FlowRT self-description does not contain Operation `{operation_name}`")
}

fn replay_events_from_file(file: &Path) -> Result<Vec<ReplayEvent>> {
    if is_jsonl_path(file) {
        return replay_events_from_jsonl_file(file);
    }

    match replay_events_from_json_document(file) {
        Ok(events) => Ok(events),
        Err(json_error) => replay_events_from_jsonl_file(file).with_context(|| {
            format!(
                "failed to parse `{}` as replay JSON document: {json_error}",
                file.display()
            )
        }),
    }
}

fn replay_events_from_json_document(file: &Path) -> Result<Vec<ReplayEvent>> {
    let input = File::open(file)
        .with_context(|| format!("failed to open replay fixture `{}`", file.display()))?;
    let value = serde_json::from_reader::<_, Value>(input)
        .with_context(|| format!("failed to parse `{}` as JSON", file.display()))?;
    let Value::Array(values) = value else {
        anyhow::bail!(
            "flowrt replay `{}` JSON document must be an array",
            file.display()
        );
    };
    let mut current_ms = 0u64;
    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            let raw: RawReplayEvent = serde_json::from_value(value).with_context(|| {
                format!(
                    "flowrt replay `{}` array index {index} is invalid",
                    file.display()
                )
            })?;
            normalize_event(
                raw,
                ReplayEventSource::JsonArrayIndex(index),
                index,
                &mut current_ms,
            )
        })
        .collect()
}

fn replay_events_from_jsonl_file(file: &Path) -> Result<Vec<ReplayEvent>> {
    let input = File::open(file)
        .with_context(|| format!("failed to open replay fixture `{}`", file.display()))?;
    let reader = BufReader::new(input);
    let mut events = Vec::new();
    let mut current_ms = 0u64;
    for (index, line) in reader.lines().enumerate() {
        let line_number = index + 1;
        let line = line.with_context(|| {
            format!(
                "failed to read replay fixture `{}` line {line_number}",
                file.display()
            )
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let raw = serde_json::from_str::<RawReplayEvent>(trimmed).with_context(|| {
            format!(
                "flowrt replay JSONL entry `{}` line {line_number} must be valid JSON event",
                file.display()
            )
        })?;
        events.push(normalize_event(
            raw,
            ReplayEventSource::JsonLine(line_number),
            index,
            &mut current_ms,
        )?);
    }
    Ok(events)
}

fn normalize_event(
    raw: RawReplayEvent,
    source: ReplayEventSource,
    order: usize,
    current_ms: &mut u64,
) -> Result<ReplayEvent> {
    if raw.boundary.trim().is_empty() {
        anyhow::bail!("replay event boundary must not be empty");
    }
    if raw.at_ms.is_some() && raw.dt_ms.is_some() {
        anyhow::bail!("replay event must not set both at_ms and dt_ms");
    }
    let at_ms = if let Some(at_ms) = raw.at_ms {
        *current_ms = at_ms;
        at_ms
    } else if let Some(dt_ms) = raw.dt_ms {
        *current_ms = current_ms
            .checked_add(dt_ms)
            .context("replay event dt_ms overflows u64 timeline")?;
        *current_ms
    } else {
        *current_ms
    };
    Ok(ReplayEvent {
        boundary: raw.boundary,
        payload: serde_json::to_string(&raw.payload)
            .context("failed to serialize replay event payload")?,
        at_ms,
        source,
        order,
    })
}

fn scaled_delay(relative_ms: u64, speed: f64) -> Duration {
    Duration::from_secs_f64(relative_ms as f64 / 1000.0 / speed)
}

fn pace_replay_event(delay: Duration, replay_started: Instant) {
    let target = replay_started + delay;
    let now = Instant::now();
    if target > now {
        std::thread::sleep(target - now);
    }
}

fn is_jsonl_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
}

fn is_mcap_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mcap"))
}
