use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::boundary_pub::{
    BoundaryPublishTarget, ensure_boundary_publish_endpoint, ensure_island_boundary_publish_mode,
};
use crate::introspection::load_self_description_with_hash;

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

pub(crate) fn replay_fixture(
    file: &Path,
    image: &Path,
    socket: Option<&Path>,
    speed: f64,
    as_fast_as_possible: bool,
) -> Result<String> {
    if speed <= 0.0 || !speed.is_finite() {
        anyhow::bail!("flowrt replay --speed must be a finite positive number");
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

    Ok(format!(
        "replay source={} events={} boundaries={} duration_ms={} speed={} mode={}",
        file.display(),
        sent,
        boundaries.len(),
        end,
        speed,
        if as_fast_as_possible {
            "as-fast-as-possible"
        } else {
            "paced"
        }
    ))
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
