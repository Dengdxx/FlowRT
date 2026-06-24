use super::*;
use std::process::Stdio;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const MANAGED_RELEASE_METADATA: &str = "flowrt-managed-release.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ManagedActiveState {
    pub(crate) schema_version: u32,
    pub(crate) current_release: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) previous_release: Option<String>,
    pub(crate) updated_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ManagedReleaseMetadata {
    pub(crate) schema_version: u32,
    pub(crate) release_id: String,
    pub(crate) package: String,
    pub(crate) profile: Option<String>,
    pub(crate) target: String,
    pub(crate) platform: Option<String>,
    pub(crate) build_mode: BuildMode,
    pub(crate) entry: String,
    pub(crate) installed_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedInstallSummary {
    pub(crate) release_id: String,
    pub(crate) path: PathBuf,
    pub(crate) message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedRollbackSummary {
    pub(crate) release_id: String,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ManagedRunState {
    pub(crate) schema_version: u32,
    pub(crate) run_id: String,
    pub(crate) release_id: String,
    pub(crate) state: String,
    pub(crate) supervisor_pid: u32,
    pub(crate) log_path: PathBuf,
    pub(crate) started_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) stopped_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) stop_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct ManagedStatus {
    pub(crate) active: ManagedActiveState,
    pub(crate) run: Option<ManagedRunState>,
    pub(crate) releases: Vec<String>,
}

pub(crate) fn managed_status_output(
    remote_dir: &Path,
    format: crate::StatusFormat,
) -> Result<String> {
    let status = managed_status(remote_dir)?;
    match format {
        crate::StatusFormat::Text => Ok(managed_status_text(&status)),
        crate::StatusFormat::Json => Ok(format!("{}\n", serde_json::to_string_pretty(&status)?)),
    }
}

pub(crate) fn remote_activate(
    host: &str,
    remote_dir: &str,
    release: &str,
    dry_run: bool,
) -> Result<String> {
    remote_managed_command(
        host,
        [
            "managed",
            "activate",
            "--remote-dir",
            remote_dir,
            "--release",
            release,
        ],
        dry_run,
    )
}

pub(crate) fn remote_start(
    host: &str,
    remote_dir: &str,
    release: Option<&str>,
    dry_run: bool,
) -> Result<String> {
    let mut args = vec!["managed", "start", "--remote-dir", remote_dir];
    if let Some(release) = release {
        args.push("--release");
        args.push(release);
    }
    remote_managed_command(host, args, dry_run)
}

pub(crate) fn remote_stop(
    host: &str,
    remote_dir: &str,
    timeout_ms: u64,
    dry_run: bool,
) -> Result<String> {
    let timeout = timeout_ms.to_string();
    remote_managed_command(
        host,
        [
            "managed",
            "stop",
            "--remote-dir",
            remote_dir,
            "--timeout-ms",
            timeout.as_str(),
        ],
        dry_run,
    )
}

pub(crate) fn remote_status(
    host: &str,
    remote_dir: &str,
    format: crate::StatusFormat,
) -> Result<String> {
    let format = status_format_name(format);
    remote_managed_command(
        host,
        [
            "managed",
            "status",
            "--remote-dir",
            remote_dir,
            "--format",
            format,
        ],
        false,
    )
}

pub(crate) fn remote_logs(host: &str, remote_dir: &str, lines: usize) -> Result<String> {
    let lines = lines.to_string();
    remote_managed_command(
        host,
        [
            "managed",
            "logs",
            "--remote-dir",
            remote_dir,
            "--lines",
            lines.as_str(),
        ],
        false,
    )
}

pub(crate) fn remote_rollback(
    host: &str,
    remote_dir: &str,
    start: bool,
    dry_run: bool,
) -> Result<String> {
    let mut args = vec!["managed", "rollback", "--remote-dir", remote_dir];
    if start {
        args.push("--start");
    }
    remote_managed_command(host, args, dry_run)
}

pub(crate) fn managed_start(remote_dir: &Path, release: Option<&str>) -> Result<ManagedRunState> {
    if let Ok(existing) = load_managed_run(remote_dir)
        && existing.state == "running"
        && process_alive(existing.supervisor_pid)
    {
        anyhow::bail!(
            "managed FlowRT release `{}` is already running with pid {}",
            existing.release_id,
            existing.supervisor_pid
        );
    }
    let active = match release {
        Some(release) => managed_activate(remote_dir, release)?,
        None => load_managed_active(remote_dir)?,
    };
    let release_path = managed_release_path(remote_dir, &active.current_release);
    let metadata = load_managed_release_metadata(&release_path)?;
    let entry = PathBuf::from(&metadata.entry);
    ensure_safe_relative_path(&entry)?;
    let executable = release_path.join(&entry);
    if !executable.is_file() {
        anyhow::bail!(
            "managed release `{}` entry `{}` does not exist",
            active.current_release,
            executable.display()
        );
    }

    let run_id = format!("{}-{}", active.current_release, current_unix_ms());
    let log_path = remote_dir.join("logs").join(&run_id).join("supervisor.log");
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open managed log `{}`", log_path.display()))?;
    let stderr = log
        .try_clone()
        .with_context(|| format!("failed to clone managed log `{}`", log_path.display()))?;
    let mut command = ProcessCommand::new(&executable);
    command
        .current_dir(&release_path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr));
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn().with_context(|| {
        format!(
            "failed to start managed FlowRT release `{}` entry `{}`",
            active.current_release,
            executable.display()
        )
    })?;
    let run = ManagedRunState {
        schema_version: 1,
        run_id,
        release_id: active.current_release,
        state: "running".to_string(),
        supervisor_pid: child.id(),
        log_path,
        started_unix_ms: current_unix_ms(),
        stopped_unix_ms: None,
        stop_reason: None,
    };
    write_json_atomic(&managed_run_path(remote_dir), &run)?;
    Ok(run)
}

pub(crate) fn managed_stop(remote_dir: &Path, timeout: Duration) -> Result<String> {
    let mut run = load_managed_run(remote_dir)?;
    if run.state != "running" || !process_alive(run.supervisor_pid) {
        run.state = "exited".to_string();
        run.stopped_unix_ms = Some(current_unix_ms());
        run.stop_reason = Some("not_running".to_string());
        write_json_atomic(&managed_run_path(remote_dir), &run)?;
        return Ok(format!(
            "managed FlowRT release {} was not running",
            run.release_id
        ));
    }
    terminate_process(run.supervisor_pid, libc::SIGTERM);
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !process_alive(run.supervisor_pid) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if process_alive(run.supervisor_pid) {
        terminate_process(run.supervisor_pid, libc::SIGKILL);
    }
    run.state = "stopped".to_string();
    run.stopped_unix_ms = Some(current_unix_ms());
    run.stop_reason = Some("requested".to_string());
    write_json_atomic(&managed_run_path(remote_dir), &run)?;
    Ok(format!(
        "stopped FlowRT release {} pid={}",
        run.release_id, run.supervisor_pid
    ))
}

pub(crate) fn managed_status(remote_dir: &Path) -> Result<ManagedStatus> {
    let active = load_managed_active(remote_dir)?;
    let mut run = load_managed_run(remote_dir).ok();
    if let Some(run) = &mut run
        && run.state == "running"
        && !process_alive(run.supervisor_pid)
    {
        run.state = "exited".to_string();
    }
    Ok(ManagedStatus {
        active,
        run,
        releases: managed_installed_releases(remote_dir)?,
    })
}

pub(crate) fn managed_logs(remote_dir: &Path, lines: usize) -> Result<String> {
    let run = load_managed_run(remote_dir)?;
    let text = fs::read_to_string(&run.log_path)
        .with_context(|| format!("failed to read managed log `{}`", run.log_path.display()))?;
    let mut selected = text.lines().rev().take(lines).collect::<Vec<_>>();
    selected.reverse();
    Ok(format!("{}\n", selected.join("\n")))
}

fn managed_status_text(status: &ManagedStatus) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "managed_active release={} previous={}",
        status.active.current_release,
        status.active.previous_release.as_deref().unwrap_or("none")
    ));
    if let Some(run) = &status.run {
        lines.push(format!(
            "managed_run state={} release={} pid={} log={}",
            run.state,
            run.release_id,
            run.supervisor_pid,
            run.log_path.display()
        ));
    } else {
        lines.push("managed_run state=none".to_string());
    }
    lines.push(format!("managed_releases [{}]", status.releases.join(",")));
    format!("{}\n", lines.join("\n"))
}

pub(crate) fn managed_install(
    bundle: &Path,
    remote_dir: &Path,
    target: &str,
    activate: bool,
) -> Result<ManagedInstallSummary> {
    let loaded = load_bundle_manifest(bundle)?;
    let manifest = loaded.manifest;
    ensure_artifact_allowed(
        &manifest.artifact_mode,
        manifest.temporary_overlay,
        manifest.test_only,
        false,
        "install",
    )?;
    select_deploy_artifacts(bundle, &manifest, target)?;
    let release_id = managed_release_id(&manifest, target);
    let release_path = managed_release_path(remote_dir, &release_id);
    if !release_path.exists() {
        let tmp_path = remote_dir
            .join("releases")
            .join(format!(".{release_id}.tmp-{}", std::process::id()));
        if tmp_path.exists() {
            fs::remove_dir_all(&tmp_path).with_context(|| {
                format!(
                    "failed to remove stale managed release temp dir `{}`",
                    tmp_path.display()
                )
            })?;
        }
        copy_dir_recursive(bundle, &tmp_path)?;
        write_managed_release_metadata(&tmp_path, &manifest, target, &release_id)?;
        fs::rename(&tmp_path, &release_path).with_context(|| {
            format!(
                "failed to publish managed release `{}` to `{}`",
                release_id,
                release_path.display()
            )
        })?;
    }
    if activate {
        managed_activate(remote_dir, &release_id)?;
    }
    Ok(ManagedInstallSummary {
        release_id: release_id.clone(),
        path: release_path,
        message: format!("installed FlowRT release {release_id}"),
    })
}

pub(crate) fn managed_activate(remote_dir: &Path, release_id: &str) -> Result<ManagedActiveState> {
    let release_path = managed_release_path(remote_dir, release_id);
    if !release_path.is_dir() {
        anyhow::bail!(
            "managed release `{release_id}` is not installed under `{}`",
            remote_dir.display()
        );
    }
    let previous = load_managed_active(remote_dir).ok();
    let previous_release = previous.as_ref().and_then(|active| {
        (active.current_release != release_id).then(|| active.current_release.clone())
    });
    let active = ManagedActiveState {
        schema_version: 1,
        current_release: release_id.to_string(),
        previous_release,
        updated_unix_ms: current_unix_ms(),
    };
    write_json_atomic(&managed_active_path(remote_dir), &active)?;
    Ok(active)
}

pub(crate) fn managed_rollback(
    remote_dir: &Path,
    start_after_rollback: bool,
) -> Result<ManagedRollbackSummary> {
    let active = load_managed_active(remote_dir)?;
    let previous = active
        .previous_release
        .clone()
        .context("managed rollback requires a previous release")?;
    let next_previous = active.current_release.clone();
    if !managed_release_path(remote_dir, &previous).is_dir() {
        anyhow::bail!("previous managed release `{previous}` is missing");
    }
    if start_after_rollback
        && let Ok(run) = load_managed_run(remote_dir)
        && run.state == "running"
        && process_alive(run.supervisor_pid)
    {
        managed_stop(remote_dir, Duration::from_millis(5000))?;
    }
    let rolled_back = ManagedActiveState {
        schema_version: 1,
        current_release: previous.clone(),
        previous_release: Some(next_previous),
        updated_unix_ms: current_unix_ms(),
    };
    write_json_atomic(&managed_active_path(remote_dir), &rolled_back)?;
    if start_after_rollback {
        let _ = managed_start(remote_dir, None)?;
    }
    Ok(ManagedRollbackSummary {
        release_id: previous.clone(),
        message: format!("rolled back FlowRT release to {previous}"),
    })
}

pub(crate) fn load_managed_active(remote_dir: &Path) -> Result<ManagedActiveState> {
    load_json(&managed_active_path(remote_dir), "managed active state")
}

pub(crate) fn managed_release_id(manifest: &BundleManifest, target: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(manifest.package.as_bytes());
    hasher.update([0]);
    hasher.update(manifest.profile.as_deref().unwrap_or("").as_bytes());
    hasher.update([0]);
    hasher.update(target.as_bytes());
    hasher.update([0]);
    hasher.update(manifest.platform.as_deref().unwrap_or("").as_bytes());
    hasher.update([0]);
    hasher.update(manifest.build_mode.to_string().as_bytes());
    hasher.update([0]);
    hasher.update(manifest.entry.as_bytes());
    let mut artifacts = manifest
        .artifacts
        .iter()
        .filter(|artifact| artifact.target == target)
        .map(|artifact| {
            format!(
                "{}\0{}\0{}\0{}",
                artifact.kind,
                artifact.platform.as_deref().unwrap_or(""),
                artifact.path.display(),
                artifact.sha256
            )
        })
        .collect::<Vec<_>>();
    artifacts.sort();
    for artifact in artifacts {
        hasher.update([0]);
        hasher.update(artifact.as_bytes());
    }
    let digest = hex_lower(&hasher.finalize());
    format!(
        "{}-{}",
        sanitize_package_name(&manifest.package).replace('_', "-"),
        &digest[..16]
    )
}

pub(crate) fn managed_release_path(remote_dir: &Path, release_id: &str) -> PathBuf {
    remote_dir.join("releases").join(release_id)
}

fn managed_active_path(remote_dir: &Path) -> PathBuf {
    remote_dir.join("state").join("active.json")
}

fn managed_run_path(remote_dir: &Path) -> PathBuf {
    remote_dir.join("state").join("run.json")
}

fn load_managed_run(remote_dir: &Path) -> Result<ManagedRunState> {
    load_json(&managed_run_path(remote_dir), "managed run state")
}

fn load_managed_release_metadata(release_path: &Path) -> Result<ManagedReleaseMetadata> {
    load_json(
        &release_path.join(MANAGED_RELEASE_METADATA),
        "managed release metadata",
    )
}

fn managed_installed_releases(remote_dir: &Path) -> Result<Vec<String>> {
    let releases_dir = remote_dir.join("releases");
    if !releases_dir.exists() {
        return Ok(Vec::new());
    }
    let mut releases = Vec::new();
    for entry in fs::read_dir(&releases_dir)
        .with_context(|| format!("failed to read `{}`", releases_dir.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to read `{}` entry", releases_dir.display()))?;
        if entry.file_type()?.is_dir()
            && entry.path().join(MANAGED_RELEASE_METADATA).is_file()
            && let Some(name) = entry.file_name().to_str()
        {
            releases.push(name.to_string());
        }
    }
    releases.sort();
    Ok(releases)
}

fn write_managed_release_metadata(
    release_path: &Path,
    manifest: &BundleManifest,
    target: &str,
    release_id: &str,
) -> Result<()> {
    let metadata = ManagedReleaseMetadata {
        schema_version: 1,
        release_id: release_id.to_string(),
        package: manifest.package.clone(),
        profile: manifest.profile.clone(),
        target: target.to_string(),
        platform: manifest.platform.clone(),
        build_mode: manifest.build_mode,
        entry: manifest.entry.clone(),
        installed_unix_ms: current_unix_ms(),
    };
    write_json_atomic(&release_path.join(MANAGED_RELEASE_METADATA), &metadata)
}

fn write_json_atomic<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let json = serde_json::to_string_pretty(value)?;
    fs::write(&tmp, format!("{json}\n"))
        .with_context(|| format!("failed to write `{}`", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("failed to publish `{}`", path.display()))?;
    Ok(())
}

fn remote_managed_command<'a, I>(host: &str, args: I, dry_run: bool) -> Result<String>
where
    I: IntoIterator<Item = &'a str>,
{
    validate_deploy_host(host)?;
    let args = args.into_iter().collect::<Vec<_>>();
    if let Some(remote_dir_index) = args.iter().position(|arg| *arg == "--remote-dir")
        && let Some(remote_dir) = args.get(remote_dir_index + 1)
    {
        validate_deploy_remote_dir(remote_dir)?;
    }
    if dry_run {
        let mut rendered = vec![
            "ssh".to_string(),
            "--".to_string(),
            host.to_string(),
            "flowrt".to_string(),
        ];
        rendered.extend(args.iter().map(|arg| (*arg).to_string()));
        return Ok(rendered.join(" "));
    }
    let output = ProcessCommand::new("ssh")
        .arg("--")
        .arg(host)
        .arg("flowrt")
        .args(&args)
        .output()
        .with_context(|| format!("failed to spawn ssh for host `{host}`"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        if detail.is_empty() {
            anyhow::bail!(
                "remote managed command failed with status {}",
                output.status
            );
        }
        anyhow::bail!("remote managed command failed: {detail}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn status_format_name(format: crate::StatusFormat) -> &'static str {
    match format {
        crate::StatusFormat::Text => "text",
        crate::StatusFormat::Json => "json",
    }
}

fn load_json<T>(path: &Path, context: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read {context} `{}`", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {context} `{}`", path.display()))
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    let mut status = 0;
    let wait = unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) };
    if wait == pid as libc::pid_t {
        return false;
    }
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
fn terminate_process(pid: u32, signal: libc::c_int) {
    if pid <= i32::MAX as u32 {
        let pgid = -(pid as i32);
        if unsafe { libc::kill(pgid, signal) == 0 } {
            return;
        }
    }
    let _ = unsafe { libc::kill(pid as libc::pid_t, signal) };
}

#[cfg(not(unix))]
fn terminate_process(_pid: u32, _signal: i32) {}
