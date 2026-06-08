use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use flowrt_record::{FlowrtMcapWriter, RecordChannel, RecordEnvelope, RecordEventKind};

/// `flowrt record` 的执行参数。
#[derive(Debug, Clone)]
pub(crate) struct RecordOptions {
    pub output: PathBuf,
    pub socket: Option<PathBuf>,
    pub duration: Option<Duration>,
    pub channels: Vec<String>,
    pub operations: Vec<String>,
    pub all: bool,
    pub force: bool,
    pub poll_interval: Duration,
    pub shutdown: flowrt::ShutdownToken,
}

#[derive(Debug, Default)]
struct RecordCounters {
    event_count: u64,
    dropped_count: u64,
    bytes_written: u64,
}

struct RecordWriterChannels {
    channel_sample: RecordChannel,
    param_event: RecordChannel,
    service_event: RecordChannel,
    operation_event: RecordChannel,
    scheduler_event: RecordChannel,
    clock_event: RecordChannel,
    runtime_event: RecordChannel,
}

impl RecordWriterChannels {
    fn register(writer: &mut FlowrtMcapWriter<File>) -> flowrt_record::RecordResult<Self> {
        Ok(Self {
            channel_sample: writer.register_channel(
                "flowrt/record/channel_sample",
                RecordEventKind::ChannelSample,
            )?,
            param_event: writer
                .register_channel("flowrt/record/param_event", RecordEventKind::ParamEvent)?,
            service_event: writer
                .register_channel("flowrt/record/service_event", RecordEventKind::ServiceEvent)?,
            operation_event: writer.register_channel(
                "flowrt/record/operation_event",
                RecordEventKind::OperationEvent,
            )?,
            scheduler_event: writer.register_channel(
                "flowrt/record/scheduler_event",
                RecordEventKind::SchedulerEvent,
            )?,
            clock_event: writer
                .register_channel("flowrt/record/clock_event", RecordEventKind::ClockEvent)?,
            runtime_event: writer
                .register_channel("flowrt/record/runtime_event", RecordEventKind::RuntimeEvent)?,
        })
    }

    const fn for_event_kind(&self, kind: RecordEventKind) -> RecordChannel {
        match kind {
            RecordEventKind::ChannelSample => self.channel_sample,
            RecordEventKind::ParamEvent => self.param_event,
            RecordEventKind::ServiceEvent => self.service_event,
            RecordEventKind::OperationEvent => self.operation_event,
            RecordEventKind::SchedulerEvent => self.scheduler_event,
            RecordEventKind::ClockEvent => self.clock_event,
            RecordEventKind::RuntimeEvent => self.runtime_event,
        }
    }
}

pub(crate) fn record_runtime(options: RecordOptions) -> Result<String> {
    let sockets = record_runtime_sockets_for_options(&options)?;
    record_runtime_for_sockets(options, sockets)
}

pub(crate) fn record_runtime_sockets_for_options(options: &RecordOptions) -> Result<Vec<PathBuf>> {
    if options.socket.is_some() {
        return Ok(Vec::new());
    }
    flowrt::discover_runtime_sockets().context("failed to scan FlowRT runtime sockets")
}

pub(crate) fn record_runtime_for_sockets(
    options: RecordOptions,
    sockets: Vec<PathBuf>,
) -> Result<String> {
    let socket = select_record_socket(options.socket.as_ref(), sockets)?;
    let filters = build_record_filters(&options)?;
    validate_record_filters(&socket, &options.channels, &options.operations)?;

    let mut output_file = RecordOutputFile::create(&options)?;
    let file = output_file.take_file();
    let mut writer = FlowrtMcapWriter::new(file).context("failed to create FlowRT MCAP writer")?;
    let channels = RecordWriterChannels::register(&mut writer)
        .context("failed to register FlowRT record MCAP channels")?;

    let started = flowrt::request_recorder_start(
        &socket,
        Some(options.output.display().to_string()),
        filters,
        Some(4096),
    )
    .with_context(|| format!("failed to start recorder on `{}`", socket.display()))?;
    let recorder = match started {
        flowrt::IntrospectionResponse::RecorderValue { recorder, .. } => recorder,
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!("runtime returned recorder start error: {message}");
        }
        _ => {
            anyhow::bail!(
                "runtime socket `{}` returned an unexpected recorder start response",
                socket.display()
            );
        }
    };
    if !recorder.enabled {
        anyhow::bail!(
            "runtime socket `{}` did not enable recorder",
            socket.display()
        );
    }
    let active_filters = recorder.active_filters.clone();

    let mut counters = RecordCounters::default();
    let started_at = Instant::now();
    let loop_result = (|| -> Result<()> {
        loop {
            drain_record_events(&socket, &mut writer, &channels, &mut counters)?;
            if options.shutdown.is_requested()
                || options
                    .duration
                    .is_some_and(|duration| started_at.elapsed() >= duration)
            {
                break Ok(());
            }
            std::thread::sleep(options.poll_interval);
        }
    })();

    let stop_result = stop_recorder(&socket);
    let final_drain = drain_record_events(&socket, &mut writer, &channels, &mut counters);

    loop_result?;
    let stopped = stop_result?;
    final_drain?;
    counters.dropped_count = counters.dropped_count.max(stopped.dropped_count);
    counters.bytes_written = counters.bytes_written.max(stopped.bytes_written);

    writer
        .flush()
        .context("failed to flush FlowRT MCAP writer")?;
    let _file = writer
        .finish_into_inner()
        .context("failed to finish FlowRT MCAP writer")?;
    drop(_file);
    output_file.commit(options.force)?;

    Ok(format!(
        "recorded output={} socket={} event_count={} dropped_count={} bytes_written={} active_filters=[{}]",
        options.output.display(),
        socket.display(),
        counters.event_count,
        counters.dropped_count,
        counters.bytes_written,
        active_filters.join(",")
    ))
}

fn select_record_socket(explicit: Option<&PathBuf>, sockets: Vec<PathBuf>) -> Result<PathBuf> {
    if let Some(socket) = explicit {
        return Ok(socket.clone());
    }

    let mut live = Vec::new();
    let mut errors = Vec::new();
    for socket in sockets {
        match flowrt::request_status(&socket) {
            Ok(flowrt::IntrospectionResponse::Status { .. }) => live.push(socket),
            Ok(flowrt::IntrospectionResponse::Error { message, .. }) => {
                errors.push(format!("{}: {message}", socket.display()));
            }
            Ok(_) => errors.push(format!(
                "{}: unexpected introspection response",
                socket.display()
            )),
            Err(error) => errors.push(format!("{}: {error}", socket.display())),
        }
    }

    match live.len() {
        0 => {
            if errors.is_empty() {
                anyhow::bail!("no live FlowRT processes; pass `--socket <path>`");
            }
            anyhow::bail!(
                "no live FlowRT processes; status errors: {}",
                errors.join("; ")
            );
        }
        1 => Ok(live.remove(0)),
        _ => {
            let sockets = live
                .iter()
                .map(|socket| socket.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!(
                "multiple live FlowRT processes: {sockets}; pass `--socket <path>` to choose one"
            );
        }
    }
}

fn build_record_filters(options: &RecordOptions) -> Result<Vec<String>> {
    if options.all && (!options.channels.is_empty() || !options.operations.is_empty()) {
        anyhow::bail!("`--all` cannot be combined with `--channel` or `--operation`");
    }
    if options.all || (options.channels.is_empty() && options.operations.is_empty()) {
        return Ok(vec!["all".to_string()]);
    }

    let mut filters = Vec::new();
    filters.extend(
        options
            .channels
            .iter()
            .map(|name| format!("channel:{name}")),
    );
    filters.extend(
        options
            .operations
            .iter()
            .map(|name| format!("operation:{name}")),
    );
    Ok(filters)
}

fn validate_record_filters(
    socket: &Path,
    channels: &[String],
    operations: &[String],
) -> Result<()> {
    if channels.is_empty() && operations.is_empty() {
        return Ok(());
    }
    let response = flowrt::request_status(socket)
        .with_context(|| format!("failed to request status from `{}`", socket.display()))?;
    let status = match response {
        flowrt::IntrospectionResponse::Status { status, .. } => status,
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!("runtime returned status error: {message}");
        }
        _ => {
            anyhow::bail!(
                "runtime socket `{}` returned an unexpected status response",
                socket.display()
            );
        }
    };
    for channel in channels {
        if !status.channels.iter().any(|entry| entry.name == *channel) {
            anyhow::bail!(
                "runtime socket `{}` does not report channel `{channel}`",
                socket.display()
            );
        }
    }
    for operation in operations {
        if !status
            .operations
            .iter()
            .any(|entry| entry.name == *operation)
        {
            anyhow::bail!(
                "runtime socket `{}` does not report operation `{operation}`",
                socket.display()
            );
        }
    }
    Ok(())
}

struct RecordOutputFile {
    final_path: PathBuf,
    temp_path: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl RecordOutputFile {
    fn create(options: &RecordOptions) -> Result<Self> {
        if options.output.exists() && !options.force {
            anyhow::bail!(
                "record output `{}` already exists; pass `--force` to overwrite",
                options.output.display()
            );
        }
        let parent = options
            .output
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if !parent.is_dir() {
            anyhow::bail!(
                "record output directory `{}` does not exist",
                parent.display()
            );
        }
        let file_name = options.output.file_name().with_context(|| {
            format!(
                "record output `{}` must name a file",
                options.output.display()
            )
        })?;
        let file_name = file_name.to_string_lossy();
        for attempt in 0..1000u32 {
            let temp_path = parent.join(format!(
                ".{file_name}.flowrt-record.tmp.{}.{}",
                std::process::id(),
                attempt
            ));
            let mut open = OpenOptions::new();
            open.write(true).create_new(true);
            match open.open(&temp_path) {
                Ok(file) => {
                    return Ok(Self {
                        final_path: options.output.clone(),
                        temp_path,
                        file: Some(file),
                        committed: false,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "failed to open temporary record output `{}`",
                            temp_path.display()
                        )
                    });
                }
            }
        }
        anyhow::bail!(
            "failed to allocate temporary record output for `{}`",
            options.output.display()
        )
    }

    fn take_file(&mut self) -> File {
        self.file
            .take()
            .expect("record output file should only be taken once")
    }

    fn commit(mut self, force: bool) -> Result<()> {
        drop(self.file.take());
        if force && self.final_path.exists() {
            fs::remove_file(&self.final_path).with_context(|| {
                format!(
                    "failed to replace existing record output `{}`",
                    self.final_path.display()
                )
            })?;
        }
        fs::rename(&self.temp_path, &self.final_path).with_context(|| {
            format!(
                "failed to move temporary record output `{}` to `{}`",
                self.temp_path.display(),
                self.final_path.display()
            )
        })?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for RecordOutputFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.temp_path);
        }
    }
}

fn drain_record_events(
    socket: &Path,
    writer: &mut FlowrtMcapWriter<File>,
    channels: &RecordWriterChannels,
    counters: &mut RecordCounters,
) -> Result<flowrt::IntrospectionRecorderStatus> {
    let response = flowrt::request_recorder_drain(socket).with_context(|| {
        format!(
            "failed to drain recorder events from `{}`",
            socket.display()
        )
    })?;
    match response {
        flowrt::IntrospectionResponse::RecorderEvents {
            recorder, events, ..
        } => {
            for event in events {
                write_record_event(writer, channels, &event)?;
                counters.event_count = counters.event_count.saturating_add(1);
            }
            counters.dropped_count = counters.dropped_count.max(recorder.dropped_count);
            counters.bytes_written = counters.bytes_written.max(recorder.bytes_written);
            Ok(recorder)
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!("runtime returned recorder error: {message}")
        }
        _ => anyhow::bail!(
            "runtime socket `{}` returned an unexpected recorder drain response",
            socket.display()
        ),
    }
}

fn write_record_event(
    writer: &mut FlowrtMcapWriter<File>,
    channels: &RecordWriterChannels,
    event: &RecordEnvelope,
) -> Result<()> {
    writer
        .write_event(channels.for_event_kind(event.event_kind), event)
        .context("failed to write FlowRT record event")
}

fn stop_recorder(socket: &Path) -> Result<flowrt::IntrospectionRecorderStatus> {
    let response = flowrt::request_recorder_stop(socket)
        .with_context(|| format!("failed to stop recorder on `{}`", socket.display()))?;
    match response {
        flowrt::IntrospectionResponse::RecorderValue { recorder, .. } => Ok(recorder),
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!("runtime returned recorder stop error: {message}")
        }
        _ => anyhow::bail!(
            "runtime socket `{}` returned an unexpected recorder stop response",
            socket.display()
        ),
    }
}
