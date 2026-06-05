use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::path::{Component, Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use flowrt_codegen::{ArtifactBundle, emit_artifacts};
use flowrt_ir::{
    ContractIr, LanguageKind, hash_source, normalize_document, project_contract_to_profile,
};
use flowrt_validate::validate_contract;
use object::{Object, ObjectSection};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const SELF_DESCRIPTION_SECTION: &str = ".flowrt.selfdesc";

#[derive(Debug, Parser)]
#[command(name = "flowrt")]
#[command(version)]
#[command(about = "FlowRT 数据流契约工具链")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 解析、归一化并校验一个 RSDL 文件。
    Check {
        /// .rsdl 文件路径。
        rsdl: PathBuf,
    },

    /// 准备 FlowRT 管理的应用产物。
    Prepare {
        /// .rsdl 文件路径。
        rsdl: PathBuf,

        /// FlowRT 管理产物输出目录。
        #[arg(long, default_value = "flowrt")]
        out_dir: PathBuf,

        /// 选择用于生成产物的 profile 名称。
        #[arg(long)]
        profile: Option<String>,
    },

    /// 准备并构建 FlowRT 管理的应用产物。
    Build {
        /// .rsdl 文件路径。
        rsdl: PathBuf,

        /// FlowRT 管理产物输出目录。
        #[arg(long, default_value = "flowrt")]
        out_dir: PathBuf,

        /// 同时构建 `flowrt launch` 需要的 generated supervisor。
        #[arg(long)]
        launcher: bool,

        /// 选择用于生成产物的 profile 名称。
        #[arg(long)]
        profile: Option<String>,
    },

    /// 准备并运行 FlowRT 管理的应用 crate。
    Run {
        /// .rsdl 文件路径。
        rsdl: PathBuf,

        /// FlowRT 管理产物输出目录。
        #[arg(long, default_value = "flowrt")]
        out_dir: PathBuf,

        /// 只运行生成应用中的一个 RSDL process group。
        #[arg(long)]
        process: Option<String>,

        /// 显式限制生成应用最多运行多少个 tick；省略表示无限运行。
        #[arg(long, value_parser = parse_positive_usize)]
        run_ticks: Option<usize>,

        /// 选择用于生成和运行的 profile 名称。
        #[arg(long)]
        profile: Option<String>,
    },

    /// 准备、构建并运行生成的 process supervisor。
    Launch {
        /// .rsdl 文件路径。
        rsdl: PathBuf,

        /// FlowRT 管理产物输出目录。
        #[arg(long, default_value = "flowrt")]
        out_dir: PathBuf,

        /// 显式限制生成应用最多运行多少个 tick；省略表示无限运行。
        #[arg(long, value_parser = parse_positive_usize)]
        run_ticks: Option<usize>,

        /// 选择用于生成和启动的 profile 名称。
        #[arg(long)]
        profile: Option<String>,
    },

    /// 查看已落盘的 Contract IR JSON 文档摘要。
    Inspect {
        /// contract.ir.json 路径。
        ir: PathBuf,
    },

    /// 从 FlowRT 应用二进制或 selfdesc.json 输出静态拓扑。
    List {
        /// FlowRT 管理应用二进制，或 flowrt/selfdesc/selfdesc.json。
        image: PathBuf,
    },

    /// 从 FlowRT 应用二进制或 selfdesc.json 输出实例列表。
    Nodes {
        /// FlowRT 管理应用二进制，或 flowrt/selfdesc/selfdesc.json。
        image: PathBuf,
    },

    /// 读取 live runtime 中一个 channel 的 latest 快照。
    Echo {
        /// channel 名称；旧式兼容用法中这是 FlowRT 应用二进制或 selfdesc.json。
        target: String,

        /// 旧式兼容用法：channel 名称。
        channel: Option<String>,

        /// 显式提供 FlowRT 应用二进制或 selfdesc.json；省略时从 live runtime 请求 self-description。
        #[arg(long)]
        image: Option<PathBuf>,

        /// 显式指定 runtime introspection socket；省略时按 selfdesc hash 自动匹配。
        #[arg(long)]
        socket: Option<PathBuf>,

        /// 持续轮询该 channel；按 Ctrl-C 结束。
        #[arg(long)]
        follow: bool,

        /// `--follow` 模式下的轮询间隔，单位毫秒。
        #[arg(long, default_value_t = 250, value_parser = clap::value_parser!(u64).range(1..))]
        interval_ms: u64,
    },

    /// 查询或提交 live runtime 参数。
    Params {
        #[command(subcommand)]
        command: ParamsCommand,
    },

    /// 扫描当前用户 runtime socket 并输出 live status。
    Status,

    /// 统计 live channel 发布频率。
    Hz {
        /// 可选 channel 名称；省略时输出所有 channel。
        channel: Option<String>,

        /// 显式指定 runtime introspection socket；省略时扫描当前用户全部 runtime socket。
        #[arg(long)]
        socket: Option<PathBuf>,

        /// 采样窗口，单位毫秒。
        #[arg(long, default_value_t = 1000, value_parser = clap::value_parser!(u64).range(1..))]
        window_ms: u64,
    },
}

#[derive(Debug, Subcommand)]
enum ParamsCommand {
    /// 列出 live runtime 参数。
    List {
        /// FlowRT 管理应用二进制，或 flowrt/selfdesc/selfdesc.json。
        image: PathBuf,

        /// 显式指定 runtime introspection socket；省略时按 selfdesc hash 自动匹配。
        #[arg(long)]
        socket: Option<PathBuf>,
    },

    /// 读取单个 live runtime 参数。
    Get {
        /// FlowRT 管理应用二进制，或 flowrt/selfdesc/selfdesc.json。
        image: PathBuf,

        /// 参数名，格式为 `<instance>.<param>`。
        name: String,

        /// 显式指定 runtime introspection socket；省略时按 selfdesc hash 自动匹配。
        #[arg(long)]
        socket: Option<PathBuf>,
    },

    /// 设置单个 live runtime 参数 pending 值。
    Set {
        /// FlowRT 管理应用二进制，或 flowrt/selfdesc/selfdesc.json。
        image: PathBuf,

        /// 参数名，格式为 `<instance>.<param>`。
        name: String,

        /// JSON 参数值，例如 `2.5`、`true` 或 `"safe"`。
        value: String,

        /// 显式指定 runtime introspection socket；省略时按 selfdesc hash 自动匹配。
        #[arg(long)]
        socket: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Check { rsdl } => {
            let contract = load_contract_from_rsdl(&rsdl)?;
            println!("OK {}", summary(&contract));
        }
        Command::Prepare {
            rsdl,
            out_dir,
            profile,
        } => {
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let _lock = WorkspaceLock::acquire(&out_dir)?;
            let prepared = prepare_workspace(&rsdl, &out_dir, profile.as_deref())?;
            println!(
                "prepared {} and {} artifact(s)",
                prepared.contract_path.display(),
                prepared.artifact_count
            );
        }
        Command::Build {
            rsdl,
            out_dir,
            launcher,
            profile,
        } => {
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let _lock = WorkspaceLock::acquire(&out_dir)?;
            let prepared = prepare_workspace(&rsdl, &out_dir, profile.as_deref())?;
            build_workspace(&prepared.selected_contract, &out_dir, launcher)?;
            println!(
                "built {} and {} artifact(s)",
                prepared.contract_path.display(),
                prepared.artifact_count
            );
        }
        Command::Run {
            rsdl,
            out_dir,
            process,
            run_ticks,
            profile,
        } => {
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let build_hint = build_command_hint(&rsdl, profile.as_deref(), false);
            let contract = load_prepared_contract(&out_dir, &build_hint)?;
            ensure_prepared_profile_matches(&contract, profile.as_deref(), &build_hint)?;
            run_workspace(&contract, &out_dir, process.as_deref(), run_ticks)?;
        }
        Command::Launch {
            rsdl,
            out_dir,
            run_ticks,
            profile,
        } => {
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let build_hint = build_command_hint(&rsdl, profile.as_deref(), true);
            let contract = load_prepared_contract(&out_dir, &build_hint)?;
            ensure_prepared_profile_matches(&contract, profile.as_deref(), &build_hint)?;
            launch_workspace(&contract, &out_dir, run_ticks)?;
        }
        Command::Inspect { ir } => {
            let contract = load_contract_from_json(&ir)?;
            println!("{}", summary(&contract));
        }
        Command::List { image } => {
            let self_description = load_self_description(&image)?;
            println!("{}", self_description_summary(&self_description));
        }
        Command::Nodes { image } => {
            let self_description = load_self_description(&image)?;
            println!("{}", self_description_nodes(&self_description));
        }
        Command::Echo {
            target,
            channel,
            image,
            socket,
            follow,
            interval_ms,
        } => {
            let echo_target = EchoTarget::from_cli(target, channel, image)?;
            if follow {
                echo_channel_follow(
                    &echo_target,
                    socket.as_deref(),
                    Duration::from_millis(interval_ms),
                    &mut io::stdout(),
                )?;
            } else {
                println!("{}", echo_channel(&echo_target, socket.as_deref())?);
            }
        }
        Command::Params { command } => match command {
            ParamsCommand::List { image, socket } => {
                println!("{}", params_list(&image, socket.as_deref())?);
            }
            ParamsCommand::Get {
                image,
                name,
                socket,
            } => {
                println!("{}", params_get(&image, &name, socket.as_deref())?);
            }
            ParamsCommand::Set {
                image,
                name,
                value,
                socket,
            } => {
                println!("{}", params_set(&image, &name, &value, socket.as_deref())?);
            }
        },
        Command::Status => {
            println!("{}", live_status_summary()?);
        }
        Command::Hz {
            channel,
            socket,
            window_ms,
        } => {
            println!(
                "{}",
                live_hz_summary(channel.as_deref(), socket.as_deref(), window_ms)?
            );
        }
    }
    Ok(())
}

fn parse_positive_usize(raw: &str) -> std::result::Result<usize, String> {
    match raw.parse::<usize>() {
        Ok(value) if value > 0 => Ok(value),
        _ => Err("must be a positive integer".to_string()),
    }
}

#[derive(Debug)]
struct WorkspaceLock {
    path: PathBuf,
    file: File,
}

impl WorkspaceLock {
    fn acquire(out_dir: &Path) -> Result<Self> {
        fs::create_dir_all(out_dir)
            .with_context(|| format!("failed to create `{}`", out_dir.display()))?;
        let path = out_dir.join(".flowrt.lock");
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("failed to open lock `{}`", path.display()))?;
        if !try_lock_file(&file)? {
            anyhow::bail!(
                "FlowRT output directory `{}` is already in use by another flowrt command; retry after it finishes, or remove `{}` if no FlowRT command is running",
                out_dir.display(),
                path.display()
            )
        }
        file.set_len(0)
            .with_context(|| format!("failed to truncate lock `{}`", path.display()))?;
        writeln!(file, "pid={}", std::process::id())
            .with_context(|| format!("failed to write `{}`", path.display()))?;
        Ok(Self { path, file })
    }
}

impl Drop for WorkspaceLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = unlock_file(&self.file);
    }
}

fn try_lock_file(file: &File) -> Result<bool> {
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN => Ok(false),
        _ => Err(error).context("failed to lock FlowRT output directory"),
    }
}

fn unlock_file(file: &File) -> Result<()> {
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error()).context("failed to unlock FlowRT output directory")
    }
}

fn normalize_contract_from_rsdl(path: &Path) -> Result<ContractIr> {
    let loaded = flowrt_rsdl::load_file(path)
        .with_context(|| format!("failed to load RSDL source `{}`", path.display()))?;
    let source_bundle = loaded.source_bundle_text();
    normalize_document(&loaded.document, hash_source(&source_bundle))
        .with_context(|| format!("failed to normalize `{}`", path.display()))
}

fn load_contract_from_rsdl(path: &Path) -> Result<ContractIr> {
    let contract = normalize_contract_from_rsdl(path)?;
    validate_contract(&contract).context("contract validation failed")?;
    Ok(contract)
}

fn load_contract_from_json(path: &Path) -> Result<ContractIr> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read Contract IR `{}`", path.display()))?;
    let contract = ContractIr::from_json_str(&source)
        .with_context(|| format!("failed to parse Contract IR `{}`", path.display()))?;
    validate_contract(&contract).context("contract validation failed")?;
    Ok(contract)
}

fn prepared_contract_path(out_dir: &Path) -> PathBuf {
    out_dir.join("contract").join("contract.ir.json")
}

fn load_prepared_contract(out_dir: &Path, build_hint: &str) -> Result<ContractIr> {
    let path = prepared_contract_path(out_dir);
    if !path.exists() {
        anyhow::bail!(
            "FlowRT generated contract `{}` not found; run `{build_hint}` first",
            path.display(),
        );
    }
    load_contract_from_json(&path)
}

fn ensure_prepared_profile_matches(
    contract: &ContractIr,
    requested_profile: Option<&str>,
    build_hint: &str,
) -> Result<()> {
    let Some(requested_profile) = requested_profile else {
        return Ok(());
    };
    let prepared_profile = selected_prepared_profile_name(contract);
    if prepared_profile == Some(requested_profile) {
        return Ok(());
    }
    let prepared = prepared_profile.unwrap_or("<none>");
    anyhow::bail!(
        "prepared FlowRT artifacts use profile `{prepared}`, but command requested profile `{requested_profile}`; run `{build_hint}` first"
    );
}

fn selected_prepared_profile_name(contract: &ContractIr) -> Option<&str> {
    contract
        .profiles
        .first()
        .map(|profile| profile.name.as_str())
}

fn build_command_hint(rsdl: &Path, profile: Option<&str>, launcher: bool) -> String {
    let mut command = "flowrt build".to_string();
    if launcher {
        command.push_str(" --launcher");
    }
    if let Some(profile) = profile {
        command.push_str(" --profile ");
        command.push_str(profile);
    }
    command.push(' ');
    command.push_str(&rsdl.display().to_string());
    command
}

#[derive(Debug, Deserialize)]
struct SelfDescription {
    self_description_version: String,
    source_hash: String,
    package: SelfDescriptionPackage,
    graphs: Vec<SelfDescriptionGraph>,
    message_abi: Vec<SelfDescriptionMessageAbi>,
    #[serde(default)]
    message_frames: Vec<SelfDescriptionMessageFrame>,
}

#[derive(Debug, Deserialize)]
struct SelfDescriptionPackage {
    name: String,
}

#[derive(Debug, Deserialize)]
struct SelfDescriptionGraph {
    name: String,
    instances: Vec<SelfDescriptionInstance>,
    tasks: Vec<SelfDescriptionTask>,
    channels: Vec<SelfDescriptionChannel>,
}

#[derive(Debug, Deserialize)]
struct SelfDescriptionInstance {
    name: String,
    component: String,
    process: String,
    runtime: String,
    #[serde(default)]
    params: Vec<SelfDescriptionParam>,
}

#[derive(Debug, Deserialize)]
struct SelfDescriptionParam {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    update: String,
}

#[derive(Debug, Deserialize)]
struct SelfDescriptionTask {
    instance: String,
    trigger: String,
}

#[derive(Debug, Deserialize)]
struct SelfDescriptionChannel {
    from: String,
    to: String,
    message_type: String,
}

#[derive(Debug, Deserialize)]
struct SelfDescriptionMessageAbi {
    type_name: String,
    size_bytes: usize,
    #[serde(default)]
    fields: Vec<SelfDescriptionFieldAbi>,
}

#[derive(Debug, Clone, Deserialize)]
struct SelfDescriptionFieldAbi {
    name: String,
    #[serde(rename = "type", default)]
    ty: String,
    offset_bytes: usize,
    size_bytes: usize,
}

#[derive(Debug, Deserialize)]
struct SelfDescriptionMessageFrame {
    type_name: String,
    #[serde(default)]
    header_size_bytes: usize,
    max_size_bytes: usize,
    variable: bool,
    #[serde(default)]
    fields: Vec<SelfDescriptionFrameField>,
}

#[derive(Debug, Clone, Deserialize)]
struct SelfDescriptionFrameField {
    name: String,
    #[serde(rename = "type", default)]
    ty: String,
    header_offset_bytes: usize,
    header_size_bytes: usize,
    tail_max_bytes: Option<usize>,
}

fn load_self_description(path: &Path) -> Result<SelfDescription> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read FlowRT image `{}`", path.display()))?;
    let json = if path
        .file_name()
        .is_some_and(|name| name == OsStr::new("selfdesc.json"))
    {
        bytes
    } else {
        self_description_section_bytes(&bytes).with_context(|| {
            format!(
                "failed to read `{SELF_DESCRIPTION_SECTION}` section from `{}`",
                path.display()
            )
        })?
    };
    serde_json::from_slice(&json).with_context(|| {
        format!(
            "failed to parse FlowRT self-description from `{}`",
            path.display()
        )
    })
}

fn load_self_description_with_hash(path: &Path) -> Result<(SelfDescription, String)> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read FlowRT image `{}`", path.display()))?;
    let json = if path
        .file_name()
        .is_some_and(|name| name == OsStr::new("selfdesc.json"))
    {
        bytes
    } else {
        self_description_section_bytes(&bytes).with_context(|| {
            format!(
                "failed to read `{SELF_DESCRIPTION_SECTION}` section from `{}`",
                path.display()
            )
        })?
    };
    let hash = self_description_hash(&json);
    let self_description = serde_json::from_slice(&json).with_context(|| {
        format!(
            "failed to parse FlowRT self-description from `{}`",
            path.display()
        )
    })?;
    Ok((self_description, hash))
}

fn self_description_section_bytes(image: &[u8]) -> Result<Vec<u8>> {
    let object =
        object::File::parse(image).context("FlowRT image is not a supported object file")?;
    let section = object
        .section_by_name(SELF_DESCRIPTION_SECTION)
        .with_context(|| format!("FlowRT image does not contain `{SELF_DESCRIPTION_SECTION}`"))?;
    let data = section
        .data()
        .context("failed to decode FlowRT self-description section data")?;
    Ok(data.to_vec())
}

fn self_description_hash(json: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(json);
    format!("{:x}", hasher.finalize())
}

fn self_description_summary(self_description: &SelfDescription) -> String {
    let mut output = format!(
        "package={} selfdesc={} source_hash={} graphs={} instances={} tasks={} channels={} messages={}",
        self_description.package.name,
        self_description.self_description_version,
        self_description.source_hash,
        self_description.graphs.len(),
        self_description
            .graphs
            .iter()
            .map(|graph| graph.instances.len())
            .sum::<usize>(),
        self_description
            .graphs
            .iter()
            .map(|graph| graph.tasks.len())
            .sum::<usize>(),
        self_description
            .graphs
            .iter()
            .map(|graph| graph.channels.len())
            .sum::<usize>(),
        self_description.message_abi.len()
    );
    for graph in &self_description.graphs {
        output.push_str(&format!("\ngraph {}", graph.name));
        for task in &graph.tasks {
            output.push_str(&format!(
                "\ntask {} trigger={}",
                task.instance, task.trigger
            ));
        }
        for channel in &graph.channels {
            output.push_str(&format!(
                "\nchannel {} -> {} type={}",
                channel.from, channel.to, channel.message_type
            ));
        }
    }
    for message in &self_description.message_abi {
        output.push_str(&format!(
            "\nmessage {} size={}",
            message.type_name, message.size_bytes
        ));
    }
    output
}

fn self_description_nodes(self_description: &SelfDescription) -> String {
    let mut lines = Vec::new();
    for graph in &self_description.graphs {
        lines.push(format!("graph {}", graph.name));
        for instance in &graph.instances {
            lines.push(format!(
                "{} process={} runtime={} component={}",
                instance.name, instance.process, instance.runtime, instance.component
            ));
        }
    }
    if lines.is_empty() {
        "no graphs".to_string()
    } else {
        lines.join("\n")
    }
}

#[derive(Debug, Clone)]
struct EchoChannelSpec {
    name: String,
    message_type: String,
    payload_shape: EchoPayloadShape,
}

#[derive(Debug, Clone)]
enum EchoPayloadShape {
    FixedAbi {
        size_bytes: usize,
        fields: Vec<SelfDescriptionFieldAbi>,
    },
    CanonicalFrame {
        header_size_bytes: usize,
        max_size_bytes: usize,
        variable: bool,
        fields: Vec<SelfDescriptionFrameField>,
    },
}

#[derive(Debug, Clone)]
struct EchoTarget {
    image: Option<PathBuf>,
    channel: String,
}

impl EchoTarget {
    fn from_cli(target: String, channel: Option<String>, image: Option<PathBuf>) -> Result<Self> {
        match (channel, image) {
            (Some(channel), None) => Ok(Self {
                image: Some(PathBuf::from(target)),
                channel,
            }),
            (Some(_), Some(_)) => anyhow::bail!(
                "flowrt echo accepts either `<image> <channel>` or `--image <path> <channel>`, not both"
            ),
            (None, image) => Ok(Self {
                image,
                channel: target,
            }),
        }
    }
}

fn echo_channel(target: &EchoTarget, socket: Option<&Path>) -> Result<String> {
    let (self_description, self_description_hash, socket) = load_echo_context(target, socket)?;
    let channel_spec = find_echo_channel(&self_description, &target.channel)?;
    let _observe = open_echo_observer(&socket, &channel_spec, &self_description_hash)?;
    let snapshot = wait_for_echo_snapshot(
        &socket,
        &channel_spec,
        &self_description_hash,
        Duration::from_millis(1000),
        Duration::from_millis(50),
    )?;

    format_echo_snapshot(&channel_spec, &snapshot)
}

#[cfg(test)]
fn echo_channel_from_image(image: &Path, channel: &str, socket: Option<&Path>) -> Result<String> {
    echo_channel(
        &EchoTarget {
            image: Some(image.to_path_buf()),
            channel: channel.to_string(),
        },
        socket,
    )
}

#[cfg(test)]
fn echo_channel_snapshot_from_image(
    image: &Path,
    channel: &str,
    socket: Option<&Path>,
) -> Result<String> {
    let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
    let channel_spec = find_echo_channel(&self_description, channel)?;
    let socket = select_echo_socket(socket, &self_description_hash)?;
    let snapshot = request_echo_snapshot(&socket, &channel_spec, &self_description_hash)?;
    format_echo_snapshot(&channel_spec, &snapshot)
}

fn load_echo_context(
    target: &EchoTarget,
    socket: Option<&Path>,
) -> Result<(SelfDescription, String, PathBuf)> {
    match &target.image {
        Some(image) => {
            let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
            let socket = select_echo_socket(socket, &self_description_hash)?;
            Ok((self_description, self_description_hash, socket))
        }
        None => load_echo_context_from_live_socket(socket),
    }
}

fn load_echo_context_from_live_socket(
    socket: Option<&Path>,
) -> Result<(SelfDescription, String, PathBuf)> {
    let socket = select_live_self_description_socket(socket)?;
    let response = flowrt::request_self_description(&socket).with_context(|| {
        format!(
            "failed to request FlowRT self-description from `{}`",
            socket.display()
        )
    })?;
    let (handshake, json) = match response {
        flowrt::IntrospectionResponse::SelfDescription { handshake, json } => (handshake, json),
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to read FlowRT self-description from `{}`: {message}",
                socket.display()
            );
        }
        _ => anyhow::bail!(
            "runtime socket `{}` returned an unexpected introspection response",
            socket.display()
        ),
    };
    let hash = self_description_hash(json.as_bytes());
    if handshake.self_description_hash != hash {
        anyhow::bail!(
            "runtime socket `{}` self-description hash `{}` does not match served self-description `{}`",
            socket.display(),
            handshake.self_description_hash,
            hash
        );
    }
    let self_description = serde_json::from_str(&json).with_context(|| {
        format!(
            "failed to parse FlowRT self-description served by `{}`",
            socket.display()
        )
    })?;
    Ok((self_description, hash, socket))
}

fn select_live_self_description_socket(socket: Option<&Path>) -> Result<PathBuf> {
    if let Some(socket) = socket {
        return Ok(socket.to_path_buf());
    }
    let mut matches = Vec::new();
    for socket in
        flowrt::discover_runtime_sockets().context("failed to scan FlowRT runtime sockets")?
    {
        let Ok(flowrt::IntrospectionResponse::SelfDescription { .. }) =
            flowrt::request_self_description(&socket)
        else {
            continue;
        };
        matches.push(socket);
    }
    match matches.len() {
        0 => anyhow::bail!(
            "no live FlowRT process exposes self-description; pass `--socket <path>` or `--image <path>`"
        ),
        1 => Ok(matches.remove(0)),
        _ => {
            let sockets = matches
                .iter()
                .map(|socket| socket.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!(
                "multiple live FlowRT processes expose self-description: {sockets}; pass `--socket <path>` to choose one"
            )
        }
    }
}

fn open_echo_observer(
    socket: &Path,
    channel_spec: &EchoChannelSpec,
    self_description_hash: &str,
) -> Result<Option<std::os::unix::net::UnixStream>> {
    let (stream, response) = flowrt::observe_channel_stream(socket, &channel_spec.name)
        .with_context(|| {
            format!(
                "failed to observe channel `{}` via `{}`",
                channel_spec.name,
                socket.display()
            )
        })?;
    match response {
        flowrt::IntrospectionResponse::ObserveReady { handshake, .. } => {
            ensure_handshake_hash(&handshake, self_description_hash, socket)?;
            Ok(Some(stream))
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to observe FlowRT channel `{}` via `{}`: {message}",
                channel_spec.name,
                socket.display()
            );
        }
        _ => anyhow::bail!(
            "runtime socket `{}` returned an unexpected introspection response",
            socket.display()
        ),
    }
}

fn echo_channel_follow(
    target: &EchoTarget,
    socket: Option<&Path>,
    interval: Duration,
    output: &mut dyn Write,
) -> Result<()> {
    echo_channel_follow_for_polls(target, socket, interval, usize::MAX, output)
}

fn echo_channel_follow_for_polls(
    target: &EchoTarget,
    socket: Option<&Path>,
    interval: Duration,
    max_polls: usize,
    output: &mut dyn Write,
) -> Result<()> {
    let (self_description, self_description_hash, socket) = load_echo_context(target, socket)?;
    let channel_spec = find_echo_channel(&self_description, &target.channel)?;
    let _observe = open_echo_observer(&socket, &channel_spec, &self_description_hash)?;
    let mut last_snapshot_key = None;

    for index in 0..max_polls {
        let snapshot = request_echo_snapshot(&socket, &channel_spec, &self_description_hash)?;
        let snapshot_key = EchoSnapshotKey::from(&snapshot);
        if last_snapshot_key.as_ref() != Some(&snapshot_key) {
            writeln!(
                output,
                "{}",
                format_echo_snapshot(&channel_spec, &snapshot)?
            )
            .context("failed to write echo output")?;
            output.flush().context("failed to flush echo output")?;
            last_snapshot_key = Some(snapshot_key);
        }
        if index + 1 < max_polls {
            std::thread::sleep(interval);
        }
    }

    Ok(())
}

fn select_echo_socket(socket: Option<&Path>, self_description_hash: &str) -> Result<PathBuf> {
    let socket = match socket {
        Some(socket) => {
            ensure_socket_matches_self_description_hash(socket, self_description_hash)?;
            socket.to_path_buf()
        }
        None => {
            let sockets = flowrt::discover_runtime_sockets()
                .context("failed to scan FlowRT runtime sockets")?;
            select_matching_runtime_socket(self_description_hash, sockets)?
        }
    };
    Ok(socket)
}

fn request_echo_snapshot(
    socket: &Path,
    channel_spec: &EchoChannelSpec,
    self_description_hash: &str,
) -> Result<flowrt::introspection::IntrospectionChannelSnapshot> {
    let response =
        flowrt::request_channel_snapshot(socket, &channel_spec.name).with_context(|| {
            format!(
                "failed to request channel snapshot from `{}`",
                socket.display()
            )
        })?;

    let (handshake, snapshot) = match response {
        flowrt::IntrospectionResponse::ChannelSnapshot { handshake, channel } => {
            (handshake, channel)
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to read channel snapshot `{}` from `{}`: {message}",
                channel_spec.name,
                socket.display()
            );
        }
        flowrt::IntrospectionResponse::Status { .. } => {
            anyhow::bail!(
                "runtime socket `{}` returned an unexpected status response",
                socket.display()
            );
        }
        flowrt::IntrospectionResponse::SelfDescription { .. }
        | flowrt::IntrospectionResponse::ObserveReady { .. } => {
            anyhow::bail!(
                "runtime socket `{}` returned an unexpected introspection response",
                socket.display()
            );
        }
        flowrt::IntrospectionResponse::ParamList { .. }
        | flowrt::IntrospectionResponse::ParamValue { .. } => {
            anyhow::bail!(
                "runtime socket `{}` returned an unexpected parameter response",
                socket.display()
            );
        }
    };

    if handshake.self_description_hash != self_description_hash {
        anyhow::bail!(
            "runtime socket `{}` self-description hash `{}` does not match static self-description `{}`",
            socket.display(),
            handshake.self_description_hash,
            self_description_hash
        );
    }

    Ok(snapshot)
}

fn wait_for_echo_snapshot(
    socket: &Path,
    channel_spec: &EchoChannelSpec,
    self_description_hash: &str,
    timeout: Duration,
    interval: Duration,
) -> Result<flowrt::introspection::IntrospectionChannelSnapshot> {
    let started = std::time::Instant::now();
    loop {
        let snapshot = request_echo_snapshot(socket, channel_spec, self_description_hash)?;
        if snapshot
            .payload
            .as_ref()
            .is_some_and(|payload| !payload.is_empty())
        {
            return Ok(snapshot);
        }
        if started.elapsed() >= timeout {
            return Ok(snapshot);
        }
        std::thread::sleep(interval);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EchoSnapshotKey {
    published_count: u64,
    published_at_ms: Option<u64>,
    payload: Option<Vec<u8>>,
}

impl From<&flowrt::introspection::IntrospectionChannelSnapshot> for EchoSnapshotKey {
    fn from(snapshot: &flowrt::introspection::IntrospectionChannelSnapshot) -> Self {
        Self {
            published_count: snapshot.published_count,
            published_at_ms: snapshot.published_at_ms,
            payload: snapshot.payload.clone(),
        }
    }
}

fn ensure_socket_matches_self_description_hash(
    socket: &Path,
    self_description_hash: &str,
) -> Result<()> {
    let response = flowrt::request_status(socket)
        .with_context(|| format!("failed to request status from `{}`", socket.display()))?;
    let flowrt::IntrospectionResponse::Status { handshake, .. } = response else {
        anyhow::bail!(
            "runtime socket `{}` returned an unexpected introspection response",
            socket.display()
        );
    };
    if handshake.self_description_hash != self_description_hash {
        anyhow::bail!(
            "runtime socket `{}` self-description hash `{}` does not match static self-description `{}`",
            socket.display(),
            handshake.self_description_hash,
            self_description_hash
        );
    }
    Ok(())
}

fn find_echo_channel(
    self_description: &SelfDescription,
    channel_name: &str,
) -> Result<EchoChannelSpec> {
    let mut matches = Vec::new();
    for graph in &self_description.graphs {
        for channel in &graph.channels {
            let name = echo_channel_name(channel);
            if name == channel_name || channel.from == channel_name || channel.to == channel_name {
                matches.push(EchoChannelSpec {
                    name,
                    message_type: channel.message_type.clone(),
                    payload_shape: echo_payload_shape(
                        &self_description.message_abi,
                        &self_description.message_frames,
                        &channel.message_type,
                    )?,
                });
            }
        }
    }

    match matches.len() {
        0 => anyhow::bail!("FlowRT self-description does not contain channel `{channel_name}`"),
        1 => Ok(matches.remove(0)),
        _ => anyhow::bail!(
            "FlowRT self-description contains multiple channels named `{channel_name}`"
        ),
    }
}

fn echo_channel_name(channel: &SelfDescriptionChannel) -> String {
    format!("{}_to_{}", channel.from, channel.to)
}

fn echo_payload_shape(
    messages: &[SelfDescriptionMessageAbi],
    frames: &[SelfDescriptionMessageFrame],
    message_type: &str,
) -> Result<EchoPayloadShape> {
    if let Some(message) = message_abi_layout(messages, message_type)? {
        return Ok(EchoPayloadShape::FixedAbi {
            size_bytes: message.size_bytes,
            fields: message.fields.clone(),
        });
    }
    let mut frame_matches = frames
        .iter()
        .filter(|message| message.type_name == message_type)
        .collect::<Vec<_>>();
    match frame_matches.len() {
        0 => anyhow::bail!(
            "FlowRT self-description does not contain Message ABI or frame layout for `{message_type}`"
        ),
        1 => {
            let frame = frame_matches.remove(0);
            Ok(EchoPayloadShape::CanonicalFrame {
                header_size_bytes: frame.header_size_bytes,
                max_size_bytes: frame.max_size_bytes,
                variable: frame.variable,
                fields: frame.fields.clone(),
            })
        }
        _ => anyhow::bail!(
            "FlowRT self-description contains multiple Message frame layouts for `{message_type}`"
        ),
    }
}

fn message_abi_layout<'a>(
    messages: &'a [SelfDescriptionMessageAbi],
    message_type: &str,
) -> Result<Option<&'a SelfDescriptionMessageAbi>> {
    let mut layouts = messages
        .iter()
        .filter(|message| message.type_name == message_type)
        .collect::<Vec<_>>();
    match layouts.len() {
        0 => Ok(None),
        1 => Ok(Some(layouts.remove(0))),
        _ => anyhow::bail!(
            "FlowRT self-description contains multiple Message ABI layouts for `{message_type}`"
        ),
    }
}

fn select_matching_runtime_socket(
    self_description_hash: &str,
    sockets: Vec<PathBuf>,
) -> Result<PathBuf> {
    let mut matches = Vec::new();
    for socket in sockets {
        let Ok(flowrt::IntrospectionResponse::Status { handshake, .. }) =
            flowrt::request_status(&socket)
        else {
            continue;
        };
        if handshake.self_description_hash == self_description_hash {
            matches.push(socket);
        }
    }

    match matches.len() {
        0 => anyhow::bail!(
            "no live FlowRT process matches self-description hash `{self_description_hash}`; pass `--socket <path>` if the process uses a non-discoverable socket"
        ),
        1 => Ok(matches.remove(0)),
        _ => {
            let sockets = matches
                .iter()
                .map(|socket| socket.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!(
                "multiple live FlowRT processes match self-description hash `{self_description_hash}`: {sockets}; pass `--socket <path>` to choose one"
            )
        }
    }
}

fn format_echo_snapshot(
    channel: &EchoChannelSpec,
    snapshot: &flowrt::introspection::IntrospectionChannelSnapshot,
) -> Result<String> {
    let published_at_ms = snapshot
        .published_at_ms
        .map_or_else(|| "none".to_string(), |value| value.to_string());
    let Some(payload) = &snapshot.payload else {
        return Ok(format!(
            "channel={} type={} {} published_count={} published_at_ms={} payload_len=0 no payload",
            channel.name,
            channel.message_type,
            echo_payload_shape_label(&channel.payload_shape),
            snapshot.published_count,
            published_at_ms
        ));
    };
    if payload.is_empty() {
        return Ok(format!(
            "channel={} type={} {} published_count={} published_at_ms={} payload_len=0 no payload",
            channel.name,
            channel.message_type,
            echo_payload_shape_label(&channel.payload_shape),
            snapshot.published_count,
            published_at_ms
        ));
    }
    match &channel.payload_shape {
        EchoPayloadShape::FixedAbi { size_bytes, fields } => {
            if payload.len() != *size_bytes {
                anyhow::bail!(
                    "channel `{}` payload length {} does not match Message ABI size {} for `{}`",
                    channel.name,
                    payload.len(),
                    size_bytes,
                    channel.message_type
                );
            }
            let fields = format_fixed_abi_fields(fields, payload)?;
            if !fields.is_empty() {
                return Ok(format!(
                    "channel={} type={} {} published_count={} published_at_ms={} payload_len={} fields={{{}}} raw={}",
                    channel.name,
                    channel.message_type,
                    echo_payload_shape_label(&channel.payload_shape),
                    snapshot.published_count,
                    published_at_ms,
                    payload.len(),
                    fields,
                    hex_bytes(payload)
                ));
            }
        }
        EchoPayloadShape::CanonicalFrame {
            header_size_bytes,
            max_size_bytes,
            fields,
            ..
        } => {
            if payload.len() > *max_size_bytes {
                anyhow::bail!(
                    "channel `{}` payload length {} exceeds canonical frame max size {} for `{}`",
                    channel.name,
                    payload.len(),
                    max_size_bytes,
                    channel.message_type
                );
            }
            let fields = format_frame_fields(fields, *header_size_bytes, payload)?;
            if !fields.is_empty() {
                return Ok(format!(
                    "channel={} type={} {} published_count={} published_at_ms={} payload_len={} fields={{{}}} raw={}",
                    channel.name,
                    channel.message_type,
                    echo_payload_shape_label(&channel.payload_shape),
                    snapshot.published_count,
                    published_at_ms,
                    payload.len(),
                    fields,
                    hex_bytes(payload)
                ));
            }
        }
    }
    Ok(format!(
        "channel={} type={} {} published_count={} published_at_ms={} payload_len={} raw={}",
        channel.name,
        channel.message_type,
        echo_payload_shape_label(&channel.payload_shape),
        snapshot.published_count,
        published_at_ms,
        payload.len(),
        hex_bytes(payload)
    ))
}

fn echo_payload_shape_label(shape: &EchoPayloadShape) -> String {
    match shape {
        EchoPayloadShape::FixedAbi { size_bytes, .. } => format!("abi_size={size_bytes}"),
        EchoPayloadShape::CanonicalFrame {
            max_size_bytes,
            variable,
            ..
        } => {
            format!("frame_max_size={max_size_bytes} variable={variable}")
        }
    }
}

fn format_fixed_abi_fields(fields: &[SelfDescriptionFieldAbi], payload: &[u8]) -> Result<String> {
    let mut formatted = Vec::new();
    for field in fields {
        if field.offset_bytes > payload.len()
            || field.size_bytes > payload.len().saturating_sub(field.offset_bytes)
        {
            anyhow::bail!(
                "field `{}` range {}..{} exceeds payload length {}",
                field.name,
                field.offset_bytes,
                field.offset_bytes.saturating_add(field.size_bytes),
                payload.len()
            );
        }
        let bytes = &payload[field.offset_bytes..field.offset_bytes + field.size_bytes];
        formatted.push(format!(
            "{}={}",
            field.name,
            format_fixed_abi_value(&field.ty, bytes)?
        ));
    }
    Ok(formatted.join(","))
}

fn format_frame_fields(
    fields: &[SelfDescriptionFrameField],
    header_size_bytes: usize,
    payload: &[u8],
) -> Result<String> {
    if payload.len() < header_size_bytes {
        anyhow::bail!(
            "canonical frame header size {} exceeds payload length {}",
            header_size_bytes,
            payload.len()
        );
    }
    let (header, tail) = payload.split_at(header_size_bytes);
    let mut formatted = Vec::new();
    for field in fields {
        if field.header_offset_bytes > header.len()
            || field.header_size_bytes > header.len().saturating_sub(field.header_offset_bytes)
        {
            anyhow::bail!(
                "field `{}` header range {}..{} exceeds frame header length {}",
                field.name,
                field.header_offset_bytes,
                field
                    .header_offset_bytes
                    .saturating_add(field.header_size_bytes),
                header.len()
            );
        }
        let bytes =
            &header[field.header_offset_bytes..field.header_offset_bytes + field.header_size_bytes];
        let value = format_frame_field_value(field, bytes, tail)?;
        formatted.push(format!("{}={value}", field.name));
    }
    Ok(formatted.join(","))
}

fn format_frame_field_value(
    field: &SelfDescriptionFrameField,
    header_bytes: &[u8],
    tail: &[u8],
) -> Result<String> {
    let ty = field.ty.trim();
    if let Some(max_len) = parse_bounded_type_max(ty, "string")? {
        let block = frame_tail_block(field, header_bytes, tail, max_len)?;
        let text = std::str::from_utf8(block)
            .with_context(|| format!("field `{}` is not valid UTF-8", field.name))?;
        return serde_json::to_string(text)
            .with_context(|| format!("failed to format string field `{}`", field.name));
    }
    if let Some(max_len) = parse_bounded_type_max(ty, "bytes")? {
        let block = frame_tail_block(field, header_bytes, tail, max_len)?;
        return Ok(format!("0x{}", hex_bytes(block)));
    }
    if let Some((element_ty, max_len)) = parse_sequence_type(ty)? {
        let element_size = required_fixed_wire_size(element_ty)
            .with_context(|| format!("unsupported sequence element type `{element_ty}`"))?;
        let max_tail_bytes = element_size
            .checked_mul(max_len)
            .with_context(|| format!("sequence `{ty}` max length overflows"))?;
        let block = frame_tail_block(field, header_bytes, tail, max_tail_bytes)?;
        if block.len() % element_size != 0 {
            anyhow::bail!(
                "field `{}` byte length {} is not divisible by element size {}",
                field.name,
                block.len(),
                element_size
            );
        }
        let element_count = block.len() / element_size;
        if element_count > max_len {
            anyhow::bail!(
                "field `{}` contains {} sequence elements, exceeding max {}",
                field.name,
                element_count,
                max_len
            );
        }
        let mut values = Vec::with_capacity(element_count);
        for chunk in block.chunks_exact(element_size) {
            values.push(format_fixed_abi_value(element_ty, chunk)?);
        }
        return Ok(format!("[{}]", values.join(",")));
    }
    format_fixed_abi_value(ty, header_bytes)
}

fn frame_tail_block<'a>(
    field: &SelfDescriptionFrameField,
    header_bytes: &[u8],
    tail: &'a [u8],
    declared_max_len: usize,
) -> Result<&'a [u8]> {
    if header_bytes.len() != 8 {
        anyhow::bail!(
            "variable field `{}` header expects 8-byte VarSpan but has {} bytes",
            field.name,
            header_bytes.len()
        );
    }
    let offset = read_u32_le(&header_bytes[0..4])? as usize;
    let len = read_u32_le(&header_bytes[4..8])? as usize;
    if len > declared_max_len {
        anyhow::bail!(
            "field `{}` length {} exceeds declared max {}",
            field.name,
            len,
            declared_max_len
        );
    }
    if let Some(tail_max_bytes) = field.tail_max_bytes
        && len > tail_max_bytes
    {
        anyhow::bail!(
            "field `{}` length {} exceeds self-description tail max {}",
            field.name,
            len,
            tail_max_bytes
        );
    }
    if offset > tail.len() || len > tail.len().saturating_sub(offset) {
        anyhow::bail!(
            "field `{}` tail range {}..{} exceeds tail length {}",
            field.name,
            offset,
            offset.saturating_add(len),
            tail.len()
        );
    }
    Ok(&tail[offset..offset + len])
}

fn read_u32_le(bytes: &[u8]) -> Result<u32> {
    let array: [u8; 4] = bytes
        .try_into()
        .context("u32 wire value must contain exactly 4 bytes")?;
    Ok(u32::from_le_bytes(array))
}

fn parse_bounded_type_max(ty: &str, prefix: &str) -> Result<Option<usize>> {
    let Some(inner) = ty
        .strip_prefix(prefix)
        .and_then(|value| value.strip_prefix("<max="))
        .and_then(|value| value.strip_suffix('>'))
    else {
        return Ok(None);
    };
    let max = inner
        .trim()
        .parse::<usize>()
        .with_context(|| format!("invalid bounded type max in `{ty}`"))?;
    Ok(Some(max))
}

fn parse_sequence_type(ty: &str) -> Result<Option<(&str, usize)>> {
    let Some(inner) = ty
        .strip_prefix("sequence<")
        .and_then(|value| value.strip_suffix('>'))
    else {
        return Ok(None);
    };
    let Some((element, max_len)) = inner.rsplit_once(",max=") else {
        anyhow::bail!("invalid sequence type `{ty}`");
    };
    let max_len = max_len
        .trim()
        .parse::<usize>()
        .with_context(|| format!("invalid sequence max length in `{ty}`"))?;
    Ok(Some((element.trim(), max_len)))
}

fn format_fixed_abi_value(ty: &str, bytes: &[u8]) -> Result<String> {
    let ty = ty.trim();
    if let Some((element, len)) = parse_fixed_array_type(ty)? {
        let element_size = required_fixed_wire_size(element)
            .with_context(|| format!("unsupported fixed array element type `{element}`"))?;
        if bytes.len() != element_size * len {
            anyhow::bail!(
                "fixed array `{ty}` expects {} bytes but payload field has {} bytes",
                element_size * len,
                bytes.len()
            );
        }
        let mut values = Vec::with_capacity(len);
        for index in 0..len {
            let start = index * element_size;
            values.push(format_fixed_abi_value(
                element,
                &bytes[start..start + element_size],
            )?);
        }
        return Ok(format!("[{}]", values.join(",")));
    }
    format_primitive_value(ty, bytes)
}

fn required_fixed_wire_size(ty: &str) -> Result<usize> {
    fixed_wire_size(ty)?.with_context(|| format!("unsupported fixed wire type `{ty}`"))
}

fn fixed_wire_size(ty: &str) -> Result<Option<usize>> {
    let ty = ty.trim();
    if let Some(size) = primitive_size(ty) {
        return Ok(Some(size));
    }
    if let Some((element, len)) = parse_fixed_array_type(ty)? {
        let Some(element_size) = fixed_wire_size(element)? else {
            return Ok(None);
        };
        return Ok(element_size.checked_mul(len));
    }
    Ok(None)
}

fn parse_fixed_array_type(ty: &str) -> Result<Option<(&str, usize)>> {
    let Some(inner) = ty
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    else {
        return Ok(None);
    };
    let Some((element, len)) = inner.split_once(';') else {
        anyhow::bail!("invalid fixed array type `{ty}`");
    };
    let len = len
        .trim()
        .parse::<usize>()
        .with_context(|| format!("invalid fixed array length in `{ty}`"))?;
    Ok(Some((element.trim(), len)))
}

fn primitive_size(ty: &str) -> Option<usize> {
    Some(match ty {
        "bool" | "u8" | "i8" => 1,
        "u16" | "i16" => 2,
        "u32" | "i32" | "f32" => 4,
        "u64" | "i64" | "f64" => 8,
        "u128" | "i128" => 16,
        _ => return None,
    })
}

fn format_primitive_value(ty: &str, bytes: &[u8]) -> Result<String> {
    let expected = primitive_size(ty).with_context(|| format!("unsupported field type `{ty}`"))?;
    if bytes.len() != expected {
        anyhow::bail!(
            "primitive `{ty}` expects {expected} bytes but payload field has {} bytes",
            bytes.len()
        );
    }
    Ok(match ty {
        "bool" => (bytes[0] != 0).to_string(),
        "u8" => bytes[0].to_string(),
        "i8" => (bytes[0] as i8).to_string(),
        "u16" => u16::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        "i16" => i16::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        "u32" => u32::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        "i32" => i32::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        "u64" => u64::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        "i64" => i64::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        "u128" => u128::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        "i128" => i128::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        "f32" => format_float(f32::from_le_bytes(bytes.try_into().unwrap()) as f64),
        "f64" => format_float(f64::from_le_bytes(bytes.try_into().unwrap())),
        _ => unreachable!("primitive_size already accepted type"),
    })
}

fn format_float(value: f64) -> String {
    let mut formatted = value.to_string();
    if !formatted.contains('.') && !formatted.contains('e') && !formatted.contains('E') {
        formatted.push_str(".0");
    }
    formatted
}

fn params_list(image: &Path, socket: Option<&Path>) -> Result<String> {
    let (_self_description, self_description_hash) = load_self_description_with_hash(image)?;
    let socket = select_echo_socket(socket, &self_description_hash)?;
    let response = flowrt::request_param_list(&socket).with_context(|| {
        format!(
            "failed to request parameter list from `{}`",
            socket.display()
        )
    })?;
    let params = match response {
        flowrt::IntrospectionResponse::ParamList { handshake, params } => {
            ensure_handshake_hash(&handshake, &self_description_hash, &socket)?;
            params
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to list FlowRT parameters from `{}`: {message}",
                socket.display()
            );
        }
        _ => anyhow::bail!(
            "runtime socket `{}` returned an unexpected introspection response",
            socket.display()
        ),
    };
    if params.is_empty() {
        return Ok("no FlowRT parameters".to_string());
    }
    Ok(params
        .iter()
        .map(format_param_status)
        .collect::<Vec<_>>()
        .join("\n"))
}

fn params_get(image: &Path, name: &str, socket: Option<&Path>) -> Result<String> {
    let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
    ensure_param_declared(&self_description, name)?;
    let socket = select_echo_socket(socket, &self_description_hash)?;
    let response = flowrt::request_param_get(&socket, name).with_context(|| {
        format!(
            "failed to request parameter `{name}` from `{}`",
            socket.display()
        )
    })?;
    let param = match response {
        flowrt::IntrospectionResponse::ParamValue { handshake, param } => {
            ensure_handshake_hash(&handshake, &self_description_hash, &socket)?;
            param
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to get FlowRT parameter `{name}` from `{}`: {message}",
                socket.display()
            );
        }
        _ => anyhow::bail!(
            "runtime socket `{}` returned an unexpected introspection response",
            socket.display()
        ),
    };
    Ok(format_param_status(&param))
}

fn params_set(image: &Path, name: &str, raw_value: &str, socket: Option<&Path>) -> Result<String> {
    let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
    ensure_param_declared(&self_description, name)?;
    let value = serde_json::from_str::<serde_json::Value>(raw_value).with_context(|| {
        format!("FlowRT parameter values must be valid JSON; got `{raw_value}`")
    })?;
    let socket = select_echo_socket(socket, &self_description_hash)?;
    let response = flowrt::request_param_set(&socket, name, value).with_context(|| {
        format!(
            "failed to set parameter `{name}` via `{}`",
            socket.display()
        )
    })?;
    let param = match response {
        flowrt::IntrospectionResponse::ParamValue { handshake, param } => {
            ensure_handshake_hash(&handshake, &self_description_hash, &socket)?;
            param
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to set FlowRT parameter `{name}` via `{}`: {message}",
                socket.display()
            );
        }
        _ => anyhow::bail!(
            "runtime socket `{}` returned an unexpected introspection response",
            socket.display()
        ),
    };
    Ok(format_param_status(&param))
}

fn declared_param<'a>(
    self_description: &'a SelfDescription,
    name: &str,
) -> Option<(&'a SelfDescriptionInstance, &'a SelfDescriptionParam)> {
    for graph in &self_description.graphs {
        for instance in &graph.instances {
            for param in &instance.params {
                if format!("{}.{}", instance.name, param.name) == name {
                    return Some((instance, param));
                }
            }
        }
    }
    None
}

fn ensure_param_declared(self_description: &SelfDescription, name: &str) -> Result<()> {
    let Some((_instance, param)) = declared_param(self_description, name) else {
        anyhow::bail!("FlowRT self-description does not contain parameter `{name}`")
    };
    if param.ty.is_empty() || param.update.is_empty() {
        anyhow::bail!("FlowRT self-description parameter `{name}` has an incomplete schema")
    }
    Ok(())
}

fn ensure_handshake_hash(
    handshake: &flowrt::IntrospectionHandshake,
    self_description_hash: &str,
    socket: &Path,
) -> Result<()> {
    if handshake.self_description_hash == self_description_hash {
        Ok(())
    } else {
        anyhow::bail!(
            "runtime socket `{}` self-description hash `{}` does not match static self-description `{}`",
            socket.display(),
            handshake.self_description_hash,
            self_description_hash
        )
    }
}

fn format_param_status(param: &flowrt::IntrospectionParamStatus) -> String {
    let pending = param
        .pending
        .as_ref()
        .map(json_inline)
        .unwrap_or_else(|| "none".to_string());
    let min = param
        .min
        .as_ref()
        .map(json_inline)
        .unwrap_or_else(|| "none".to_string());
    let max = param
        .max
        .as_ref()
        .map(json_inline)
        .unwrap_or_else(|| "none".to_string());
    let choices = if param.choices.is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[{}]",
            param
                .choices
                .iter()
                .map(json_inline)
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    format!(
        "{} type={} update={} current={} pending={} min={} max={} choices={}",
        param.name,
        param.ty,
        param.update,
        json_inline(&param.current),
        pending,
        min,
        max,
        choices
    )
}

fn json_inline(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn live_status_summary() -> Result<String> {
    let sockets =
        flowrt::discover_runtime_sockets().context("failed to scan FlowRT runtime sockets")?;
    live_status_summary_for_sockets(sockets)
}

fn live_status_summary_for_sockets(sockets: Vec<PathBuf>) -> Result<String> {
    let mut lines = Vec::new();
    for socket in sockets {
        match flowrt::request_status(&socket) {
            Ok(flowrt::IntrospectionResponse::Status { handshake, status }) => {
                let active_observers = status
                    .channels
                    .iter()
                    .map(|channel| channel.active_observers)
                    .sum::<u64>();
                let dropped_samples = status
                    .channels
                    .iter()
                    .map(|channel| channel.dropped_samples)
                    .sum::<u64>();
                lines.push(format!(
                    "pid={} package={} process={} runtime={} selfdesc={} ticks={} channels={} observers={} dropped_samples={} socket={}",
                    handshake.pid,
                    handshake.package,
                    handshake.process,
                    handshake.runtime,
                    handshake.self_description_hash,
                    status.tick_count,
                    status.channels.len(),
                    active_observers,
                    dropped_samples,
                    socket.display()
                ));
            }
            Ok(flowrt::IntrospectionResponse::ChannelSnapshot { .. }) => {
                lines.push(format!(
                    "stale socket={} error=unexpected channel snapshot response",
                    socket.display()
                ));
            }
            Ok(flowrt::IntrospectionResponse::SelfDescription { .. })
            | Ok(flowrt::IntrospectionResponse::ObserveReady { .. }) => {
                lines.push(format!(
                    "stale socket={} error=unexpected introspection response",
                    socket.display()
                ));
            }
            Ok(flowrt::IntrospectionResponse::ParamList { .. })
            | Ok(flowrt::IntrospectionResponse::ParamValue { .. }) => {
                lines.push(format!(
                    "stale socket={} error=unexpected parameter response",
                    socket.display()
                ));
            }
            Ok(flowrt::IntrospectionResponse::Error { message, .. }) => {
                lines.push(format!("stale socket={} error={message}", socket.display()));
            }
            Err(error) => {
                lines.push(format!("stale socket={} error={error}", socket.display()));
            }
        }
    }
    if lines.is_empty() {
        Ok("no live FlowRT processes".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

fn live_hz_summary(channel: Option<&str>, socket: Option<&Path>, window_ms: u64) -> Result<String> {
    let sockets = match socket {
        Some(socket) => vec![socket.to_path_buf()],
        None => {
            flowrt::discover_runtime_sockets().context("failed to scan FlowRT runtime sockets")?
        }
    };
    live_hz_summary_for_sockets(channel, sockets, Duration::from_millis(window_ms))
}

fn live_hz_summary_for_sockets(
    channel: Option<&str>,
    sockets: Vec<PathBuf>,
    window: Duration,
) -> Result<String> {
    if sockets.is_empty() {
        return Ok("no live FlowRT processes".to_string());
    }

    let mut first = Vec::new();
    let mut lines = Vec::new();
    for socket in &sockets {
        match flowrt::request_status(socket) {
            Ok(response) => first.push((socket.clone(), response)),
            Err(error) => lines.push(format!("stale socket={} error={error}", socket.display())),
        }
    }
    if first.is_empty() {
        return Ok(lines.join("\n"));
    }
    let started = Instant::now();
    std::thread::sleep(window);
    let elapsed = started.elapsed();

    for (socket, first_status) in first {
        let second_status = match flowrt::request_status(&socket) {
            Ok(response) => response,
            Err(error) => {
                lines.push(format!("stale socket={} error={error}", socket.display()));
                continue;
            }
        };
        let summary = format_hz_summary_from_status_pair(&first_status, &second_status, elapsed)?;
        for line in summary.lines() {
            if channel.is_none_or(|selected| hz_summary_line_matches_channel(line, selected)) {
                lines.push(format!("{line} socket={}", socket.display()));
            }
        }
    }

    if lines.is_empty() {
        match channel {
            Some(channel) => Ok(format!("no live FlowRT channel matched `{channel}`")),
            None => Ok("no live FlowRT channels".to_string()),
        }
    } else {
        Ok(lines.join("\n"))
    }
}

fn hz_summary_line_matches_channel(line: &str, channel: &str) -> bool {
    line.split_ascii_whitespace()
        .any(|field| field.strip_prefix("channel=") == Some(channel))
}

fn format_hz_summary_from_status_pair(
    first: &flowrt::IntrospectionResponse,
    second: &flowrt::IntrospectionResponse,
    elapsed: Duration,
) -> Result<String> {
    let flowrt::IntrospectionResponse::Status {
        handshake,
        status: first_status,
    } = first
    else {
        anyhow::bail!("first hz sample returned non-status response");
    };
    let flowrt::IntrospectionResponse::Status {
        status: second_status,
        ..
    } = second
    else {
        anyhow::bail!("second hz sample returned non-status response");
    };
    let elapsed_secs = elapsed.as_secs_f64();
    if elapsed_secs <= 0.0 {
        anyhow::bail!("hz sample window must be positive");
    }

    let first_channels = first_status
        .channels
        .iter()
        .map(|channel| (channel.name.as_str(), channel))
        .collect::<BTreeMap<_, _>>();
    let mut lines = Vec::new();
    for second_channel in &second_status.channels {
        let Some(first_channel) = first_channels.get(second_channel.name.as_str()) else {
            continue;
        };
        let delta = second_channel
            .published_count
            .saturating_sub(first_channel.published_count);
        let hz = delta as f64 / elapsed_secs;
        lines.push(format!(
            "pid={} package={} process={} channel={} type={} delta={} hz={:.2}",
            handshake.pid,
            handshake.package,
            handshake.process,
            second_channel.name,
            second_channel.message_type,
            delta,
            hz
        ));
    }

    if lines.is_empty() {
        Ok("no live FlowRT channels".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

fn write_contract(contract: &ContractIr, out_dir: &Path) -> Result<PathBuf> {
    let contract_dir = out_dir.join("contract");
    fs::create_dir_all(&contract_dir)
        .with_context(|| format!("failed to create `{}`", contract_dir.display()))?;
    let output = contract_dir.join("contract.ir.json");
    fs::write(&output, contract.to_canonical_json()?)
        .with_context(|| format!("failed to write `{}`", output.display()))?;
    Ok(output)
}

struct PreparedWorkspace {
    contract_path: PathBuf,
    artifact_count: usize,
    selected_contract: ContractIr,
}

fn prepare_workspace(
    rsdl: &Path,
    out_dir: &Path,
    profile: Option<&str>,
) -> Result<PreparedWorkspace> {
    let contract = normalize_contract_from_rsdl(rsdl)?;
    let selected_contract = project_contract_to_profile(&contract, profile)
        .with_context(|| format!("failed to select profile for `{}`", rsdl.display()))?;
    validate_contract(&selected_contract).context("contract validation failed")?;
    let contract_path = write_contract(&selected_contract, out_dir)?;
    let artifacts = emit_artifacts(&selected_contract).context("failed to prepare artifacts")?;
    let artifact_count = write_artifacts(&artifacts, out_dir)?;
    Ok(PreparedWorkspace {
        contract_path,
        artifact_count,
        selected_contract,
    })
}

fn write_artifacts(bundle: &ArtifactBundle, out_dir: &Path) -> Result<usize> {
    for artifact in &bundle.artifacts {
        ensure_safe_relative_path(&artifact.relative_path)?;
        let output = out_dir.join(&artifact.relative_path);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create `{}`", parent.display()))?;
        }
        fs::write(&output, &artifact.content)
            .with_context(|| format!("failed to write `{}`", output.display()))?;
    }
    Ok(bundle.artifacts.len())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildStep {
    CargoApp,
    CargoSupervisor,
    CmakeApp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunMode {
    CargoApp,
    CmakeApp,
}

fn build_steps(contract: &ContractIr, include_launcher: bool) -> Vec<BuildStep> {
    let mut steps = Vec::new();
    if has_component_language(contract, LanguageKind::Rust) {
        steps.push(BuildStep::CargoApp);
    }
    if has_component_language(contract, LanguageKind::Cpp) {
        steps.push(BuildStep::CmakeApp);
    }
    if include_launcher {
        steps.push(BuildStep::CargoSupervisor);
    }
    steps
}

fn run_mode(contract: &ContractIr) -> Option<RunMode> {
    match (
        has_component_language(contract, LanguageKind::Rust),
        has_component_language(contract, LanguageKind::Cpp),
    ) {
        (true, false) => Some(RunMode::CargoApp),
        (false, true) => Some(RunMode::CmakeApp),
        _ => None,
    }
}

fn run_mode_for_process(contract: &ContractIr, process: Option<&str>) -> Result<RunMode> {
    if let Some(mode) = run_mode(contract) {
        return Ok(mode);
    }

    let Some(process) = process else {
        anyhow::bail!(
            "mixed-language `run` requires `--process <name>`; use `flowrt launch` to start every process group"
        );
    };

    let runtimes = process_runtime_flags(contract, process)
        .with_context(|| format!("unknown FlowRT process group `{process}`"))?;

    match (runtimes.rust, runtimes.cpp) {
        (true, false) => Ok(RunMode::CargoApp),
        (false, true) => Ok(RunMode::CmakeApp),
        (true, true) => anyhow::bail!(
            "mixed-language `run` cannot run process `{process}` because it contains both C++ and Rust components"
        ),
        (false, false) => {
            anyhow::bail!("FlowRT process group `{process}` has no runnable components")
        }
    }
}

fn process_runtime_flags(contract: &ContractIr, process: &str) -> Option<ProcessRuntimeFlags> {
    let component_languages = contract
        .components
        .iter()
        .map(|component| (component.name.as_str(), component.language))
        .collect::<BTreeMap<_, _>>();

    let mut runtimes = ProcessRuntimeFlags::default();
    let mut found = false;
    for graph in &contract.graphs {
        for instance in &graph.instances {
            let instance_process = instance.process.as_deref().unwrap_or("main");
            if instance_process != process {
                continue;
            }
            let Some(language) = component_languages
                .get(instance.component.name.as_str())
                .copied()
            else {
                continue;
            };
            runtimes.add(language);
            found = true;
        }
    }

    found.then_some(runtimes)
}

fn has_component_language(contract: &ContractIr, language: LanguageKind) -> bool {
    contract
        .components
        .iter()
        .any(|component| component.language == language)
}

fn is_mixed_language_contract(contract: &ContractIr) -> bool {
    has_component_language(contract, LanguageKind::Rust)
        && has_component_language(contract, LanguageKind::Cpp)
}

fn ensure_direct_runtime_supported(contract: &ContractIr, command: &str) -> Result<()> {
    if !is_mixed_language_contract(contract) {
        return Ok(());
    }

    if let Some(group) = mixed_process_group(contract) {
        anyhow::bail!(
            "mixed-language `{command}` cannot run graph `{}` process `{}` because it contains both C++ and Rust components; split them into language-specific RSDL process groups before using a cross-language backend",
            group.graph,
            group.process
        );
    }

    let backend = selected_runtime_backend_name(contract);
    if !matches!(backend, "iox2" | "zenoh") {
        anyhow::bail!(
            "mixed-language `{command}` requires backend `iox2` or `zenoh`; selected backend `{backend}` cannot carry cross-language process boundaries"
        );
    }

    Ok(())
}

#[derive(Debug, Clone, Default)]
struct ProcessRuntimeFlags {
    cpp: bool,
    rust: bool,
}

impl ProcessRuntimeFlags {
    fn add(&mut self, language: LanguageKind) {
        match language {
            LanguageKind::Cpp => self.cpp = true,
            LanguageKind::Rust => self.rust = true,
        }
    }

    fn is_mixed(&self) -> bool {
        self.cpp && self.rust
    }
}

#[derive(Debug, Clone)]
struct MixedProcessGroup {
    graph: String,
    process: String,
}

fn mixed_process_group(contract: &ContractIr) -> Option<MixedProcessGroup> {
    let component_languages = contract
        .components
        .iter()
        .map(|component| (component.name.as_str(), component.language))
        .collect::<BTreeMap<_, _>>();

    for graph in &contract.graphs {
        let mut processes = BTreeMap::<String, ProcessRuntimeFlags>::new();
        for instance in &graph.instances {
            let Some(language) = component_languages
                .get(instance.component.name.as_str())
                .copied()
            else {
                continue;
            };
            processes
                .entry(
                    instance
                        .process
                        .clone()
                        .unwrap_or_else(|| "main".to_string()),
                )
                .or_default()
                .add(language);
        }

        if let Some((process, _)) = processes
            .into_iter()
            .find(|(_, runtimes)| runtimes.is_mixed())
        {
            return Some(MixedProcessGroup {
                graph: graph.name.clone(),
                process,
            });
        }
    }

    None
}

fn selected_runtime_backend_name(contract: &ContractIr) -> &str {
    contract
        .profiles
        .iter()
        .find(|profile| profile.name == "default")
        .or_else(|| contract.profiles.first())
        .map(|profile| profile.backend.0.as_str())
        .unwrap_or("inproc")
}

fn ensure_backend_runtime_supported(_contract: &ContractIr, _command: &str) -> Result<()> {
    Ok(())
}

fn build_workspace(contract: &ContractIr, out_dir: &Path, include_launcher: bool) -> Result<()> {
    ensure_backend_runtime_supported(contract, "build")?;
    let rust_runtime_dir = rust_runtime_dir_for_generated_build()?;
    for step in build_steps(contract, include_launcher) {
        match step {
            BuildStep::CargoApp => {
                let manifest =
                    cargo_manifest_with_runtime_patch(out_dir, rust_runtime_dir.as_deref())?;
                run_cargo_build_bin(&manifest, &app_bin_name(contract))?;
            }
            BuildStep::CargoSupervisor => {
                let manifest =
                    cargo_manifest_with_runtime_patch(out_dir, rust_runtime_dir.as_deref())?;
                run_cargo_build_bin(&manifest, &supervisor_bin_name(contract))?;
            }
            BuildStep::CmakeApp => {
                run_cmake_configure_and_build(out_dir)?;
            }
        }
    }
    Ok(())
}

fn run_workspace(
    contract: &ContractIr,
    out_dir: &Path,
    process: Option<&str>,
    run_ticks: Option<usize>,
) -> Result<()> {
    ensure_direct_runtime_supported(contract, "run")?;
    ensure_backend_runtime_supported(contract, "run")?;
    ensure_run_process_boundaries_supported(contract, process)?;
    match run_mode_for_process(contract, process)
        .context("contract does not contain runnable components")?
    {
        RunMode::CargoApp => {
            let bin = cargo_app_executable_path(contract, out_dir);
            if !bin.exists() {
                anyhow::bail!(
                    "app binary `{}` not found; run `flowrt build` first",
                    bin.display()
                );
            }
            run_binary(&bin, process, run_ticks)?;
        }
        RunMode::CmakeApp => {
            run_cmake_app(contract, out_dir, process, run_ticks)?;
        }
    }
    Ok(())
}

fn launch_workspace(contract: &ContractIr, out_dir: &Path, run_ticks: Option<usize>) -> Result<()> {
    ensure_direct_runtime_supported(contract, "launch")?;
    ensure_backend_runtime_supported(contract, "launch")?;
    ensure_launch_process_boundaries_supported(contract)?;
    let supervisor = cargo_supervisor_executable_path(contract, out_dir);
    if !supervisor.exists() {
        anyhow::bail!(
            "FlowRT supervisor `{}` not found; run `flowrt build --launcher` first",
            supervisor.display()
        );
    }
    run_supervisor_binary(&supervisor, run_ticks)?;
    Ok(())
}

fn ensure_launch_process_boundaries_supported(contract: &ContractIr) -> Result<()> {
    let backend = selected_runtime_backend_name(contract);
    if backend != "inproc" {
        return Ok(());
    }

    if let Some(boundary) = first_cross_process_bind(contract) {
        anyhow::bail!(
            "backend `inproc` cannot launch dataflow `{}` -> `{}` across process groups `{}` -> `{}`; use backend `iox2` or `zenoh`, or place both instances in the same RSDL process group",
            boundary.from,
            boundary.to,
            boundary.from_process,
            boundary.to_process
        );
    }

    Ok(())
}

fn ensure_run_process_boundaries_supported(
    contract: &ContractIr,
    process: Option<&str>,
) -> Result<()> {
    let backend = selected_runtime_backend_name(contract);
    if backend != "inproc" {
        return Ok(());
    }

    let Some(process) = process else {
        return Ok(());
    };

    if let Some(boundary) = first_cross_process_bind_for_process(contract, process) {
        anyhow::bail!(
            "backend `inproc` cannot run --process `{}` because dataflow `{}` -> `{}` crosses process groups `{}` -> `{}`; use backend `iox2` or `zenoh`, run the whole inproc app, or place both instances in the same RSDL process group",
            process,
            boundary.from,
            boundary.to,
            boundary.from_process,
            boundary.to_process
        );
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct CrossProcessBind {
    from: String,
    to: String,
    from_process: String,
    to_process: String,
}

fn first_cross_process_bind(contract: &ContractIr) -> Option<CrossProcessBind> {
    first_cross_process_bind_matching(contract, |_| true)
}

fn first_cross_process_bind_for_process(
    contract: &ContractIr,
    process: &str,
) -> Option<CrossProcessBind> {
    first_cross_process_bind_matching(contract, |boundary| {
        boundary.from_process == process || boundary.to_process == process
    })
}

fn first_cross_process_bind_matching(
    contract: &ContractIr,
    matches: impl Fn(&CrossProcessBind) -> bool,
) -> Option<CrossProcessBind> {
    for graph in &contract.graphs {
        let processes = graph
            .instances
            .iter()
            .map(|instance| {
                (
                    instance.name.as_str(),
                    instance.process.as_deref().unwrap_or("main").to_string(),
                )
            })
            .collect::<BTreeMap<_, _>>();

        for bind in &graph.binds {
            let from_process = processes.get(bind.from.instance.name.as_str())?;
            let to_process = processes.get(bind.to.instance.name.as_str())?;
            if from_process != to_process {
                let boundary = CrossProcessBind {
                    from: format!("{}.{}", bind.from.instance.name, bind.from.port),
                    to: format!("{}.{}", bind.to.instance.name, bind.to.port),
                    from_process: from_process.clone(),
                    to_process: to_process.clone(),
                };
                if matches(&boundary) {
                    return Some(boundary);
                }
            }
        }
    }

    None
}

fn cargo_manifest_with_runtime_patch(
    out_dir: &Path,
    runtime_dir: Option<&Path>,
) -> Result<PathBuf> {
    let generated_manifest = out_dir.join("build").join("Cargo.toml");
    let generated = fs::read_to_string(&generated_manifest)
        .with_context(|| format!("failed to read `{}`", generated_manifest.display()))?;
    if generated.contains("[patch.crates-io]") || !manifest_declares_flowrt_dependency(&generated) {
        return Ok(generated_manifest);
    }
    let Some(runtime_dir) = runtime_dir else {
        return Ok(generated_manifest);
    };
    let patched = format!(
        "{generated}\n[patch.crates-io]\nflowrt = {{ path = {} }}\n",
        toml_basic_string(runtime_dir)
    );
    fs::write(&generated_manifest, patched)
        .with_context(|| format!("failed to write `{}`", generated_manifest.display()))?;
    Ok(generated_manifest)
}

fn manifest_declares_flowrt_dependency(manifest: &str) -> bool {
    manifest
        .lines()
        .any(|line| line.trim_start().starts_with("flowrt ="))
}

fn run_cmake_configure_and_build(out_dir: &Path) -> Result<()> {
    let source_dir = out_dir.join("build");
    let build_dir = source_dir.join("cmake");
    let runtime_dir = cpp_runtime_dir_for_generated_build()?;
    run_cmake_configure(&source_dir, &build_dir, runtime_dir.as_deref())?;
    run_cmake_build(&build_dir)
}

fn run_cmake_configure(
    source_dir: &Path,
    build_dir: &Path,
    runtime_dir: Option<&Path>,
) -> Result<()> {
    let args = cmake_configure_args(source_dir, build_dir, runtime_dir);
    let status = ProcessCommand::new("cmake")
        .args(args)
        .status()
        .context("failed to spawn cmake configure")?;
    if !status.success() {
        anyhow::bail!("cmake configure failed with status {status}");
    }
    Ok(())
}

fn cmake_configure_args(
    source_dir: &Path,
    build_dir: &Path,
    runtime_dir: Option<&Path>,
) -> Vec<String> {
    let mut args = vec![
        "-S".to_string(),
        source_dir.to_string_lossy().into_owned(),
        "-B".to_string(),
        build_dir.to_string_lossy().into_owned(),
    ];
    if let Some(runtime_dir) = runtime_dir {
        args.push(format!(
            "-DFLOWRT_CPP_RUNTIME_DIR={}",
            runtime_dir.to_string_lossy()
        ));
    }
    args
}

fn run_cmake_build(build_dir: &Path) -> Result<()> {
    let status = ProcessCommand::new("cmake")
        .arg("--build")
        .arg(build_dir)
        .status()
        .context("failed to spawn cmake build")?;
    if !status.success() {
        anyhow::bail!("cmake build failed with status {status}");
    }
    Ok(())
}

fn run_cmake_app(
    contract: &ContractIr,
    out_dir: &Path,
    process: Option<&str>,
    run_ticks: Option<usize>,
) -> Result<()> {
    let app = cpp_app_executable_path(contract, out_dir);
    if !app.exists() {
        anyhow::bail!(
            "C++ app executable `{}` not found; run `flowrt build` first",
            app.display()
        );
    }
    let mut command = ProcessCommand::new(&app);
    if let Some(process) = process {
        command.arg("--process").arg(process);
    }
    if let Some(run_ticks) = run_ticks {
        command.arg("--flowrt-run-ticks").arg(run_ticks.to_string());
    }
    let status = command
        .status()
        .with_context(|| format!("failed to spawn C++ app `{}`", app.display()))?;
    if !status.success() {
        anyhow::bail!("C++ app invocation failed with status {status}");
    }
    Ok(())
}

fn cpp_app_executable_path(contract: &ContractIr, out_dir: &Path) -> PathBuf {
    out_dir
        .join("build")
        .join("cmake")
        .join(cpp_app_executable_name(contract))
}

fn cpp_app_executable_name(contract: &ContractIr) -> String {
    format!(
        "{}_cpp_app{}",
        sanitize_package_name(&contract.package.name).replace('-', "_"),
        std::env::consts::EXE_SUFFIX
    )
}

fn cargo_app_executable_path(contract: &ContractIr, out_dir: &Path) -> PathBuf {
    cargo_bin_executable_path(out_dir, &app_bin_name(contract))
}

fn cargo_supervisor_executable_path(contract: &ContractIr, out_dir: &Path) -> PathBuf {
    cargo_bin_executable_path(out_dir, &supervisor_bin_name(contract))
}

fn cargo_bin_executable_path(out_dir: &Path, bin_name: &str) -> PathBuf {
    out_dir
        .join("build")
        .join("target")
        .join("debug")
        .join(format!("{bin_name}{}", std::env::consts::EXE_SUFFIX))
}

fn run_binary(binary: &Path, process: Option<&str>, run_ticks: Option<usize>) -> Result<()> {
    let mut command = ProcessCommand::new(binary);
    if let Some(process) = process {
        command.arg("--process").arg(process);
    }
    if let Some(run_ticks) = run_ticks {
        command.arg("--flowrt-run-ticks").arg(run_ticks.to_string());
    }
    let status = command
        .status()
        .with_context(|| format!("failed to spawn `{}`", binary.display()))?;
    if !status.success() {
        anyhow::bail!("app invocation failed with status {status}");
    }
    Ok(())
}

fn run_cargo_build_bin(manifest: &Path, bin_name: &str) -> Result<()> {
    let status = ProcessCommand::new("cargo")
        .arg("build")
        .arg("--manifest-path")
        .arg(manifest)
        .arg("--bin")
        .arg(bin_name)
        .status()
        .context("failed to spawn cargo")?;
    if !status.success() {
        anyhow::bail!("cargo invocation failed with status {status}");
    }
    Ok(())
}

fn run_supervisor_binary(binary: &Path, run_ticks: Option<usize>) -> Result<()> {
    let mut command = ProcessCommand::new(binary);
    if let Some(run_ticks) = run_ticks {
        command.arg("--flowrt-run-ticks").arg(run_ticks.to_string());
    }
    let status = command
        .status()
        .with_context(|| format!("failed to spawn `{}`", binary.display()))?;
    if !status.success() {
        anyhow::bail!("FlowRT supervisor invocation failed with status {status}");
    }
    Ok(())
}

fn resolve_output_dir(rsdl: &Path, out_dir: &Path) -> Result<PathBuf> {
    if out_dir.is_absolute() {
        return Ok(out_dir.to_path_buf());
    }
    Ok(application_root_from_rsdl(rsdl)?.join(out_dir))
}

fn application_root_from_rsdl(rsdl: &Path) -> Result<PathBuf> {
    for ancestor in rsdl.ancestors() {
        if ancestor.file_name() == Some(OsStr::new("rsdl")) {
            return ancestor
                .parent()
                .map(Path::to_path_buf)
                .context("failed to resolve application root from `rsdl/` directory");
        }
    }
    rsdl.parent()
        .map(Path::to_path_buf)
        .context("failed to resolve application root from RSDL path")
}

fn rust_runtime_dir_for_generated_build() -> Result<Option<PathBuf>> {
    if let Some(runtime_dir) =
        runtime_dir_from_env("FLOWRT_RUST_RUNTIME_DIR", "Cargo.toml", "Rust")?
    {
        return Ok(Some(runtime_dir));
    }
    if let Some(runtime_dir) = installed_runtime_dir("runtime/rust", "Cargo.toml")? {
        return Ok(Some(runtime_dir));
    }
    Ok(repo_runtime_dir("runtime/rust", "Cargo.toml"))
}

fn cpp_runtime_dir_for_generated_build() -> Result<Option<PathBuf>> {
    if let Some(runtime_dir) = runtime_dir_from_env(
        "FLOWRT_CPP_RUNTIME_DIR",
        "include/flowrt/runtime.hpp",
        "C++",
    )? {
        return Ok(Some(runtime_dir));
    }
    if let Some(runtime_dir) = installed_runtime_dir("runtime/cpp", "include/flowrt/runtime.hpp")? {
        return Ok(Some(runtime_dir));
    }
    Ok(repo_runtime_dir(
        "runtime/cpp",
        "include/flowrt/runtime.hpp",
    ))
}

fn runtime_dir_from_env(
    var_name: &str,
    marker: &str,
    runtime_name: &str,
) -> Result<Option<PathBuf>> {
    let Some(raw) = env::var_os(var_name) else {
        return Ok(None);
    };
    let runtime_dir = PathBuf::from(raw);
    if runtime_dir.join(marker).exists() {
        return Ok(Some(runtime_dir));
    }
    anyhow::bail!(
        "{var_name} points to `{}`, but `{}` is missing; set it to a valid FlowRT {runtime_name} runtime directory",
        runtime_dir.display(),
        runtime_dir.join(marker).display()
    );
}

fn installed_runtime_dir(relative: &str, marker: &str) -> Result<Option<PathBuf>> {
    let current_exe = env::current_exe().context("failed to resolve current flowrt executable")?;
    let Some(bin_dir) = current_exe.parent() else {
        return Ok(None);
    };
    let Some(prefix) = bin_dir.parent() else {
        return Ok(None);
    };
    let runtime_dir = prefix.join("share").join("flowrt").join(relative);
    if runtime_dir.join(marker).exists() {
        Ok(Some(runtime_dir))
    } else {
        Ok(None)
    }
}

fn repo_runtime_dir(relative: &str, marker: &str) -> Option<PathBuf> {
    let repo_root = repo_root_dir().ok()?;
    let runtime_dir = repo_root.join(relative);
    runtime_dir.join(marker).exists().then_some(runtime_dir)
}

fn repo_root_dir() -> Result<PathBuf> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    fs::canonicalize(&repo_root).with_context(|| {
        format!(
            "failed to resolve repository root from `{}`",
            repo_root.display()
        )
    })
}

fn toml_basic_string(path: &Path) -> String {
    let escaped = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('\"', "\\\"");
    format!("\"{escaped}\"")
}

fn supervisor_bin_name(contract: &ContractIr) -> String {
    format!(
        "{}-flowrt-supervisor",
        sanitize_package_name(&contract.package.name).replace('_', "-")
    )
}

fn app_bin_name(contract: &ContractIr) -> String {
    format!(
        "{}-flowrt-app",
        sanitize_package_name(&contract.package.name).replace('_', "-")
    )
}

fn sanitize_package_name(name: &str) -> String {
    let mut output = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.is_empty() {
        "flowrt-app".to_string()
    } else {
        output
    }
}

fn ensure_safe_relative_path(path: &Path) -> Result<()> {
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => anyhow::bail!("unsafe artifact path `{}`", path.display()),
        }
    }
    Ok(())
}

fn summary(contract: &ContractIr) -> String {
    let graph = contract.graphs.first();
    let instance_count = graph.map(|graph| graph.instances.len()).unwrap_or(0);
    let task_count = graph.map(|graph| graph.tasks.len()).unwrap_or(0);
    let bind_count = graph.map(|graph| graph.binds.len()).unwrap_or(0);
    format!(
        "package={} types={} components={} instances={} tasks={} binds={}",
        contract.package.name,
        contract.types.len(),
        contract.components.len(),
        instance_count,
        task_count,
        bind_count
    )
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;
    use flowrt_rsdl::parse_str;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn contract_from_source(source: &str) -> ContractIr {
        let raw = parse_str(source).unwrap();
        let contract = normalize_document(&raw, hash_source(source)).unwrap();
        validate_contract(&contract).unwrap();
        contract
    }

    fn temp_test_dir(test_name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("flowrt-{test_name}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn build_plan_selects_cargo_for_rust_contract() {
        let contract = contract_from_source(
            r#"
[package]
name = "rust_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
"#,
        );

        assert_eq!(build_steps(&contract, false), vec![BuildStep::CargoApp]);
        assert_eq!(
            build_steps(&contract, true),
            vec![BuildStep::CargoApp, BuildStep::CargoSupervisor]
        );
    }

    #[test]
    fn build_plan_selects_cmake_for_cpp_contract() {
        let contract = contract_from_source(
            r#"
[package]
name = "cpp_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"
"#,
        );

        assert_eq!(build_steps(&contract, false), vec![BuildStep::CmakeApp]);
        assert_eq!(
            build_steps(&contract, true),
            vec![BuildStep::CmakeApp, BuildStep::CargoSupervisor]
        );
    }

    #[test]
    fn default_build_plan_does_not_build_launcher() {
        let contract = contract_from_source(
            r#"
[package]
name = "cpp_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"
"#,
        );

        assert!(!build_steps(&contract, false).contains(&BuildStep::CargoSupervisor));
        assert!(build_steps(&contract, true).contains(&BuildStep::CargoSupervisor));
    }

    #[test]
    fn build_plan_selects_cargo_and_cmake_for_mixed_contract() {
        let contract = contract_from_source(
            r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[component.cpp_worker]
language = "cpp"

[component.rust_worker]
language = "rust"
"#,
        );

        assert_eq!(
            build_steps(&contract, false),
            vec![BuildStep::CargoApp, BuildStep::CmakeApp]
        );
        assert_eq!(
            build_steps(&contract, true),
            vec![
                BuildStep::CargoApp,
                BuildStep::CmakeApp,
                BuildStep::CargoSupervisor
            ]
        );
    }

    #[test]
    fn run_mode_selects_cmake_app_only_for_cpp_only_contracts() {
        let cpp_contract = contract_from_source(
            r#"
[package]
name = "cpp_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"
"#,
        );
        assert_eq!(run_mode(&cpp_contract), Some(RunMode::CmakeApp));

        let rust_contract = contract_from_source(
            r#"
[package]
name = "rust_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
"#,
        );
        assert_eq!(run_mode(&rust_contract), Some(RunMode::CargoApp));

        let mixed_contract = contract_from_source(
            r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[component.cpp_worker]
language = "cpp"

[component.rust_worker]
language = "rust"
"#,
        );
        assert_eq!(run_mode(&mixed_contract), None);
        assert!(is_mixed_language_contract(&mixed_contract));
        let error = ensure_direct_runtime_supported(&mixed_contract, "run").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("mixed-language `run` requires backend `iox2` or `zenoh`")
        );
    }

    #[test]
    fn run_mode_selects_app_by_process_for_mixed_iox2_contracts() {
        let contract = contract_from_source(
            r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "rust_main"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "cpp_main"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );

        assert_eq!(
            run_mode_for_process(&contract, Some("rust_main")).unwrap(),
            RunMode::CargoApp
        );
        assert_eq!(
            run_mode_for_process(&contract, Some("cpp_main")).unwrap(),
            RunMode::CmakeApp
        );
        assert!(run_mode_for_process(&contract, None).is_err());
    }

    #[test]
    fn mixed_runtime_readiness_rejects_same_process_mixed_components() {
        let contract = contract_from_source(
            r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "main"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "main"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );

        let error = ensure_direct_runtime_supported(&contract, "launch").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("process `main`"));
        assert!(message.contains("contains both C++ and Rust components"));
        assert!(message.contains("split them into language-specific RSDL process groups"));
    }

    #[test]
    fn mixed_runtime_readiness_rejects_inproc_cross_process_components() {
        let contract = contract_from_source(
            r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "rust_main"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "cpp_main"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );

        let error = ensure_direct_runtime_supported(&contract, "launch").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("mixed-language `launch` requires backend `iox2` or `zenoh`"));
        assert!(message.contains("selected backend `inproc`"));
    }

    #[test]
    fn mixed_runtime_readiness_allows_iox2_cross_process_components() {
        let contract = contract_from_source(
            r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "rust_main"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "cpp_main"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );

        ensure_direct_runtime_supported(&contract, "launch").unwrap();
    }

    #[test]
    fn mixed_runtime_readiness_allows_zenoh_cross_process_components() {
        let contract = contract_from_source(
            r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "rust_main"
target = "dev_host"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "cpp_main"
target = "pi_host"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "zenoh"
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[target.dev_host]
runtime = ["rust"]
backends = ["zenoh"]

[target.pi_host]
runtime = ["cpp"]
backends = ["zenoh"]
"#,
        );

        ensure_direct_runtime_supported(&contract, "launch").unwrap();
        ensure_launch_process_boundaries_supported(&contract).unwrap();
    }

    #[test]
    fn launch_readiness_rejects_inproc_dataflow_across_process_groups() {
        let contract = contract_from_source(
            r#"
[package]
name = "split_rust_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "source_process"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "sink_process"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );

        let error = ensure_launch_process_boundaries_supported(&contract).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("backend `inproc`"));
        assert!(message.contains("source_process"));
        assert!(message.contains("sink_process"));
    }

    #[test]
    fn run_process_readiness_rejects_inproc_dataflow_across_process_groups() {
        let contract = contract_from_source(
            r#"
[package]
name = "split_rust_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "source_process"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "sink_process"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );

        let error =
            ensure_run_process_boundaries_supported(&contract, Some("sink_process")).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("backend `inproc`"));
        assert!(message.contains("source_process"));
        assert!(message.contains("sink_process"));
        assert!(message.contains("run --process"));
        ensure_run_process_boundaries_supported(&contract, None).unwrap();
    }

    #[test]
    fn backend_runtime_readiness_allows_cpp_iox2_contracts() {
        let contract = contract_from_source(
            r#"
[package]
name = "cpp_iox2_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );

        ensure_backend_runtime_supported(&contract, "build").unwrap();
        ensure_backend_runtime_supported(&contract, "run").unwrap();
    }

    #[test]
    fn backend_runtime_readiness_allows_rust_iox2_contracts() {
        let contract = contract_from_source(
            r#"
[package]
name = "rust_iox2_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );

        ensure_backend_runtime_supported(&contract, "build").unwrap();
    }

    #[test]
    fn cli_exposes_installed_binary_metadata() {
        let command = Cli::command();

        assert_eq!(command.get_name(), "flowrt");
        assert_eq!(command.get_version(), Some(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn self_description_sidecar_drives_list_and_nodes_output() {
        let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [{
      "name": "source",
      "component": "imu_sim",
      "process": "main",
      "target": null,
      "runtime": "rust"
    }],
    "tasks": [{ "instance": "source", "trigger": "periodic" }],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 8 }]
}
"#;
        let root = temp_test_dir("selfdesc-sidecar");
        let path = root.join("selfdesc.json");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&path, source).unwrap();

        let self_description = load_self_description(&path).unwrap();
        let list = self_description_summary(&self_description);
        let nodes = self_description_nodes(&self_description);

        assert!(list.contains("package=robot_demo"));
        assert!(list.contains("channel source.imu -> sink.imu type=Imu"));
        assert!(list.contains("message Imu size=8"));
        assert!(nodes.contains("source process=main runtime=rust component=imu_sim"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reads_self_description_from_object_section() {
        let root = temp_test_dir("selfdesc-section");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "selfdesc-section-test"
version = "0.1.0"
edition = "2024"

[workspace]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            r##"
#[used]
#[unsafe(link_section = ".flowrt.selfdesc")]
static FLOWRT_SELF_DESCRIPTION: [u8; 253] = *br#"{
  "self_description_version": "0.1",
  "source_hash": "feedface",
  "package": { "name": "binary_demo" },
  "graphs": [{ "name": "default", "instances": [], "tasks": [], "channels": [] }],
  "message_abi": [{ "type_name": "Ping", "size_bytes": 4 }]
}
"#;

fn main() {}
"##,
        )
        .unwrap();

        let status = ProcessCommand::new("cargo")
            .arg("build")
            .arg("--quiet")
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(status.success());

        let binary_name = if cfg!(windows) {
            "selfdesc-section-test.exe"
        } else {
            "selfdesc-section-test"
        };
        let binary = root.join("target/debug").join(binary_name);
        let self_description = load_self_description(&binary).unwrap();

        assert_eq!(self_description.package.name, "binary_demo");
        assert_eq!(self_description.message_abi[0].type_name, "Ping");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn live_status_summary_reads_runtime_socket_handshake() {
        let root = temp_test_dir("live-status");
        let socket = root.join("main.sock");
        let handshake = flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 77,
            started_at_unix_ms: 1234,
            self_description_hash: "feedface".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = flowrt::IntrospectionState::new();
        state.register_channel("source.imu_to_sink.imu", "Imu");
        for _ in 0..9 {
            state.record_tick();
        }
        for _ in 0..4 {
            state.record_channel_publish_bytes(
                "source.imu_to_sink.imu",
                "Imu",
                vec![0u8; 48],
                None,
            );
        }
        let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
            .expect("status server should start");

        let output = live_status_summary_for_sockets(vec![socket]).unwrap();

        assert!(output.contains("pid=77"));
        assert!(output.contains("package=robot_demo"));
        assert!(output.contains("process=main"));
        assert!(output.contains("selfdesc=feedface"));
        assert!(output.contains("ticks=9"));
        assert!(output.contains("channels=1"));

        drop(server);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cli_parses_hz_command_with_socket_and_window() {
        let cli = Cli::try_parse_from([
            "flowrt",
            "hz",
            "source.imu_to_sink.imu",
            "--socket",
            "/tmp/flowrt-main.sock",
            "--window-ms",
            "250",
        ])
        .unwrap();

        let Command::Hz {
            channel,
            socket,
            window_ms,
        } = cli.command
        else {
            panic!("hz command should parse into Command::Hz")
        };

        assert_eq!(channel.as_deref(), Some("source.imu_to_sink.imu"));
        assert_eq!(socket, Some(PathBuf::from("/tmp/flowrt-main.sock")));
        assert_eq!(window_ms, 250);
    }

    #[test]
    fn cli_rejects_zero_hz_window() {
        let error = Cli::try_parse_from(["flowrt", "hz", "--window-ms", "0"])
            .expect_err("zero hz window should be rejected");

        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn live_hz_summary_formats_channel_delta_rate() {
        let first = flowrt::IntrospectionResponse::Status {
            handshake: flowrt::IntrospectionHandshake {
                protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
                pid: 77,
                started_at_unix_ms: 1234,
                self_description_hash: "feedface".to_string(),
                package: "robot_demo".to_string(),
                process: "main".to_string(),
                runtime: "rust".to_string(),
            },
            status: flowrt::IntrospectionStatus {
                tick_count: 10,
                channels: vec![flowrt::IntrospectionChannelStatus {
                    name: "source.imu_to_sink.imu".to_string(),
                    message_type: "Imu".to_string(),
                    published_count: 100,
                    last_payload_len: None,
                    active_observers: 0,
                    dropped_samples: 0,
                }],
            },
        };
        let second = flowrt::IntrospectionResponse::Status {
            handshake: flowrt::IntrospectionHandshake {
                protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
                pid: 77,
                started_at_unix_ms: 1234,
                self_description_hash: "feedface".to_string(),
                package: "robot_demo".to_string(),
                process: "main".to_string(),
                runtime: "rust".to_string(),
            },
            status: flowrt::IntrospectionStatus {
                tick_count: 20,
                channels: vec![flowrt::IntrospectionChannelStatus {
                    name: "source.imu_to_sink.imu".to_string(),
                    message_type: "Imu".to_string(),
                    published_count: 150,
                    last_payload_len: None,
                    active_observers: 0,
                    dropped_samples: 0,
                }],
            },
        };

        let output =
            format_hz_summary_from_status_pair(&first, &second, Duration::from_millis(500))
                .expect("hz summary should format status pair");

        assert!(output.contains("channel=source.imu_to_sink.imu"));
        assert!(output.contains("type=Imu"));
        assert!(output.contains("delta=50"));
        assert!(output.contains("hz=100.00"));
    }

    #[test]
    fn live_hz_summary_reads_status_without_enabling_probe() {
        let root = temp_test_dir("live-hz");
        let socket = root.join("main.sock");
        let handshake = flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 77,
            started_at_unix_ms: 1234,
            self_description_hash: "feedface".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = flowrt::IntrospectionState::new();
        state.register_channel("source.imu_to_sink.imu", "Imu");
        state.record_channel_publish_bytes("source.imu_to_sink.imu", "Imu", vec![0u8; 48], None);
        let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state.clone())
            .expect("status server should start");
        let publish_state = state.clone();
        let publisher = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            for _ in 0..5 {
                publish_state.record_channel_publish_bytes(
                    "source.imu_to_sink.imu",
                    "Imu",
                    vec![0u8; 48],
                    None,
                );
            }
        });

        let output = live_hz_summary_for_sockets(
            Some("source.imu_to_sink.imu"),
            vec![socket],
            Duration::from_millis(50),
        )
        .unwrap();
        publisher.join().unwrap();

        assert!(output.contains("channel=source.imu_to_sink.imu"));
        assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(0));

        drop(server);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn live_hz_summary_reports_stale_socket_without_failing_scan() {
        let root = temp_test_dir("live-hz-stale");
        let socket = root.join("missing.sock");
        std::fs::create_dir_all(&root).unwrap();

        let output =
            live_hz_summary_for_sockets(None, vec![socket.clone()], Duration::from_millis(1))
                .expect("stale socket should be reported as a line");

        assert!(output.contains(&format!("stale socket={}", socket.display())));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cli_parses_echo_command_with_optional_socket() {
        let cli = Cli::try_parse_from([
            "flowrt",
            "echo",
            "flowrt/selfdesc/selfdesc.json",
            "source.imu_to_sink.imu",
            "--socket",
            "/tmp/flowrt-main.sock",
        ])
        .unwrap();

        let Command::Echo {
            target,
            image,
            channel,
            socket,
            follow,
            interval_ms,
        } = cli.command
        else {
            panic!("echo command should parse into Command::Echo")
        };

        assert_eq!(target, "flowrt/selfdesc/selfdesc.json");
        assert_eq!(image, None);
        assert_eq!(channel.as_deref(), Some("source.imu_to_sink.imu"));
        assert_eq!(socket, Some(PathBuf::from("/tmp/flowrt-main.sock")));
        assert!(!follow);
        assert_eq!(interval_ms, 250);
    }

    #[test]
    fn cli_parses_echo_channel_without_image() {
        let cli = Cli::try_parse_from([
            "flowrt",
            "echo",
            "source.imu_to_sink.imu",
            "--socket",
            "/tmp/flowrt-main.sock",
        ])
        .unwrap();

        let Command::Echo {
            target,
            image,
            channel,
            socket,
            ..
        } = cli.command
        else {
            panic!("echo command should parse into Command::Echo")
        };

        assert_eq!(target, "source.imu_to_sink.imu");
        assert_eq!(image, None);
        assert_eq!(channel, None);
        assert_eq!(socket, Some(PathBuf::from("/tmp/flowrt-main.sock")));
    }

    #[test]
    fn cli_parses_echo_image_option() {
        let cli = Cli::try_parse_from([
            "flowrt",
            "echo",
            "source.imu_to_sink.imu",
            "--image",
            "flowrt/selfdesc/selfdesc.json",
        ])
        .unwrap();

        let Command::Echo {
            target,
            image,
            channel,
            ..
        } = cli.command
        else {
            panic!("echo command should parse into Command::Echo")
        };

        assert_eq!(target, "source.imu_to_sink.imu");
        assert_eq!(image, Some(PathBuf::from("flowrt/selfdesc/selfdesc.json")));
        assert_eq!(channel, None);
    }

    #[test]
    fn cli_parses_echo_follow_options() {
        let cli = Cli::try_parse_from([
            "flowrt",
            "echo",
            "flowrt/selfdesc/selfdesc.json",
            "source.imu_to_sink.imu",
            "--follow",
            "--interval-ms",
            "10",
        ]);

        let Command::Echo {
            follow,
            interval_ms,
            ..
        } = cli.unwrap().command
        else {
            panic!("echo --follow should parse into Command::Echo")
        };

        assert!(follow);
        assert_eq!(interval_ms, 10);
    }

    #[test]
    fn cli_parses_params_set_command() {
        let cli = Cli::try_parse_from([
            "flowrt",
            "params",
            "set",
            "flowrt/selfdesc/selfdesc.json",
            "controller.kp",
            "2.5",
            "--socket",
            "/tmp/flowrt-main.sock",
        ])
        .unwrap();

        let Command::Params {
            command:
                ParamsCommand::Set {
                    image,
                    name,
                    value,
                    socket,
                },
        } = cli.command
        else {
            panic!("params set command should parse into Command::Params")
        };

        assert_eq!(image, PathBuf::from("flowrt/selfdesc/selfdesc.json"));
        assert_eq!(name, "controller.kp");
        assert_eq!(value, "2.5");
        assert_eq!(socket, Some(PathBuf::from("/tmp/flowrt-main.sock")));
    }

    #[test]
    fn cli_rejects_zero_echo_follow_interval() {
        let error = Cli::try_parse_from([
            "flowrt",
            "echo",
            "flowrt/selfdesc/selfdesc.json",
            "source.imu_to_sink.imu",
            "--follow",
            "--interval-ms",
            "0",
        ])
        .expect_err("zero follow interval should be rejected");

        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn cli_parses_run_ticks_for_run_and_launch() {
        let run_cli = Cli::try_parse_from([
            "flowrt",
            "run",
            "examples/import_demo/rsdl/robot.rsdl",
            "--process",
            "main",
            "--run-ticks",
            "5",
        ])
        .unwrap();
        let Command::Run {
            process, run_ticks, ..
        } = run_cli.command
        else {
            panic!("run command should parse into Command::Run")
        };
        assert_eq!(process.as_deref(), Some("main"));
        assert_eq!(run_ticks, Some(5));

        let launch_cli = Cli::try_parse_from([
            "flowrt",
            "launch",
            "examples/import_demo/rsdl/robot.rsdl",
            "--run-ticks",
            "7",
        ])
        .unwrap();
        let Command::Launch { run_ticks, .. } = launch_cli.command else {
            panic!("launch command should parse into Command::Launch")
        };
        assert_eq!(run_ticks, Some(7));
    }

    #[test]
    fn cli_parses_build_launcher_flag() {
        let cli = Cli::try_parse_from([
            "flowrt",
            "build",
            "examples/import_demo/rsdl/robot.rsdl",
            "--launcher",
        ])
        .unwrap();

        let Command::Build { launcher, .. } = cli.command else {
            panic!("build command should parse into Command::Build")
        };
        assert!(launcher);
    }

    #[test]
    fn cli_rejects_zero_run_ticks() {
        let error = Cli::try_parse_from([
            "flowrt",
            "run",
            "examples/import_demo/rsdl/robot.rsdl",
            "--run-ticks",
            "0",
        ])
        .expect_err("zero run tick limit should be rejected");

        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn load_prepared_contract_reports_build_required() {
        let root = temp_test_dir("missing-prepared-contract");
        let out_dir = root.join("flowrt");
        let rsdl = root.join("rsdl/robot.rsdl");

        let build_hint = build_command_hint(&rsdl, None, false);
        let error = load_prepared_contract(&out_dir, &build_hint).unwrap_err();

        let message = error.to_string();
        assert!(message.contains("generated contract"));
        assert!(message.contains("flowrt build"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prepared_profile_must_match_explicit_run_profile() {
        let contract = contract_from_source(
            r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );

        let build_hint = build_command_hint(
            Path::new("examples/profile_switch_demo/rsdl/robot.rsdl"),
            Some("iox2"),
            false,
        );
        let error =
            ensure_prepared_profile_matches(&contract, Some("iox2"), &build_hint).unwrap_err();

        let message = error.to_string();
        assert!(message.contains("prepared FlowRT artifacts use profile `default`"));
        assert!(message.contains("flowrt build --profile iox2"));
    }

    #[test]
    fn build_command_hint_includes_launcher_when_launch_needs_profile() {
        let hint = build_command_hint(
            Path::new("examples/profile_switch_demo/rsdl/robot.rsdl"),
            Some("iox2"),
            true,
        );

        assert_eq!(
            hint,
            "flowrt build --launcher --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl"
        );
    }

    #[test]
    fn launch_workspace_requires_prebuilt_supervisor() {
        let contract = contract_from_source(
            r#"
[package]
name = "launcher_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
"#,
        );
        let root = temp_test_dir("missing-launcher");

        let error = launch_workspace(&contract, &root.join("flowrt"), Some(1)).unwrap_err();

        let message = error.to_string();
        assert!(message.contains("FlowRT supervisor"));
        assert!(message.contains("flowrt build --launcher"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn echo_reads_channel_snapshot_from_fake_status_server() {
        let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
        let root = temp_test_dir("echo-snapshot");
        let selfdesc = root.join("selfdesc.json");
        let socket = root.join("main.sock");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&selfdesc, source).unwrap();

        let handshake = flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 81,
            started_at_unix_ms: 1234,
            self_description_hash: self_description_hash(source.as_bytes()),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = flowrt::IntrospectionState::new();
        state.record_channel_publish_bytes(
            "source.imu_to_sink.imu",
            "Imu",
            vec![0x01, 0x02, 0x0a, 0xff],
            Some(123),
        );
        let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
            .expect("status server should start");

        let output = echo_channel_from_image(&selfdesc, "source.imu", Some(&socket)).unwrap();

        assert!(output.contains("channel=source.imu_to_sink.imu"));
        assert!(output.contains("type=Imu"));
        assert!(output.contains("published_count=1"));
        assert!(output.contains("published_at_ms=123"));
        assert!(output.contains("payload_len=4"));
        assert!(output.contains("raw=01020aff"));

        drop(server);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn echo_formats_fixed_abi_fields_from_self_description_layout() {
        let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.count",
      "to": "sink.count",
      "message_type": "Count"
    }]
  }],
  "message_abi": [{
    "type_name": "Count",
    "size_bytes": 4,
    "align_bytes": 4,
    "fields": [{
      "name": "value",
      "type": "u32",
      "offset_bytes": 0,
      "size_bytes": 4,
      "align_bytes": 4
    }]
  }]
}
"#;
        let root = temp_test_dir("echo-format-fields");
        let selfdesc = root.join("selfdesc.json");
        let socket = root.join("main.sock");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&selfdesc, source).unwrap();

        let handshake = flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 88,
            started_at_unix_ms: 1234,
            self_description_hash: self_description_hash(source.as_bytes()),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = flowrt::IntrospectionState::new();
        state.record_channel_publish_bytes(
            "source.count_to_sink.count",
            "Count",
            vec![0x01, 0x02, 0x03, 0x04],
            Some(123),
        );
        let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
            .expect("status server should start");

        let output = echo_channel_from_image(&selfdesc, "source.count", Some(&socket)).unwrap();

        assert!(output.contains("fields={value=67305985}"));
        assert!(output.contains("raw=01020304"));

        drop(server);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn echo_formats_variable_frame_fields_from_self_description_layout() {
        let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.packet",
      "to": "sink.packet",
      "message_type": "Packet"
    }]
  }],
  "message_abi": [],
  "message_frames": [{
    "type_name": "Packet",
    "encoding": "canonical_frame_v1",
    "header_size_bytes": 17,
    "max_size_bytes": 64,
    "variable": true,
    "fields": [{
      "name": "valid",
      "type": "bool",
      "header_offset_bytes": 0,
      "header_size_bytes": 1,
      "tail_max_bytes": null
    }, {
      "name": "label",
      "type": "string<max=8>",
      "header_offset_bytes": 1,
      "header_size_bytes": 8,
      "tail_max_bytes": 8
    }, {
      "name": "samples",
      "type": "sequence<u32,max=2>",
      "header_offset_bytes": 9,
      "header_size_bytes": 8,
      "tail_max_bytes": 8
    }]
  }]
}
"#;
        let root = temp_test_dir("echo-format-frame");
        let selfdesc = root.join("selfdesc.json");
        let socket = root.join("main.sock");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&selfdesc, source).unwrap();

        let handshake = flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 89,
            started_at_unix_ms: 1234,
            self_description_hash: self_description_hash(source.as_bytes()),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let mut payload = Vec::new();
        payload.push(1);
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&2u32.to_le_bytes());
        payload.extend_from_slice(&2u32.to_le_bytes());
        payload.extend_from_slice(&8u32.to_le_bytes());
        payload.extend_from_slice(b"ok");
        payload.extend_from_slice(&10u32.to_le_bytes());
        payload.extend_from_slice(&20u32.to_le_bytes());

        let state = flowrt::IntrospectionState::new();
        state.record_channel_publish_bytes(
            "source.packet_to_sink.packet",
            "Packet",
            payload,
            Some(123),
        );
        let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
            .expect("status server should start");

        let output = echo_channel_from_image(&selfdesc, "source.packet", Some(&socket)).unwrap();

        assert!(output.contains("fields={valid=true,label=\"ok\",samples=[10,20]}"));

        drop(server);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn echo_online_loads_self_description_and_enables_probe() {
        let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.count",
      "to": "sink.count",
      "message_type": "Count"
    }]
  }],
  "message_abi": [{
    "type_name": "Count",
    "size_bytes": 4,
    "align_bytes": 4,
    "fields": [{
      "name": "value",
      "type": "u32",
      "offset_bytes": 0,
      "size_bytes": 4,
      "align_bytes": 4
    }]
  }]
}
"#;
        let root = temp_test_dir("echo-online-selfdesc");
        let socket = root.join("main.sock");
        std::fs::create_dir_all(&root).unwrap();

        let handshake = flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 90,
            started_at_unix_ms: 1234,
            self_description_hash: self_description_hash(source.as_bytes()),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = flowrt::IntrospectionState::new();
        state.set_self_description_json(source);
        state.register_channel("source.count_to_sink.count", "Count");
        assert!(
            !state
                .try_probe_channel_publish_bytes(
                    "source.count_to_sink.count",
                    "Count",
                    &[0, 0, 0, 0],
                    Some(100)
                )
                .recorded
        );
        let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state.clone())
            .expect("status server should start");

        let publisher = std::thread::spawn({
            let state = state.clone();
            move || {
                for _ in 0..100 {
                    if state.active_probe_count("source.count_to_sink.count") == Some(1) {
                        let record = state.try_probe_channel_publish_bytes(
                            "source.count_to_sink.count",
                            "Count",
                            &[0x2a, 0x00, 0x00, 0x00],
                            Some(124),
                        );
                        assert!(record.recorded);
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                panic!("echo did not enable channel probe");
            }
        });

        let output = echo_channel(
            &EchoTarget {
                image: None,
                channel: "source.count".to_string(),
            },
            Some(&socket),
        )
        .unwrap();
        publisher.join().unwrap();

        assert!(output.contains("fields={value=42}"));
        assert!(output.contains("published_at_ms=124"));
        assert!(output.contains("raw=2a000000"));

        drop(server);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn echo_follow_outputs_changed_snapshots_from_fake_status_server() {
        let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
        let root = temp_test_dir("echo-follow");
        let selfdesc = root.join("selfdesc.json");
        let socket = root.join("main.sock");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&selfdesc, source).unwrap();

        let handshake = flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 86,
            started_at_unix_ms: 1234,
            self_description_hash: self_description_hash(source.as_bytes()),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = flowrt::IntrospectionState::new();
        state.record_channel_publish_bytes(
            "source.imu_to_sink.imu",
            "Imu",
            vec![0x01, 0x02, 0x03, 0x04],
            Some(10),
        );
        let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state.clone())
            .expect("status server should start");
        let mut output = Vec::new();

        echo_channel_follow_for_polls(
            &EchoTarget {
                image: Some(selfdesc.clone()),
                channel: "source.imu".to_string(),
            },
            Some(&socket),
            std::time::Duration::from_millis(0),
            1,
            &mut output,
        )
        .unwrap();
        state.record_channel_publish_bytes(
            "source.imu_to_sink.imu",
            "Imu",
            vec![0x05, 0x06, 0x07, 0x08],
            Some(11),
        );
        echo_channel_follow_for_polls(
            &EchoTarget {
                image: Some(selfdesc.clone()),
                channel: "source.imu".to_string(),
            },
            Some(&socket),
            std::time::Duration::from_millis(0),
            2,
            &mut output,
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        let lines = output.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("published_count=1"));
        assert!(lines[0].contains("published_at_ms=10"));
        assert!(lines[0].contains("raw=01020304"));
        assert!(lines[1].contains("published_count=2"));
        assert!(lines[1].contains("published_at_ms=11"));
        assert!(lines[1].contains("raw=05060708"));

        drop(server);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn echo_auto_socket_requires_explicit_socket_for_multiple_matches() {
        let root = temp_test_dir("echo-multiple-sockets");
        let first_socket = root.join("first.sock");
        let second_socket = root.join("second.sock");
        std::fs::create_dir_all(&root).unwrap();

        let self_description_hash = "feedface".to_string();
        let state = flowrt::IntrospectionState::new();
        let first = flowrt::spawn_status_server_at(
            first_socket.clone(),
            flowrt::IntrospectionHandshake {
                protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
                pid: 91,
                started_at_unix_ms: 1,
                self_description_hash: self_description_hash.clone(),
                package: "robot_demo".to_string(),
                process: "first".to_string(),
                runtime: "rust".to_string(),
            },
            state.clone(),
        )
        .expect("first status server should start");
        let second = flowrt::spawn_status_server_at(
            second_socket.clone(),
            flowrt::IntrospectionHandshake {
                protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
                pid: 92,
                started_at_unix_ms: 2,
                self_description_hash: self_description_hash.clone(),
                package: "robot_demo".to_string(),
                process: "second".to_string(),
                runtime: "rust".to_string(),
            },
            state,
        )
        .expect("second status server should start");

        let error = select_matching_runtime_socket(
            &self_description_hash,
            vec![first_socket.clone(), second_socket.clone()],
        )
        .expect_err("multiple matching sockets should require explicit selection");

        let message = error.to_string();
        assert!(message.contains("multiple live FlowRT processes match self-description hash"));
        assert!(message.contains("--socket"));
        assert!(message.contains(&first_socket.display().to_string()));
        assert!(message.contains(&second_socket.display().to_string()));

        drop(first);
        drop(second);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn params_commands_use_selfdesc_matched_runtime_socket() {
        let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "param_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [{
      "name": "controller",
      "component": "controller",
      "process": "main",
      "runtime": "rust",
      "params": [{
        "name": "kp",
        "type": "f32",
        "update": "on_tick"
      }]
    }],
    "tasks": [],
    "channels": []
  }],
  "message_abi": []
}
"#;
        let root = temp_test_dir("params-cli");
        let selfdesc = root.join("selfdesc.json");
        let socket = root.join("main.sock");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&selfdesc, source).unwrap();

        let handshake = flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 87,
            started_at_unix_ms: 1234,
            self_description_hash: self_description_hash(source.as_bytes()),
            package: "param_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = flowrt::IntrospectionState::new();
        state.register_param(flowrt::IntrospectionParamSchema {
            name: "controller.kp".to_string(),
            ty: "f32".to_string(),
            update: "on_tick".to_string(),
            current: serde_json::json!(1.0),
            min: Some(serde_json::json!(0.0)),
            max: Some(serde_json::json!(10.0)),
            choices: Vec::new(),
        });
        let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
            .expect("status server should start");

        let list = params_list(&selfdesc, Some(&socket)).unwrap();
        assert!(list.contains("controller.kp type=f32 update=on_tick current=1.0"));

        let get = params_get(&selfdesc, "controller.kp", Some(&socket)).unwrap();
        assert!(get.contains("pending=none"));

        let set = params_set(&selfdesc, "controller.kp", "2.5", Some(&socket)).unwrap();
        assert!(set.contains("current=1.0"));
        assert!(set.contains("pending=2.5"));

        drop(server);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn echo_endpoint_alias_reports_ambiguous_channels() {
        let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "left_sink.imu",
      "message_type": "Imu"
    }, {
      "from": "source.imu",
      "to": "right_sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
        let self_description: SelfDescription = serde_json::from_str(source).unwrap();

        let error = find_echo_channel(&self_description, "source.imu").unwrap_err();

        assert!(
            error
                .to_string()
                .contains("contains multiple channels named `source.imu`")
        );
    }

    #[test]
    fn echo_reports_no_payload_when_snapshot_is_empty() {
        let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
        let root = temp_test_dir("echo-no-payload");
        let selfdesc = root.join("selfdesc.json");
        let socket = root.join("main.sock");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&selfdesc, source).unwrap();

        let handshake = flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 82,
            started_at_unix_ms: 1234,
            self_description_hash: self_description_hash(source.as_bytes()),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = flowrt::IntrospectionState::new();
        state.register_channel("source.imu_to_sink.imu", "Imu");
        let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
            .expect("status server should start");

        let output =
            echo_channel_snapshot_from_image(&selfdesc, "source.imu_to_sink.imu", Some(&socket))
                .unwrap();

        assert!(output.contains("payload_len=0"));
        assert!(output.contains("no payload"));

        drop(server);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn echo_rejects_payload_length_that_does_not_match_message_abi() {
        let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
        let root = temp_test_dir("echo-bad-payload-len");
        let selfdesc = root.join("selfdesc.json");
        let socket = root.join("main.sock");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&selfdesc, source).unwrap();

        let handshake = flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 83,
            started_at_unix_ms: 1234,
            self_description_hash: self_description_hash(source.as_bytes()),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = flowrt::IntrospectionState::new();
        state.record_channel_publish_bytes("source.imu_to_sink.imu", "Imu", vec![0x01, 0x02], None);
        let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
            .expect("status server should start");

        let error =
            echo_channel_snapshot_from_image(&selfdesc, "source.imu", Some(&socket)).unwrap_err();

        let message = error.to_string();
        assert!(message.contains("payload length 2"));
        assert!(message.contains("Message ABI size 4"));

        drop(server);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn echo_checks_explicit_socket_hash_before_snapshot_request() {
        let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
        let root = temp_test_dir("echo-wrong-socket-hash");
        let selfdesc = root.join("selfdesc.json");
        let socket = root.join("main.sock");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&selfdesc, source).unwrap();

        let handshake = flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 84,
            started_at_unix_ms: 1234,
            self_description_hash: "different_hash".to_string(),
            package: "other_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = flowrt::IntrospectionState::new();
        let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
            .expect("status server should start");

        let error =
            echo_channel_snapshot_from_image(&selfdesc, "source.imu", Some(&socket)).unwrap_err();

        let message = error.to_string();
        assert!(message.contains("self-description hash `different_hash` does not match"));
        assert!(!message.contains("failed to request channel snapshot"));

        drop(server);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn echo_reports_structured_live_channel_errors() {
        let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
        let root = temp_test_dir("echo-live-channel-error");
        let selfdesc = root.join("selfdesc.json");
        let socket = root.join("main.sock");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&selfdesc, source).unwrap();

        let handshake = flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 85,
            started_at_unix_ms: 1234,
            self_description_hash: self_description_hash(source.as_bytes()),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = flowrt::IntrospectionState::new();
        let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
            .expect("status server should start");

        let error =
            echo_channel_snapshot_from_image(&selfdesc, "source.imu", Some(&socket)).unwrap_err();

        let message = error.to_string();
        assert!(message.contains("failed to read channel snapshot `source.imu_to_sink.imu`"));
        assert!(message.contains("unknown FlowRT channel"));

        drop(server);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn workspace_lock_rejects_concurrent_access_to_same_out_dir() {
        let root = temp_test_dir("workspace-lock");
        let out_dir = root.join("flowrt");

        let first = WorkspaceLock::acquire(&out_dir).expect("first lock should be acquired");
        let error =
            WorkspaceLock::acquire(&out_dir).expect_err("second lock for same out dir should fail");

        assert!(error.to_string().contains("already in use"));
        drop(first);
        WorkspaceLock::acquire(&out_dir).expect("lock should be released on drop");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn workspace_lock_reclaims_lock_owned_by_dead_pid() {
        let root = temp_test_dir("workspace-lock-stale");
        let out_dir = root.join("flowrt");
        std::fs::create_dir_all(&out_dir).unwrap();
        std::fs::write(out_dir.join(".flowrt.lock"), "pid=99999999\nold=metadata\n").unwrap();

        let lock =
            WorkspaceLock::acquire(&out_dir).expect("unlocked stale lock file should be reclaimed");

        let contents = std::fs::read_to_string(out_dir.join(".flowrt.lock")).unwrap();
        assert_eq!(contents, format!("pid={}\n", std::process::id()));
        drop(lock);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cargo_manifest_patch_is_skipped_when_flowrt_dependency_is_absent() {
        let root = temp_test_dir("cargo-patch-skip");
        let build_dir = root.join("flowrt").join("build");
        std::fs::create_dir_all(&build_dir).unwrap();
        let manifest = build_dir.join("Cargo.toml");
        std::fs::write(
            &manifest,
            r#"[package]
name = "supervisor-only"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
        )
        .unwrap();

        let patched_manifest =
            cargo_manifest_with_runtime_patch(&root.join("flowrt"), Some(Path::new("/tmp/unused")))
                .expect("manifest without flowrt dependency should still be accepted");
        let content = std::fs::read_to_string(&patched_manifest).unwrap();

        assert!(!content.contains("[patch.crates-io]"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cargo_manifest_patch_uses_available_rust_runtime_dir() {
        let root = temp_test_dir("cargo-patch-runtime");
        let build_dir = root.join("flowrt").join("build");
        let runtime_dir = root.join("installed").join("runtime").join("rust");
        std::fs::create_dir_all(&build_dir).unwrap();
        std::fs::create_dir_all(&runtime_dir).unwrap();
        let manifest = build_dir.join("Cargo.toml");
        std::fs::write(
            &manifest,
            r#"[package]
name = "robot-flowrt-app"
version = "0.1.0"
edition = "2024"

[dependencies]
flowrt = { version = "0.1" }
"#,
        )
        .unwrap();

        let patched_manifest =
            cargo_manifest_with_runtime_patch(&root.join("flowrt"), Some(&runtime_dir))
                .expect("manifest with flowrt dependency should be patched to available runtime");
        let content = std::fs::read_to_string(&patched_manifest).unwrap();

        assert!(content.contains("[patch.crates-io]"));
        assert!(content.contains(&format!(
            "flowrt = {{ path = {} }}",
            toml_basic_string(&runtime_dir)
        )));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cargo_manifest_patch_is_skipped_when_no_runtime_dir_is_available() {
        let root = temp_test_dir("cargo-patch-no-runtime");
        let build_dir = root.join("flowrt").join("build");
        std::fs::create_dir_all(&build_dir).unwrap();
        let manifest = build_dir.join("Cargo.toml");
        std::fs::write(
            &manifest,
            r#"[package]
name = "robot-flowrt-app"
version = "0.1.0"
edition = "2024"

[dependencies]
flowrt = { version = "0.1" }
"#,
        )
        .unwrap();

        let patched_manifest = cargo_manifest_with_runtime_patch(&root.join("flowrt"), None)
            .expect("manifest should remain usable for registry-resolved flowrt");
        let content = std::fs::read_to_string(&patched_manifest).unwrap();

        assert!(!content.contains("[patch.crates-io]"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cmake_configure_args_do_not_inject_runtime_dir_by_default() {
        let source_dir = Path::new("/tmp/flowrt/build");
        let build_dir = Path::new("/tmp/flowrt/build/cmake");

        let args = cmake_configure_args(source_dir, build_dir, None);

        assert_eq!(
            args,
            vec![
                "-S".to_string(),
                "/tmp/flowrt/build".to_string(),
                "-B".to_string(),
                "/tmp/flowrt/build/cmake".to_string()
            ]
        );
    }

    #[test]
    fn cmake_configure_args_can_pass_explicit_runtime_dir() {
        let source_dir = Path::new("/tmp/flowrt/build");
        let build_dir = Path::new("/tmp/flowrt/build/cmake");
        let runtime_dir = Path::new("/opt/flowrt/runtime/cpp");

        let args = cmake_configure_args(source_dir, build_dir, Some(runtime_dir));

        assert!(args.contains(&"-DFLOWRT_CPP_RUNTIME_DIR=/opt/flowrt/runtime/cpp".to_string()));
    }

    #[test]
    fn prepare_workspace_projects_selected_profile_before_validation() {
        let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
process = "main"
target = "linux"

[instance.worker.task]
trigger = "periodic"
period_ms = 1

[profile.default]
backend = "inproc"

[profile.iox2]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#;
        let rsdl_dir = temp_test_dir("prepare-profile");
        let rsdl_path = rsdl_dir.join("robot.rsdl");
        std::fs::create_dir_all(&rsdl_dir).unwrap();
        std::fs::write(&rsdl_path, source).unwrap();
        let out_dir = rsdl_dir.join("flowrt");

        assert!(load_contract_from_rsdl(&rsdl_path).is_err());
        let prepared = prepare_workspace(&rsdl_path, &out_dir, Some("iox2"))
            .expect("selected profile should prepare");
        let prepared_ir =
            ContractIr::from_json_str(&std::fs::read_to_string(&prepared.contract_path).unwrap())
                .unwrap();

        assert_eq!(prepared_ir.profiles.len(), 1);
        assert_eq!(prepared_ir.profiles[0].name, "iox2");
        assert_eq!(prepared_ir.deployments.len(), 1);
        assert_eq!(prepared_ir.deployments[0].profile.name, "iox2");

        let _ = std::fs::remove_dir_all(&rsdl_dir);
    }

    #[test]
    fn prepare_workspace_projects_default_profile_when_selection_is_omitted() {
        let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
process = "main"
target = "linux"

[instance.worker.task]
trigger = "periodic"
period_ms = 1

[profile.default]
backend = "inproc"

[profile.iox2]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let rsdl_dir = temp_test_dir("prepare-default-profile");
        let rsdl_path = rsdl_dir.join("robot.rsdl");
        std::fs::create_dir_all(&rsdl_dir).unwrap();
        std::fs::write(&rsdl_path, source).unwrap();
        let out_dir = rsdl_dir.join("flowrt");

        assert!(load_contract_from_rsdl(&rsdl_path).is_err());
        let prepared =
            prepare_workspace(&rsdl_path, &out_dir, None).expect("default profile should prepare");
        let prepared_ir =
            ContractIr::from_json_str(&std::fs::read_to_string(&prepared.contract_path).unwrap())
                .unwrap();

        assert_eq!(prepared_ir.profiles.len(), 1);
        assert_eq!(prepared_ir.profiles[0].name, "default");
        assert_eq!(prepared_ir.deployments.len(), 1);
        assert_eq!(prepared_ir.deployments[0].profile.name, "default");

        let _ = std::fs::remove_dir_all(&rsdl_dir);
    }

    #[test]
    fn prepare_workspace_writes_projected_channel_policy_to_managed_artifacts() {
        let source = r#"
[package]
name = "profile_policy_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["defaulted:Sample", "explicit:Sample"]

[component.consumer]
language = "rust"
input = ["defaulted:Sample", "explicit:Sample"]

[instance.producer]
component = "producer"
process = "main"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 1
output = ["defaulted", "explicit"]

[instance.consumer]
component = "consumer"
process = "main"
target = "linux"

[instance.consumer.task]
trigger = "on_message"
input = ["defaulted", "explicit"]

[[bind.dataflow]]
from = "producer.defaulted"
to = "consumer.defaulted"
channel = "fifo"
depth = 2

[[bind.dataflow]]
from = "producer.explicit"
to = "consumer.explicit"
channel = "latest"
overflow = "drop_newest"
stale_policy = "hold_last"
max_age_ms = 7

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[profile.safety]
backend = "inproc"
default_overflow = "error"
default_stale_policy = "drop"
max_age_ms = 25

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let rsdl_dir = temp_test_dir("prepare-profile-policy");
        let rsdl_path = rsdl_dir.join("robot.rsdl");
        std::fs::create_dir_all(&rsdl_dir).unwrap();
        std::fs::write(&rsdl_path, source).unwrap();
        let out_dir = rsdl_dir.join("flowrt");

        let prepared = prepare_workspace(&rsdl_path, &out_dir, Some("safety"))
            .expect("selected profile policy should prepare");
        let prepared_ir =
            ContractIr::from_json_str(&std::fs::read_to_string(&prepared.contract_path).unwrap())
                .unwrap();
        let defaulted_ir = prepared_ir.graphs[0]
            .binds
            .iter()
            .find(|bind| bind.to.port == "defaulted")
            .unwrap();
        let explicit_ir = prepared_ir.graphs[0]
            .binds
            .iter()
            .find(|bind| bind.to.port == "explicit")
            .unwrap();

        assert_eq!(defaulted_ir.overflow, flowrt_ir::OverflowPolicy::Error);
        assert_eq!(defaulted_ir.stale, flowrt_ir::StalePolicy::Drop);
        assert_eq!(defaulted_ir.max_age_ms, Some(25));
        assert_eq!(explicit_ir.overflow, flowrt_ir::OverflowPolicy::DropNewest);
        assert_eq!(explicit_ir.stale, flowrt_ir::StalePolicy::HoldLast);
        assert_eq!(explicit_ir.max_age_ms, Some(7));

        let launch: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(out_dir.join("launch/launch.json")).unwrap(),
        )
        .unwrap();
        let channels = launch["graphs"][0]["channels"].as_array().unwrap();
        let defaulted_launch = channels
            .iter()
            .find(|channel| channel["to"] == "consumer.defaulted")
            .unwrap();
        let explicit_launch = channels
            .iter()
            .find(|channel| channel["to"] == "consumer.explicit")
            .unwrap();

        assert_eq!(defaulted_launch["overflow"], "error");
        assert_eq!(defaulted_launch["stale_policy"], "drop");
        assert_eq!(defaulted_launch["max_age_ms"], 25);
        assert_eq!(explicit_launch["overflow"], "drop_newest");
        assert_eq!(explicit_launch["stale_policy"], "hold_last");
        assert_eq!(explicit_launch["max_age_ms"], 7);

        let _ = std::fs::remove_dir_all(&rsdl_dir);
    }
}
