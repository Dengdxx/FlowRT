use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::path::{Component, Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use flowrt_codegen::{ArtifactBundle, emit_artifacts};
use flowrt_ir::{
    ContractIr, LanguageKind, TargetPlatform, hash_source, normalize_loaded_document,
    project_contract_to_profile,
};
use flowrt_validate::validate_contract;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod build_model;
mod introspection;
mod record;
mod toolchain;

use build_model::{BuildMode, CacheLayout, DepsCacheKey, RuntimeFeatureSet, default_cache_root};
use introspection::{
    EchoTarget, echo_channel, echo_channel_follow, live_hz_summary, live_status_summary,
    load_self_description, operation_cancel, operation_list, operation_status_summary, params_get,
    params_list, params_set, remote_params_get, remote_params_list, remote_params_set,
    self_description_nodes, self_description_summary,
};
use record::{RecordOptions, record_runtime};
use toolchain::{ToolchainProfile, ToolchainProfileOverrides, resolve_toolchain_profile};

#[cfg(test)]
use flowrt_selfdesc::SelfDescription;
#[cfg(test)]
use introspection::{
    echo_channel_follow_for_polls, echo_channel_from_image, echo_channel_snapshot_from_image,
    find_echo_channel, format_hz_summary_from_status_pair, live_hz_summary_for_sockets,
    live_status_summary_for_sockets, operation_cancel_for_sockets,
    operation_status_summary_for_sockets, select_matching_runtime_socket, self_description_hash,
};

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

        /// 目标 platform；省略时优先使用 Contract IR target platform，再回退 native 构建。
        #[arg(long)]
        target: Option<String>,

        /// 构建模式；默认 release，debug 仅用于本地调试。
        #[arg(long, default_value_t, value_enum)]
        build_mode: BuildMode,
    },

    /// 补全并预热 FlowRT 底层依赖缓存。
    Deps {
        /// 可选 RSDL 文件路径；提供时按选定 profile 推导实际 backend feature。
        rsdl: Option<PathBuf>,

        /// 显式选择要预热的 backend；省略时有 RSDL 则自动推导，无 RSDL 则预热 all。
        #[arg(long, value_enum)]
        backend: Option<DepsBackend>,

        /// 选择用于推导 backend feature 的 profile 名称。
        #[arg(long)]
        profile: Option<String>,

        /// 目标 platform；省略时有 RSDL 则使用 Contract IR target platform，无 platform 则回退 native。
        #[arg(long)]
        target: Option<String>,

        /// 依赖预热模式；默认 release。
        #[arg(long, default_value_t, value_enum)]
        build_mode: BuildMode,

        /// 只检查依赖缓存是否已存在，不触发编译。
        #[arg(long)]
        check: bool,
    },

    /// 检查和列出 FlowRT external package。
    External {
        #[command(subcommand)]
        command: ExternalCommand,
    },

    /// 将已构建的 FlowRT 项目打包成本地离线 bundle。
    Bundle {
        /// .rsdl 文件路径。
        rsdl: PathBuf,

        /// FlowRT 管理产物输出目录。
        #[arg(long, default_value = "flowrt")]
        out_dir: PathBuf,

        /// bundle 输出目录。
        #[arg(long)]
        output: PathBuf,

        /// 选择用于校验 bundle 的 profile 名称。
        #[arg(long)]
        profile: Option<String>,

        /// 要打包的构建模式；省略时使用最近一次成功 build 记录的模式。
        #[arg(long, value_enum)]
        build_mode: Option<BuildMode>,
    },

    /// 通过 ssh/scp 部署本地 bundle baseline。
    Deploy {
        /// `flowrt bundle` 生成的 bundle 目录。
        bundle: PathBuf,

        /// 远端主机，格式同 ssh/scp，例如 `user@host`。
        #[arg(long)]
        host: String,

        /// 目标 target 名称。
        #[arg(long)]
        target: String,

        /// 远端目录。
        #[arg(long)]
        remote_dir: String,

        /// 只输出计划，不执行 ssh/scp。
        #[arg(long)]
        dry_run: bool,
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
        #[arg(long, visible_alias = "run-steps", value_parser = parse_positive_usize)]
        run_ticks: Option<usize>,

        /// 选择用于生成和运行的 profile 名称。
        #[arg(long)]
        profile: Option<String>,

        /// 要运行的构建模式；省略时使用最近一次成功 build 记录的模式。
        #[arg(long, value_enum)]
        build_mode: Option<BuildMode>,
    },

    /// 准备、构建并运行生成的 process supervisor。
    Launch {
        /// .rsdl 文件路径。
        rsdl: PathBuf,

        /// FlowRT 管理产物输出目录。
        #[arg(long, default_value = "flowrt")]
        out_dir: PathBuf,

        /// 显式限制生成应用最多运行多少个 tick；省略表示无限运行。
        #[arg(long, visible_alias = "run-steps", value_parser = parse_positive_usize)]
        run_ticks: Option<usize>,

        /// 选择用于生成和启动的 profile 名称。
        #[arg(long)]
        profile: Option<String>,

        /// 要启动的构建模式；省略时使用最近一次成功 build 记录的模式。
        #[arg(long, value_enum)]
        build_mode: Option<BuildMode>,
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

    /// 观察或控制 live runtime Operation。
    Op {
        #[command(subcommand)]
        command: OpCommand,
    },

    /// 扫描当前用户 runtime socket 并输出 live status。
    Status {
        /// 只输出成功响应 status 的 live runtime，隐藏 stale socket 诊断行。
        #[arg(long)]
        live_only: bool,
    },

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

    /// 按需录制 live runtime 事件到 MCAP 文件。
    Record {
        /// 输出 MCAP 文件路径。
        #[arg(long)]
        output: PathBuf,

        /// 显式指定 runtime introspection socket；省略时扫描当前用户全部 runtime socket。
        #[arg(long)]
        socket: Option<PathBuf>,

        /// 录制时长，例如 `10s`、`500ms`、`2m`；省略时直到 Ctrl-C。
        #[arg(long, value_parser = parse_record_duration)]
        duration: Option<Duration>,

        /// 只录制指定 channel，可重复。
        #[arg(long)]
        channel: Vec<String>,

        /// 只录制指定 Operation，可重复。
        #[arg(long)]
        operation: Vec<String>,

        /// 录制所有支持的 FlowRT 事件。
        #[arg(long)]
        all: bool,

        /// 允许覆盖已有输出文件。
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ParamsCommand {
    /// 列出 live runtime 参数。
    List {
        /// FlowRT 管理应用二进制，或 flowrt/selfdesc/selfdesc.json。
        #[arg(long)]
        image: Option<PathBuf>,

        /// 显式指定 runtime introspection socket；省略时按 selfdesc hash 自动匹配。
        #[arg(long)]
        socket: Option<PathBuf>,

        /// 显式指定远端 runtime key expression；仅 --remote 使用。
        #[arg(long)]
        runtime: Option<String>,

        /// 通过 zenoh control-plane 发现远端 runtime。
        #[arg(long)]
        remote: bool,

        /// 远程发现和请求超时毫秒。
        #[arg(long, default_value_t = 5000, value_parser = clap::value_parser!(u64).range(1..))]
        timeout_ms: u64,
    },

    /// 读取单个 live runtime 参数。
    Get {
        /// 参数名，格式为 `<instance>.<param>`。
        name: String,

        /// FlowRT 管理应用二进制，或 flowrt/selfdesc/selfdesc.json。
        #[arg(long)]
        image: Option<PathBuf>,

        /// 显式指定 runtime introspection socket；省略时按 selfdesc hash 自动匹配。
        #[arg(long)]
        socket: Option<PathBuf>,

        /// 显式指定远端 runtime key expression；仅 --remote 使用。
        #[arg(long)]
        runtime: Option<String>,

        /// 通过 zenoh control-plane 发现远端 runtime。
        #[arg(long)]
        remote: bool,

        /// 远程发现和请求超时毫秒。
        #[arg(long, default_value_t = 5000, value_parser = clap::value_parser!(u64).range(1..))]
        timeout_ms: u64,
    },

    /// 设置单个 live runtime 参数 pending 值。
    Set {
        /// 参数名，格式为 `<instance>.<param>`。
        name: String,

        /// JSON 参数值，例如 `2.5`、`true` 或 `"safe"`。
        value: String,

        /// FlowRT 管理应用二进制，或 flowrt/selfdesc/selfdesc.json。
        #[arg(long)]
        image: Option<PathBuf>,

        /// 显式指定 runtime introspection socket；省略时按 selfdesc hash 自动匹配。
        #[arg(long)]
        socket: Option<PathBuf>,

        /// 显式指定远端 runtime key expression；仅 --remote 使用。
        #[arg(long)]
        runtime: Option<String>,

        /// 通过 zenoh control-plane 发现远端 runtime。
        #[arg(long)]
        remote: bool,

        /// 远程发现和请求超时毫秒。
        #[arg(long, default_value_t = 5000, value_parser = clap::value_parser!(u64).range(1..))]
        timeout_ms: u64,
    },
}

#[derive(Debug, Subcommand)]
enum ExternalCommand {
    /// 检查一个 external package manifest 和 executable。
    Check {
        /// external package 目录，包含 flowrt-external.toml。
        package_dir: PathBuf,
    },

    /// 列出一个目录下可发现的 external package。
    List {
        /// external package 搜索目录。
        #[arg(long, default_value = "external")]
        path: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum OpCommand {
    /// 列出 Operation 拓扑；省略 --image 时从 live runtime 读取 self-description。
    List {
        /// FlowRT 管理应用二进制，或 flowrt/selfdesc/selfdesc.json。
        #[arg(long)]
        image: Option<PathBuf>,

        /// 显式指定 runtime introspection socket。
        #[arg(long)]
        socket: Option<PathBuf>,
    },

    /// 查看 live Operation 健康状态。
    Status {
        /// 可选 Operation 名称，格式 `<client_instance>.<client_port>`。
        name: Option<String>,

        /// 显式指定 runtime introspection socket。
        #[arg(long)]
        socket: Option<PathBuf>,
    },

    /// 取消 live Operation invocation。
    Cancel {
        /// `flowrt op status` 输出中的 operation id。
        operation_id: String,

        /// 显式指定 runtime introspection socket。
        #[arg(long)]
        socket: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum DepsBackend {
    Inproc,
    Iox2,
    Zenoh,
    All,
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
            target,
            build_mode,
        } => {
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let _lock = WorkspaceLock::acquire(&out_dir)?;
            let prepared = prepare_workspace(&rsdl, &out_dir, profile.as_deref())?;
            let workspace_root = application_root_from_rsdl(&rsdl)?;
            let target_profile = resolve_build_toolchain_profile(
                &prepared.selected_contract,
                target.as_deref(),
                &workspace_root,
            )?;
            build_workspace(
                &prepared.selected_contract,
                &out_dir,
                launcher,
                build_mode,
                target_profile.as_ref(),
            )?;
            println!(
                "built {} and {} artifact(s)",
                prepared.contract_path.display(),
                prepared.artifact_count
            );
        }
        Command::Deps {
            rsdl,
            backend,
            profile,
            target,
            build_mode,
            check,
        } => {
            let features = deps_runtime_features(rsdl.as_deref(), profile.as_deref(), backend)?;
            let target_profile = resolve_deps_toolchain_profile(
                rsdl.as_deref(),
                profile.as_deref(),
                target.as_deref(),
            )?;
            let layout = deps_cache_layout(build_mode, features.clone(), target_profile.as_ref())?;
            if check {
                ensure_deps_ready(&layout, build_mode, &features, target_profile.as_ref())?;
                println!(
                    "FlowRT dependency cache is ready: {}",
                    layout.target_dir.display()
                );
            } else {
                prepare_deps_cache(&layout, build_mode, &features, target_profile.as_ref())?;
                println!(
                    "prepared FlowRT dependency cache: {}",
                    layout.target_dir.display()
                );
            }
        }
        Command::External { command } => match command {
            ExternalCommand::Check { package_dir } => {
                println!("{}", external_check_package_dir(&package_dir)?);
            }
            ExternalCommand::List { path } => {
                println!("{}", external_list_packages(&path)?);
            }
        },
        Command::Bundle {
            rsdl,
            out_dir,
            output,
            profile,
            build_mode,
        } => {
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let build_hint = build_command_hint(&rsdl, profile.as_deref(), true);
            let contract = load_prepared_contract(&out_dir, &build_hint)?;
            ensure_prepared_profile_matches(&contract, profile.as_deref(), &build_hint)?;
            println!(
                "{}",
                bundle_workspace(&rsdl, &contract, &out_dir, &output, build_mode)?
            );
        }
        Command::Deploy {
            bundle,
            host,
            target,
            remote_dir,
            dry_run,
        } => {
            println!(
                "{}",
                deploy_bundle(&bundle, &host, &target, &remote_dir, dry_run)?
            );
        }
        Command::Run {
            rsdl,
            out_dir,
            process,
            run_ticks,
            profile,
            build_mode,
        } => {
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let build_hint = build_command_hint(&rsdl, profile.as_deref(), false);
            let contract = load_prepared_contract(&out_dir, &build_hint)?;
            ensure_prepared_profile_matches(&contract, profile.as_deref(), &build_hint)?;
            run_workspace(
                &contract,
                &out_dir,
                process.as_deref(),
                run_ticks,
                build_mode,
            )?;
        }
        Command::Launch {
            rsdl,
            out_dir,
            run_ticks,
            profile,
            build_mode,
        } => {
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let build_hint = build_command_hint(&rsdl, profile.as_deref(), true);
            let contract = load_prepared_contract(&out_dir, &build_hint)?;
            ensure_prepared_profile_matches(&contract, profile.as_deref(), &build_hint)?;
            launch_workspace(&contract, &out_dir, run_ticks, build_mode)?;
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
            ParamsCommand::List {
                image,
                socket,
                runtime,
                remote,
                timeout_ms,
            } => {
                let remote_runtime =
                    params_remote_runtime_arg(remote, socket.as_deref(), runtime.as_deref())?;
                if remote {
                    let image = require_image_for_remote(image.as_deref())?;
                    let hash = introspection::self_description_hash_for_image(&image)?;
                    println!(
                        "{}",
                        remote_params_list(&hash, remote_runtime.as_deref(), timeout_ms)?
                    );
                } else {
                    let image = require_image_for_local(image.as_deref())?;
                    println!("{}", params_list(&image, socket.as_deref())?);
                }
            }
            ParamsCommand::Get {
                name,
                image,
                socket,
                runtime,
                remote,
                timeout_ms,
            } => {
                let remote_runtime =
                    params_remote_runtime_arg(remote, socket.as_deref(), runtime.as_deref())?;
                if remote {
                    let image = require_image_for_remote(image.as_deref())?;
                    let hash = introspection::self_description_hash_for_image(&image)?;
                    println!(
                        "{}",
                        remote_params_get(&hash, &name, remote_runtime.as_deref(), timeout_ms)?
                    );
                } else {
                    let image = require_image_for_local(image.as_deref())?;
                    println!("{}", params_get(&image, &name, socket.as_deref())?);
                }
            }
            ParamsCommand::Set {
                name,
                value,
                image,
                socket,
                runtime,
                remote,
                timeout_ms,
            } => {
                let remote_runtime =
                    params_remote_runtime_arg(remote, socket.as_deref(), runtime.as_deref())?;
                if remote {
                    let image = require_image_for_remote(image.as_deref())?;
                    let hash = introspection::self_description_hash_for_image(&image)?;
                    println!(
                        "{}",
                        remote_params_set(
                            &hash,
                            &name,
                            &value,
                            remote_runtime.as_deref(),
                            timeout_ms
                        )?
                    );
                } else {
                    let image = require_image_for_local(image.as_deref())?;
                    println!("{}", params_set(&image, &name, &value, socket.as_deref())?);
                }
            }
        },
        Command::Op { command } => match command {
            OpCommand::List { image, socket } => {
                println!("{}", operation_list(image.as_deref(), socket.as_deref())?);
            }
            OpCommand::Status { name, socket } => {
                println!(
                    "{}",
                    operation_status_summary(socket.as_deref(), name.as_deref())?
                );
            }
            OpCommand::Cancel {
                operation_id,
                socket,
            } => {
                println!("{}", operation_cancel(&operation_id, socket.as_deref())?);
            }
        },
        Command::Status { live_only } => {
            println!("{}", live_status_summary(live_only)?);
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
        Command::Record {
            output,
            socket,
            duration,
            channel,
            operation,
            all,
            force,
        } => {
            println!(
                "{}",
                record_runtime(RecordOptions {
                    output,
                    socket,
                    duration,
                    channels: channel,
                    operations: operation,
                    all,
                    force,
                    poll_interval: Duration::from_millis(100),
                    shutdown: flowrt::install_signal_shutdown_token(),
                })?
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

fn parse_record_duration(raw: &str) -> std::result::Result<Duration, String> {
    let (number, unit) = raw
        .strip_suffix("ms")
        .map(|number| (number, "ms"))
        .or_else(|| raw.strip_suffix('s').map(|number| (number, "s")))
        .or_else(|| raw.strip_suffix('m').map(|number| (number, "m")))
        .unwrap_or((raw, "s"));
    let value = number.parse::<u64>().map_err(|_| {
        "duration must be a positive integer with optional ms/s/m suffix".to_string()
    })?;
    if value == 0 {
        return Err("duration must be greater than zero".to_string());
    }
    match unit {
        "ms" => Ok(Duration::from_millis(value)),
        "s" => Ok(Duration::from_secs(value)),
        "m" => Ok(Duration::from_secs(value.saturating_mul(60))),
        _ => unreachable!(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalPackageManifest {
    package: ExternalPackageMetadata,
    #[serde(default)]
    executable: Vec<ExternalExecutableMetadata>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalPackageMetadata {
    name: String,
    version: String,
    flowrt_version: String,
    license: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalExecutableMetadata {
    name: String,
    path: PathBuf,
    platforms: Vec<String>,
    backends: Vec<String>,
    health: String,
}

fn external_check_package_dir(package_dir: &Path) -> Result<String> {
    let manifest = load_external_manifest(package_dir)?;
    validate_external_manifest(package_dir, &manifest)?;
    Ok(format!(
        "external package `{}` version={} executable_count={}",
        manifest.package.name,
        manifest.package.version,
        manifest.executable.len()
    ))
}

fn external_list_packages(path: &Path) -> Result<String> {
    let mut package_dirs = Vec::new();
    if path.join("flowrt-external.toml").is_file() {
        package_dirs.push(path.to_path_buf());
    } else {
        for entry in fs::read_dir(path)
            .with_context(|| format!("failed to read external package path `{}`", path.display()))?
        {
            let entry = entry.with_context(|| {
                format!("failed to read external package path `{}`", path.display())
            })?;
            let child = entry.path();
            if child.join("flowrt-external.toml").is_file() {
                package_dirs.push(child);
            }
        }
    }
    package_dirs.sort();
    if package_dirs.is_empty() {
        anyhow::bail!(
            "no FlowRT external packages found under `{}`",
            path.display()
        );
    }

    let mut lines = Vec::new();
    for package_dir in package_dirs {
        let manifest = load_external_manifest(&package_dir)?;
        validate_external_manifest(&package_dir, &manifest)?;
        let executables = manifest
            .executable
            .iter()
            .map(|executable| {
                let platforms = canonical_external_platforms(&executable.platforms).join(",");
                format!(
                    "{} platforms=[{}] backends=[{}] health={}",
                    executable.name,
                    platforms,
                    executable.backends.join(","),
                    executable.health
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        lines.push(format!(
            "package={} version={} path={} executables={}",
            manifest.package.name,
            manifest.package.version,
            package_dir.display(),
            executables
        ));
    }
    Ok(lines.join("\n"))
}

fn load_external_manifest(package_dir: &Path) -> Result<ExternalPackageManifest> {
    let path = package_dir.join("flowrt-external.toml");
    let source = fs::read_to_string(&path)
        .with_context(|| format!("failed to read external manifest `{}`", path.display()))?;
    toml::from_str(&source)
        .with_context(|| format!("failed to parse external manifest `{}`", path.display()))
}

fn validate_external_manifest(
    package_dir: &Path,
    manifest: &ExternalPackageManifest,
) -> Result<()> {
    ensure_non_empty_manifest_field(&manifest.package.name, "package.name")?;
    ensure_non_empty_manifest_field(&manifest.package.version, "package.version")?;
    ensure_non_empty_manifest_field(&manifest.package.flowrt_version, "package.flowrt_version")?;
    ensure_non_empty_manifest_field(&manifest.package.license, "package.license")?;
    if manifest.executable.is_empty() {
        anyhow::bail!(
            "external manifest `{}` must declare at least one [[executable]]",
            package_dir.join("flowrt-external.toml").display()
        );
    }

    let mut names = std::collections::BTreeSet::new();
    for executable in &manifest.executable {
        ensure_non_empty_manifest_field(&executable.name, "executable.name")?;
        if !names.insert(executable.name.as_str()) {
            anyhow::bail!(
                "external package `{}` declares executable `{}` more than once",
                manifest.package.name,
                executable.name
            );
        }
        if executable.path.as_os_str().is_empty() {
            anyhow::bail!(
                "external package `{}` executable `{}` has empty path",
                manifest.package.name,
                executable.name
            );
        }
        let exe_path =
            validate_manifest_executable_path(package_dir, &manifest.package.name, executable)?;
        if !exe_path.is_file() {
            anyhow::bail!(
                "external package `{}` executable `{}` path does not exist: {}",
                manifest.package.name,
                executable.name,
                exe_path.display()
            );
        }
        if executable.platforms.is_empty() {
            anyhow::bail!(
                "external package `{}` executable `{}` must declare at least one platform",
                manifest.package.name,
                executable.name
            );
        }
        for platform in &executable.platforms {
            if TargetPlatform::parse_alias(platform).is_none() {
                anyhow::bail!(
                    "external package `{}` executable `{}` declares unsupported platform `{}`",
                    manifest.package.name,
                    executable.name,
                    platform
                );
            }
        }
        if executable.backends.is_empty() {
            anyhow::bail!(
                "external package `{}` executable `{}` must declare at least one backend",
                manifest.package.name,
                executable.name
            );
        }
        for backend in &executable.backends {
            if !flowrt_ir::is_known_backend(backend) {
                anyhow::bail!(
                    "external package `{}` executable `{}` declares unknown backend `{}`",
                    manifest.package.name,
                    executable.name,
                    backend
                );
            }
        }
        if !matches!(
            executable.health.as_str(),
            "process_started" | "runtime_socket"
        ) {
            anyhow::bail!(
                "external package `{}` executable `{}` declares unsupported health `{}`",
                manifest.package.name,
                executable.name,
                executable.health
            );
        }
    }
    Ok(())
}

fn canonical_external_platforms(platforms: &[String]) -> Vec<String> {
    let mut canonical = platforms
        .iter()
        .filter_map(|platform| TargetPlatform::parse_alias(platform).map(|value| value.as_str()))
        .map(str::to_string)
        .collect::<Vec<_>>();
    canonical.sort();
    canonical.dedup();
    canonical
}

fn validate_manifest_executable_path(
    package_dir: &Path,
    package_name: &str,
    executable: &ExternalExecutableMetadata,
) -> Result<PathBuf> {
    let path = &executable.path;
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        anyhow::bail!(
            "external package `{}` executable `{}` path must be package-relative without `.` or `..` components",
            package_name,
            executable.name
        );
    }
    let exe_path = package_dir.join(path);
    if exe_path.exists() {
        let package_root = package_dir.canonicalize().with_context(|| {
            format!(
                "failed to canonicalize external package root `{}`",
                package_dir.display()
            )
        })?;
        let canonical_exe = exe_path.canonicalize().with_context(|| {
            format!(
                "failed to canonicalize external package `{}` executable `{}` path `{}`",
                package_name,
                executable.name,
                exe_path.display()
            )
        })?;
        if !canonical_exe.starts_with(&package_root) {
            anyhow::bail!(
                "external package `{}` executable `{}` path escapes package root: {}",
                package_name,
                executable.name,
                exe_path.display()
            );
        }
    }
    Ok(exe_path)
}

fn ensure_non_empty_manifest_field(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("external manifest field `{field}` must not be empty");
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct BundleManifest {
    schema_version: u32,
    flowrt_version: String,
    package: String,
    profile: Option<String>,
    target: String,
    platform: Option<String>,
    build_mode: BuildMode,
    created_unix_ms: u64,
    entry: String,
    #[serde(default)]
    executables: Vec<BundleExecutable>,
    #[serde(default)]
    external_processes: Vec<BundleExternalProcess>,
    #[serde(default)]
    artifacts: Vec<BundleArtifact>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BundleExecutable {
    kind: String,
    path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct BundleExternalProcess {
    process: String,
    package: String,
    executable: String,
    path: PathBuf,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    supported_platforms: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BundleArtifact {
    kind: String,
    target: String,
    platform: Option<String>,
    path: PathBuf,
    sha256: String,
}

#[derive(Debug)]
struct LoadedBundleManifest {
    manifest: BundleManifest,
    version_warning: Option<String>,
}

#[derive(Debug, Clone)]
struct DeployArtifactSelection {
    count: usize,
    platforms: Vec<String>,
}

#[derive(Debug)]
struct BundleExecutablePlan {
    kind: String,
    source: PathBuf,
    target: String,
    platform: Option<String>,
    dest: PathBuf,
    source_sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FlowrtReleaseVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

fn bundle_workspace(
    rsdl: &Path,
    contract: &ContractIr,
    out_dir: &Path,
    output: &Path,
    requested_build_mode: Option<BuildMode>,
) -> Result<String> {
    let build_info = load_build_info(out_dir, requested_build_mode, true)?;
    let supervisor = executable_from_build_info(
        out_dir,
        build_info.executables.supervisor.as_ref(),
        "FlowRT supervisor",
        "flowrt build --launcher",
    )?;
    if !supervisor.exists() {
        anyhow::bail!(
            "FlowRT supervisor `{}` not found; run `flowrt build --launcher` first",
            supervisor.display()
        );
    }
    ensure_bundle_output_dir(output)?;
    let target_name = bundle_target_name_for_build(&build_info, contract);
    let target_platform = bundle_target_platform_for_build(&build_info, contract)?;

    copy_required_file(
        &prepared_contract_path(out_dir),
        &output.join("flowrt/contract/contract.ir.json"),
    )?;
    copy_required_file(
        &out_dir.join("selfdesc/selfdesc.json"),
        &output.join("flowrt/selfdesc/selfdesc.json"),
    )?;
    copy_required_file(
        &out_dir.join("launch/launch.json"),
        &output.join("flowrt/launch/launch.json"),
    )?;
    copy_required_file(
        &build_model::BuildInfo::path(out_dir),
        &output.join("flowrt/build/build-info.json"),
    )?;

    let mut executables = Vec::new();
    let mut artifacts = Vec::new();
    let mut strip_stats = BundleStripStats::default();
    for plan in bundle_executable_plans(
        &build_info,
        out_dir,
        &target_name,
        target_platform.as_deref(),
    )? {
        let dest_abs = output.join(&plan.dest);
        copy_required_file(&plan.source, &dest_abs)?;
        if let Some(expected_hash) = &plan.source_sha256 {
            let actual_hash = file_sha256(&plan.source)?;
            if actual_hash != *expected_hash {
                anyhow::bail!(
                    "build-info artifact `{}` sha256 mismatch before bundle: metadata has {}, actual is {}; run `{}` first",
                    plan.source.display(),
                    expected_hash,
                    actual_hash,
                    build_launcher_hint(plan.platform.as_deref())
                );
            }
        }
        strip_stats.record(strip_bundle_executable(&dest_abs)?);
        artifacts.push(BundleArtifact {
            kind: plan.kind.clone(),
            target: plan.target.clone(),
            platform: plan.platform.clone(),
            path: plan.dest.clone(),
            sha256: file_sha256(&dest_abs)?,
        });
        executables.push(BundleExecutable {
            kind: plan.kind,
            path: plan.dest,
        });
    }

    let project_root = project_root_for_rsdl(rsdl);
    let mut external_processes = Vec::new();
    for graph in &contract.graphs {
        for external in &graph.external_processes {
            let package_root = resolve_external_package_root(&project_root, external)?;
            let manifest = load_external_manifest(&package_root)?;
            validate_external_manifest(&package_root, &manifest)?;
            let executable_metadata = select_external_executable_metadata(&manifest, external)?;
            let supported_platforms = canonical_external_platforms(&executable_metadata.platforms);
            if let Some(platform) = &target_platform {
                if !supported_platforms
                    .iter()
                    .any(|candidate| candidate == platform)
                {
                    anyhow::bail!(
                        "external package `{}` executable `{}` does not support target platform `{}`",
                        external.package,
                        external.executable,
                        platform
                    );
                }
            }
            let dest = PathBuf::from("external").join(&external.package);
            copy_dir_recursive(&package_root, &output.join(&dest))?;
            let artifact_path = dest.join(&external.executable);
            artifacts.push(BundleArtifact {
                kind: "external_process".to_string(),
                target: target_name.clone(),
                platform: target_platform.clone(),
                path: artifact_path.clone(),
                sha256: file_sha256(&output.join(&artifact_path))?,
            });
            external_processes.push(BundleExternalProcess {
                process: external.process.clone(),
                package: external.package.clone(),
                executable: external.executable.clone(),
                path: dest,
                platform: target_platform.clone(),
                supported_platforms,
            });
        }
    }

    let entry = executables
        .iter()
        .find(|executable| executable.kind == "supervisor")
        .map(|executable| executable.path.clone())
        .context("internal error: bundle entry supervisor executable was not copied")?;
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: contract.package.name.clone(),
        profile: build_info.rsdl_profile,
        target: target_name,
        platform: target_platform,
        build_mode: build_info.build_mode,
        created_unix_ms: current_unix_ms(),
        entry: entry.to_string_lossy().into_owned(),
        executables,
        external_processes,
        artifacts,
    };
    let mut manifest_toml = toml::to_string_pretty(&manifest)?;
    manifest_toml.push('\n');
    fs::write(output.join("bundle.toml"), manifest_toml)
        .with_context(|| format!("failed to write `{}`", output.join("bundle.toml").display()))?;

    Ok(format!(
        "created FlowRT bundle: {} entry={} external_packages={} stripped_executables={} strip_warnings={}",
        output.display(),
        manifest.entry,
        manifest.external_processes.len(),
        strip_stats.stripped,
        strip_stats.warnings
    ))
}

fn bundle_executable_plans(
    build_info: &build_model::BuildInfo,
    out_dir: &Path,
    default_target: &str,
    default_platform: Option<&str>,
) -> Result<Vec<BundleExecutablePlan>> {
    let entries = [
        ("supervisor", build_info.executables.supervisor.as_ref()),
        ("rust_app", build_info.executables.rust_app.as_ref()),
        ("cpp_app", build_info.executables.cpp_app.as_ref()),
        ("ros2_bridge", build_info.executables.ros2_bridge.as_ref()),
    ];
    let mut plans = Vec::new();
    let has_artifact_facts = !build_info.artifacts.is_empty();
    for (kind, relative) in entries {
        let Some(relative) = relative else {
            continue;
        };
        ensure_safe_relative_path(relative)?;
        let source = out_dir.join(relative);
        if !source.exists() {
            anyhow::bail!(
                "build-info records {kind} executable `{}`, but it does not exist; run `{}` first",
                source.display(),
                build_launcher_hint(default_platform)
            );
        }
        let artifact = if has_artifact_facts {
            Some(bundle_build_artifact_for_executable(
                build_info, kind, relative,
            )?)
        } else {
            None
        };
        let (target, platform, source_sha256) = if let Some(artifact) = artifact {
            ensure_safe_relative_path(&artifact.path)?;
            if artifact.path != *relative {
                anyhow::bail!(
                    "build-info executable `{}` points to `{}`, but artifact metadata points to `{}`; run `{}` first",
                    kind,
                    relative.display(),
                    artifact.path.display(),
                    build_launcher_hint(artifact.platform.as_deref().or(default_platform))
                );
            }
            validate_build_artifact_target(kind, artifact, default_target, default_platform)?;
            (
                artifact.target.clone(),
                canonical_optional_platform(artifact.platform.as_deref())?,
                Some(artifact.sha256.clone()),
            )
        } else {
            (
                default_target.to_string(),
                canonical_optional_platform(default_platform)?,
                None,
            )
        };
        let dest = bundle_binary_dest(&source, platform.as_deref())?;
        plans.push(BundleExecutablePlan {
            kind: kind.to_string(),
            source,
            target,
            platform,
            dest,
            source_sha256,
        });
    }
    Ok(plans)
}

fn bundle_build_artifact_for_executable<'a>(
    build_info: &'a build_model::BuildInfo,
    kind: &str,
    relative: &Path,
) -> Result<&'a build_model::BuildArtifactInfo> {
    let matches = build_info
        .artifacts
        .iter()
        .filter(|artifact| artifact.kind == kind)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [artifact] => Ok(*artifact),
        [] => anyhow::bail!(
            "build-info records {kind} executable `{}`, but artifact metadata is missing; run `{}` first",
            relative.display(),
            build_launcher_hint(build_info.platform.as_deref())
        ),
        _ => anyhow::bail!(
            "build-info records multiple {kind} artifacts; run `{}` first",
            build_launcher_hint(build_info.platform.as_deref())
        ),
    }
}

fn validate_build_artifact_target(
    kind: &str,
    artifact: &build_model::BuildArtifactInfo,
    expected_target: &str,
    expected_platform: Option<&str>,
) -> Result<()> {
    if artifact.target != expected_target {
        anyhow::bail!(
            "build-info {kind} artifact target `{}` does not match Contract IR target `{expected_target}`; run `{}` first",
            artifact.target,
            build_launcher_hint(artifact.platform.as_deref().or(expected_platform))
        );
    }
    let expected_platform = canonical_optional_platform(expected_platform)?;
    let artifact_platform = canonical_optional_platform(artifact.platform.as_deref())?;
    if artifact_platform != expected_platform {
        anyhow::bail!(
            "build-info {kind} artifact platform {:?} does not match Contract IR platform {:?}; run `{}` first",
            artifact_platform,
            expected_platform,
            build_launcher_hint(
                expected_platform
                    .as_deref()
                    .or(artifact.platform.as_deref())
            )
        );
    }
    Ok(())
}

fn bundle_binary_dest(source: &Path, platform: Option<&str>) -> Result<PathBuf> {
    let file_name = source.file_name().with_context(|| {
        format!(
            "failed to determine executable file name for `{}`",
            source.display()
        )
    })?;
    let mut dest = PathBuf::from("bin");
    if let Some(platform) = platform {
        dest.push(platform);
    }
    dest.push(file_name);
    Ok(dest)
}

fn canonical_optional_platform(platform: Option<&str>) -> Result<Option<String>> {
    platform
        .map(|platform| {
            TargetPlatform::parse_alias(platform)
                .map(|value| value.as_str().to_string())
                .with_context(|| format!("unsupported target platform `{platform}`"))
        })
        .transpose()
}

#[derive(Default)]
struct BundleStripStats {
    stripped: usize,
    warnings: usize,
}

impl BundleStripStats {
    fn record(&mut self, outcome: BundleStripOutcome) {
        match outcome {
            BundleStripOutcome::Stripped => self.stripped += 1,
            BundleStripOutcome::Skipped | BundleStripOutcome::Warning => {
                if outcome == BundleStripOutcome::Warning {
                    self.warnings += 1;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BundleStripOutcome {
    Stripped,
    Skipped,
    Warning,
}

fn strip_bundle_executable(path: &Path) -> Result<BundleStripOutcome> {
    if !is_elf_file(path)? {
        return Ok(BundleStripOutcome::Skipped);
    }
    let strip = env::var_os("FLOWRT_STRIP").unwrap_or_else(|| OsStr::new("strip").to_os_string());
    let output = match ProcessCommand::new(&strip)
        .arg("--strip-unneeded")
        .arg(path)
        .output()
    {
        Ok(output) => output,
        Err(_) => return Ok(BundleStripOutcome::Warning),
    };
    if output.status.success() {
        Ok(BundleStripOutcome::Stripped)
    } else {
        Ok(BundleStripOutcome::Warning)
    }
}

fn is_elf_file(path: &Path) -> Result<bool> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open `{}`", path.display()))?;
    let mut magic = [0u8; 4];
    let read = std::io::Read::read(&mut file, &mut magic)
        .with_context(|| format!("failed to read `{}`", path.display()))?;
    Ok(read == magic.len() && magic == [0x7f, b'E', b'L', b'F'])
}

fn ensure_bundle_output_dir(output: &Path) -> Result<()> {
    if output.exists() {
        if !output.is_dir() {
            anyhow::bail!(
                "bundle output `{}` exists and is not a directory",
                output.display()
            );
        }
        if fs::read_dir(output)
            .with_context(|| format!("failed to read `{}`", output.display()))?
            .next()
            .is_some()
        {
            anyhow::bail!(
                "bundle output directory `{}` is not empty",
                output.display()
            );
        }
    }
    fs::create_dir_all(output)
        .with_context(|| format!("failed to create bundle output `{}`", output.display()))
}

fn project_root_for_rsdl(rsdl: &Path) -> PathBuf {
    let rsdl_dir = rsdl.parent().unwrap_or_else(|| Path::new("."));
    if rsdl_dir.file_name() == Some(OsStr::new("rsdl")) {
        rsdl_dir.parent().unwrap_or(rsdl_dir).to_path_buf()
    } else {
        rsdl_dir.to_path_buf()
    }
}

fn resolve_external_package_root(
    project_root: &Path,
    external: &flowrt_ir::ExternalProcessIr,
) -> Result<PathBuf> {
    let mut roots = Vec::new();
    if let Some(paths) = env::var_os("FLOWRT_EXTERNAL_PATH") {
        for entry in env::split_paths(&paths) {
            push_external_search_entry(&mut roots, entry, &external.package);
        }
    }
    push_unique_external_path(
        &mut roots,
        PathBuf::from("/opt/flowrt/external").join(&external.package),
    );
    push_unique_external_path(
        &mut roots,
        project_root.join("external").join(&external.package),
    );

    let mut searched = Vec::new();
    for root in roots {
        let manifest_path = root.join("flowrt-external.toml");
        let executable_path = root.join(&external.executable);
        searched.push(root.clone());
        if !manifest_path.exists() || !executable_path.exists() {
            continue;
        }
        let manifest = load_external_manifest(&root)?;
        if manifest.package.name == external.package {
            return Ok(root);
        }
    }

    anyhow::bail!(
        "external package `{}` executable `{}` was not found for bundle; searched package roots: {}",
        external.package,
        external.executable,
        searched
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn select_external_executable_metadata<'a>(
    manifest: &'a ExternalPackageManifest,
    external: &flowrt_ir::ExternalProcessIr,
) -> Result<&'a ExternalExecutableMetadata> {
    manifest
        .executable
        .iter()
        .find(|executable| executable.path.as_path() == Path::new(&external.executable))
        .or_else(|| {
            manifest
                .executable
                .iter()
                .find(|executable| executable.name == external.executable)
        })
        .with_context(|| {
            format!(
                "external package `{}` manifest does not describe executable `{}`",
                external.package, external.executable
            )
        })
}

fn push_external_search_entry(roots: &mut Vec<PathBuf>, entry: PathBuf, package: &str) {
    push_unique_external_path(roots, entry.clone());
    push_unique_external_path(roots, entry.join(package));
}

fn push_unique_external_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn copy_required_file(source: &Path, dest: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("failed to inspect bundle file `{}`", source.display()))?;
    if metadata.file_type().is_symlink() {
        anyhow::bail!(
            "bundle source `{}` is a symbolic link; symlinks are not allowed",
            source.display()
        );
    }
    if !metadata.is_file() {
        anyhow::bail!("required bundle file `{}` does not exist", source.display());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    fs::copy(source, dest).with_context(|| {
        format!(
            "failed to copy `{}` to `{}`",
            source.display(),
            dest.display()
        )
    })?;
    Ok(())
}

fn file_sha256(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open `{}`", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = std::io::Read::read(&mut file, &mut buffer)
            .with_context(|| format!("failed to read `{}`", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

fn copy_dir_recursive(source: &Path, dest: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("failed to inspect bundle directory `{}`", source.display()))?;
    if metadata.file_type().is_symlink() {
        anyhow::bail!(
            "bundle source `{}` is a symbolic link; symlinks are not allowed",
            source.display()
        );
    }
    if !metadata.is_dir() {
        anyhow::bail!(
            "required bundle directory `{}` does not exist",
            source.display()
        );
    }
    fs::create_dir_all(dest).with_context(|| format!("failed to create `{}`", dest.display()))?;
    for entry in fs::read_dir(source)
        .with_context(|| format!("failed to read directory `{}`", source.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to read `{}` entry", source.display()))?;
        let path = entry.path();
        let target = dest.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect bundle source `{}`", path.display()))?;
        if file_type.is_symlink() {
            anyhow::bail!(
                "bundle source `{}` is a symbolic link; symlinks are not allowed",
                path.display()
            );
        } else if file_type.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else if file_type.is_file() {
            copy_required_file(&path, &target)?;
        }
    }
    Ok(())
}

fn bundle_target_name(contract: &ContractIr) -> String {
    contract
        .deployments
        .first()
        .map(|deployment| deployment.target.name.clone())
        .or_else(|| contract.targets.first().map(|target| target.name.clone()))
        .unwrap_or_else(|| "default".to_string())
}

fn bundle_target_platform(contract: &ContractIr) -> Option<String> {
    let target_name = bundle_target_name(contract);
    contract
        .targets
        .iter()
        .find(|target| target.name == target_name)
        .and_then(|target| {
            target
                .platform
                .map(|platform| platform.as_str().to_string())
        })
}

fn bundle_target_name_for_build(
    build_info: &build_model::BuildInfo,
    contract: &ContractIr,
) -> String {
    build_info
        .target
        .clone()
        .unwrap_or_else(|| bundle_target_name(contract))
}

fn bundle_target_platform_for_build(
    build_info: &build_model::BuildInfo,
    contract: &ContractIr,
) -> Result<Option<String>> {
    if build_info.platform.is_some() {
        return canonical_optional_platform(build_info.platform.as_deref());
    }
    let contract_platform = bundle_target_platform(contract);
    canonical_optional_platform(contract_platform.as_deref())
}

fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or_default()
}

fn deploy_bundle(
    bundle: &Path,
    host: &str,
    target: &str,
    remote_dir: &str,
    dry_run: bool,
) -> Result<String> {
    validate_deploy_host(host)?;
    validate_deploy_remote_dir(remote_dir)?;
    let loaded = load_bundle_manifest(bundle)?;
    let manifest = loaded.manifest;
    let selected_artifacts = select_deploy_artifacts(bundle, &manifest, target)?;
    let mut warnings = Vec::new();
    if let Some(version_warning) = loaded.version_warning {
        warnings.push(version_warning);
    }
    let warning = deploy_warning_suffix(&warnings);
    if dry_run {
        return Ok(format!(
            "deploy plan bundle={} host={} target={} remote_dir={} entry={}{}{}",
            bundle.display(),
            host,
            target,
            remote_dir,
            manifest.entry,
            deploy_artifact_suffix(&selected_artifacts),
            warning
        ));
    }

    let version_check = ProcessCommand::new("ssh")
        .arg("--")
        .arg(host)
        .arg("flowrt --version")
        .output()
        .with_context(|| format!("failed to spawn ssh for host `{host}`"))?;
    let remote_warning = validate_remote_flowrt_version_check(
        version_check.status.success(),
        &String::from_utf8_lossy(&version_check.stdout),
        &String::from_utf8_lossy(&version_check.stderr),
        &manifest.flowrt_version,
    )?;
    if let Some(remote_warning) = remote_warning {
        warnings.push(remote_warning);
    }
    let warning = deploy_warning_suffix(&warnings);

    let remote = format!("{host}:{remote_dir}");
    let upload = ProcessCommand::new("scp")
        .arg("-r")
        .arg("--")
        .arg(bundle)
        .arg(&remote)
        .status()
        .with_context(|| format!("failed to spawn scp for host `{host}`"))?;
    if !upload.success() {
        anyhow::bail!("bundle upload failed with status {upload}");
    }

    Ok(format!(
        "deployed FlowRT bundle {} to {}{}",
        bundle.display(),
        remote,
        warning
    ))
}

fn deploy_warning_suffix(warnings: &[String]) -> String {
    if warnings.is_empty() {
        String::new()
    } else {
        format!(" warning={}", warnings.join("; "))
    }
}

fn deploy_artifact_suffix(selection: &DeployArtifactSelection) -> String {
    if selection.count == 0 {
        String::new()
    } else {
        format!(
            " artifacts={} platforms=[{}]",
            selection.count,
            selection.platforms.join(",")
        )
    }
}

fn select_deploy_artifacts(
    bundle: &Path,
    manifest: &BundleManifest,
    target: &str,
) -> Result<DeployArtifactSelection> {
    ensure_safe_relative_path(Path::new(&manifest.entry))?;
    if manifest.schema_version < 2 || manifest.artifacts.is_empty() {
        if manifest.target != target {
            anyhow::bail!(
                "bundle target `{}` does not match requested target `{target}`",
                manifest.target
            );
        }
        return Ok(DeployArtifactSelection {
            count: 0,
            platforms: Vec::new(),
        });
    }

    let mut platforms = Vec::new();
    let mut count = 0usize;
    for artifact in manifest
        .artifacts
        .iter()
        .filter(|artifact| artifact.target == target)
    {
        validate_bundle_artifact(bundle, manifest, artifact)?;
        count += 1;
        if let Some(platform) = &artifact.platform {
            let canonical = TargetPlatform::parse_alias(platform)
                .with_context(|| {
                    format!(
                        "bundle artifact `{}` declares unsupported platform `{platform}`",
                        artifact.path.display()
                    )
                })?
                .as_str()
                .to_string();
            if !platforms.iter().any(|existing| existing == &canonical) {
                platforms.push(canonical);
            }
        }
    }
    if count == 0 {
        anyhow::bail!("bundle does not contain deployable artifacts for target `{target}`");
    }
    platforms.sort();
    Ok(DeployArtifactSelection { count, platforms })
}

fn validate_bundle_artifact(
    bundle: &Path,
    manifest: &BundleManifest,
    artifact: &BundleArtifact,
) -> Result<()> {
    ensure_safe_relative_path(&artifact.path)?;
    let platform = artifact.platform.as_deref().with_context(|| {
        format!(
            "bundle artifact `{}` is missing platform metadata",
            artifact.path.display()
        )
    })?;
    let canonical_platform = TargetPlatform::parse_alias(platform).with_context(|| {
        format!(
            "bundle artifact `{}` declares unsupported platform `{platform}`",
            artifact.path.display()
        )
    })?;
    let canonical_platform = canonical_platform.as_str().to_string();
    if manifest.target == artifact.target {
        if let Some(manifest_platform) = &manifest.platform {
            let manifest_platform = TargetPlatform::parse_alias(manifest_platform)
                .map(|value| value.as_str().to_string())
                .with_context(|| {
                    format!(
                        "bundle target `{}` declares unsupported platform `{manifest_platform}`",
                        manifest.target
                    )
                })?;
            if manifest_platform != canonical_platform {
                anyhow::bail!(
                    "bundle artifact `{}` platform mismatch: target `{}` expects `{}`, artifact declares `{}`; run `{}` before bundling again",
                    artifact.path.display(),
                    artifact.target,
                    manifest_platform,
                    canonical_platform,
                    build_launcher_hint(Some(&manifest_platform))
                );
            }
        }
    }
    validate_bundle_artifact_path_platform(artifact, &canonical_platform)?;
    let path = bundle.join(&artifact.path);
    let metadata = fs::symlink_metadata(&path).with_context(|| {
        format!(
            "bundle artifact `{}` does not exist; run `{}` before bundling again",
            path.display(),
            build_launcher_hint(Some(&canonical_platform))
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        anyhow::bail!(
            "bundle artifact `{}` must be a regular file",
            artifact.path.display()
        );
    }
    let actual_hash = file_sha256(&path)?;
    if actual_hash != artifact.sha256 {
        anyhow::bail!(
            "bundle artifact `{}` sha256 mismatch: manifest has {}, actual is {}; run `{}` before bundling again",
            artifact.path.display(),
            artifact.sha256,
            actual_hash,
            build_launcher_hint(Some(&canonical_platform))
        );
    }
    Ok(())
}

fn validate_bundle_artifact_path_platform(
    artifact: &BundleArtifact,
    canonical_platform: &str,
) -> Result<()> {
    let mut components = artifact.path.components();
    if !matches!(components.next(), Some(Component::Normal(value)) if value == "bin") {
        return Ok(());
    }
    let Some(Component::Normal(platform_component)) = components.next() else {
        return Ok(());
    };
    let platform_component = platform_component.to_string_lossy();
    let Some(path_platform) = TargetPlatform::parse_alias(&platform_component) else {
        return Ok(());
    };
    let path_platform = path_platform.as_str();
    if path_platform != canonical_platform {
        anyhow::bail!(
            "bundle artifact `{}` platform mismatch: path uses `{path_platform}`, artifact declares `{canonical_platform}`; run `{}` before bundling again",
            artifact.path.display(),
            build_launcher_hint(Some(path_platform))
        );
    }
    Ok(())
}

fn build_launcher_hint(platform: Option<&str>) -> String {
    match platform {
        Some(platform) => format!("flowrt build --target {platform} --launcher"),
        None => "flowrt build --launcher".to_string(),
    }
}

fn validate_remote_flowrt_version_check(
    success: bool,
    stdout: &str,
    stderr: &str,
    bundle_version: &str,
) -> Result<Option<String>> {
    if !success {
        let stderr = stderr.trim();
        if stderr.is_empty() {
            anyhow::bail!("remote FlowRT version check failed");
        }
        anyhow::bail!("remote FlowRT version check failed: {stderr}");
    }

    let remote_version = parse_flowrt_version_output(stdout)?;
    remote_version_warning(remote_version, bundle_version)
}

fn parse_flowrt_version_output(output: &str) -> Result<&str> {
    output
        .split_whitespace()
        .find(|token| parse_flowrt_release_version(token).is_ok())
        .context("remote `flowrt --version` output did not contain a MAJOR.MINOR.PATCH version")
}

fn remote_version_warning(remote_version: &str, bundle_version: &str) -> Result<Option<String>> {
    if remote_version == bundle_version {
        return Ok(None);
    }
    let remote = parse_flowrt_release_version(remote_version)
        .with_context(|| format!("invalid remote FlowRT version `{remote_version}`"))?;
    let bundle = parse_flowrt_release_version(bundle_version)
        .with_context(|| format!("invalid FlowRT bundle version `{bundle_version}`"))?;
    if remote.major == bundle.major && remote.minor == bundle.minor {
        return Ok(Some(format!(
            "remote patch version {remote_version} differs from bundle {bundle_version}; deploy is allowed within the same major.minor release line"
        )));
    }
    anyhow::bail!(
        "incompatible remote FlowRT version: remote has FlowRT {remote_version}, but bundle was created with FlowRT {bundle_version}"
    );
}

fn validate_deploy_host(host: &str) -> Result<()> {
    if host.is_empty() {
        anyhow::bail!("deploy host must not be empty");
    }
    if host.starts_with('-') {
        anyhow::bail!("deploy host `{host}` is invalid: host must not start with `-`");
    }
    Ok(())
}

fn validate_deploy_remote_dir(remote_dir: &str) -> Result<()> {
    if remote_dir.trim().is_empty() {
        anyhow::bail!("deploy remote_dir must not be empty");
    }
    if !remote_dir.starts_with('/') {
        anyhow::bail!("deploy remote_dir `{remote_dir}` is invalid: path must be absolute");
    }
    if remote_dir
        .split('/')
        .any(|segment| segment == ".." || segment == ".")
    {
        anyhow::bail!(
            "deploy remote_dir `{remote_dir}` is invalid: `.` and `..` path segments are not allowed"
        );
    }
    if !remote_dir.bytes().all(|byte| {
        byte == b'/' || byte == b'.' || byte == b'_' || byte == b'-' || byte.is_ascii_alphanumeric()
    }) {
        anyhow::bail!(
            "deploy remote_dir `{remote_dir}` is invalid: only POSIX-safe characters [A-Za-z0-9._/-] are allowed"
        );
    }
    Ok(())
}

fn load_bundle_manifest(bundle: &Path) -> Result<LoadedBundleManifest> {
    let path = bundle.join("bundle.toml");
    let source = fs::read_to_string(&path)
        .with_context(|| format!("failed to read bundle manifest `{}`", path.display()))?;
    let manifest: BundleManifest = toml::from_str(&source)
        .with_context(|| format!("failed to parse bundle manifest `{}`", path.display()))?;
    if !matches!(manifest.schema_version, 1 | 2) {
        anyhow::bail!(
            "unsupported FlowRT bundle schema version {} in `{}`",
            manifest.schema_version,
            path.display()
        );
    }
    let version_warning =
        bundle_version_warning(&manifest.flowrt_version, env!("CARGO_PKG_VERSION"))?;
    Ok(LoadedBundleManifest {
        manifest,
        version_warning,
    })
}

fn bundle_version_warning(bundle_version: &str, cli_version: &str) -> Result<Option<String>> {
    if bundle_version == cli_version {
        return Ok(None);
    }
    let bundle = parse_flowrt_release_version(bundle_version)
        .with_context(|| format!("invalid FlowRT bundle version `{bundle_version}`"))?;
    let cli = parse_flowrt_release_version(cli_version)
        .with_context(|| format!("invalid FlowRT CLI version `{cli_version}`"))?;
    if bundle.major == cli.major && bundle.minor == cli.minor {
        return Ok(Some(format!(
            "bundle patch version {bundle_version} differs from CLI {cli_version}; deploy is allowed within the same major.minor release line"
        )));
    }
    anyhow::bail!(
        "incompatible FlowRT version: bundle was created with FlowRT {bundle_version}, but this CLI is {cli_version}"
    );
}

fn parse_flowrt_release_version(version: &str) -> Result<FlowrtReleaseVersion> {
    let mut parts = version.split('.');
    let major = parse_release_version_part(parts.next(), "major")?;
    let minor = parse_release_version_part(parts.next(), "minor")?;
    let patch = parse_release_version_part(parts.next(), "patch")?;
    if parts.next().is_some() {
        anyhow::bail!("expected MAJOR.MINOR.PATCH");
    }
    Ok(FlowrtReleaseVersion {
        major,
        minor,
        patch,
    })
}

fn parse_release_version_part(part: Option<&str>, name: &str) -> Result<u64> {
    let part = part.with_context(|| format!("missing {name} version part"))?;
    if part.is_empty() || !part.chars().all(|ch| ch.is_ascii_digit()) {
        anyhow::bail!("{name} version part `{part}` is not a non-negative integer");
    }
    part.parse::<u64>()
        .with_context(|| format!("failed to parse {name} version part `{part}`"))
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
    normalize_loaded_document(&loaded, hash_source(&source_bundle))
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
    if has_component_language(contract, LanguageKind::Cpp) || has_ros2_bridge(contract) {
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

fn has_ros2_bridge(contract: &ContractIr) -> bool {
    contract
        .graphs
        .iter()
        .any(|graph| !graph.ros2_bridges.is_empty())
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

    if let Some(boundary) = first_mixed_language_bind_with_unsupported_backend(contract) {
        anyhow::bail!(
            "mixed-language `{command}` cannot carry dataflow `{}` -> `{}` over backend `{}`; use backend `iox2` or `zenoh` for cross-language process boundaries",
            boundary.from,
            boundary.to,
            boundary.backend
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
            LanguageKind::External => {}
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

#[derive(Debug, Clone)]
struct MixedLanguageBind {
    from: String,
    to: String,
    backend: String,
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

fn first_mixed_language_bind_with_unsupported_backend(
    contract: &ContractIr,
) -> Option<MixedLanguageBind> {
    let component_languages = contract
        .components
        .iter()
        .map(|component| (component.name.as_str(), component.language))
        .collect::<BTreeMap<_, _>>();

    for graph in &contract.graphs {
        let instance_languages = graph
            .instances
            .iter()
            .filter_map(|instance| {
                component_languages
                    .get(instance.component.name.as_str())
                    .copied()
                    .map(|language| (instance.name.as_str(), language))
            })
            .collect::<BTreeMap<_, _>>();

        for bind in &graph.binds {
            let Some(from_language) = instance_languages
                .get(bind.from.instance.name.as_str())
                .copied()
            else {
                continue;
            };
            let Some(to_language) = instance_languages
                .get(bind.to.instance.name.as_str())
                .copied()
            else {
                continue;
            };
            if from_language == to_language || matches!(bind.backend.0.as_str(), "iox2" | "zenoh") {
                continue;
            }
            return Some(MixedLanguageBind {
                from: format!("{}.{}", bind.from.instance.name, bind.from.port),
                to: format!("{}.{}", bind.to.instance.name, bind.to.port),
                backend: bind.backend.0.clone(),
            });
        }
    }

    None
}

fn ensure_backend_runtime_supported(_contract: &ContractIr, _command: &str) -> Result<()> {
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildToolchainProfile {
    profile: ToolchainProfile,
    cargo_target_triple: Option<String>,
}

fn resolve_build_toolchain_profile(
    contract: &ContractIr,
    explicit_target: Option<&str>,
    workspace_root: &Path,
) -> Result<Option<BuildToolchainProfile>> {
    let platform = explicit_target
        .map(str::to_string)
        .or_else(|| contract_target_platform(contract));
    resolve_optional_toolchain_profile(
        platform.as_deref(),
        explicit_target.is_some(),
        workspace_root,
    )
}

fn resolve_deps_toolchain_profile(
    rsdl: Option<&Path>,
    profile: Option<&str>,
    explicit_target: Option<&str>,
) -> Result<Option<BuildToolchainProfile>> {
    let workspace_root = match rsdl {
        Some(rsdl) => application_root_from_rsdl(rsdl)?,
        None => env::current_dir().context("failed to resolve current working directory")?,
    };
    if let Some(explicit_target) = explicit_target {
        return resolve_optional_toolchain_profile(Some(explicit_target), true, &workspace_root);
    }
    let Some(rsdl) = rsdl else {
        return Ok(None);
    };
    let contract = normalize_contract_from_rsdl(rsdl)?;
    let projected = project_contract_to_profile(&contract, profile)
        .with_context(|| format!("failed to select profile for `{}`", rsdl.display()))?;
    validate_contract(&projected).context("contract validation failed")?;
    resolve_optional_toolchain_profile(
        contract_target_platform(&projected).as_deref(),
        false,
        &workspace_root,
    )
}

fn resolve_optional_toolchain_profile(
    platform: Option<&str>,
    explicit_target: bool,
    workspace_root: &Path,
) -> Result<Option<BuildToolchainProfile>> {
    let Some(platform) = platform else {
        return Ok(None);
    };
    let platform = canonical_toolchain_platform(platform)?;
    let profile = resolve_toolchain_profile(
        &platform,
        workspace_root,
        &ToolchainProfileOverrides::default(),
    )?;
    let (_, host_target) = rustc_toolchain_identity()?;
    let cargo_target_triple = if explicit_target || profile.rust_target != host_target {
        Some(profile.rust_target.clone())
    } else {
        None
    };
    Ok(Some(BuildToolchainProfile {
        profile,
        cargo_target_triple,
    }))
}

fn canonical_toolchain_platform(platform: &str) -> Result<String> {
    TargetPlatform::parse_alias(platform)
        .map(|platform| platform.as_str().to_string())
        .with_context(|| format!("unsupported toolchain platform `{platform}`"))
}

fn contract_target_platform(contract: &ContractIr) -> Option<String> {
    bundle_target_platform(contract)
}

fn cargo_target_args(target_triple: Option<&str>) -> Vec<String> {
    target_triple
        .map(|target| vec!["--target".to_string(), target.to_string()])
        .unwrap_or_default()
}

fn deps_runtime_features(
    rsdl: Option<&Path>,
    profile: Option<&str>,
    backend: Option<DepsBackend>,
) -> Result<RuntimeFeatureSet> {
    if let Some(backend) = backend {
        return match backend {
            DepsBackend::Inproc => Ok(RuntimeFeatureSet::inproc_only()),
            DepsBackend::Iox2 => RuntimeFeatureSet::from_backend_names(["iox2"]),
            DepsBackend::Zenoh => RuntimeFeatureSet::from_backend_names(["zenoh"]),
            DepsBackend::All => Ok(RuntimeFeatureSet::all()),
        };
    }
    let Some(rsdl) = rsdl else {
        return Ok(RuntimeFeatureSet::all());
    };
    let contract = normalize_contract_from_rsdl(rsdl)?;
    let projected = project_contract_to_profile(&contract, profile)
        .with_context(|| format!("failed to select profile for `{}`", rsdl.display()))?;
    validate_contract(&projected).context("contract validation failed")?;
    RuntimeFeatureSet::from_contract(&projected)
}

fn deps_cache_layout(
    build_mode: BuildMode,
    features: RuntimeFeatureSet,
    target_profile: Option<&BuildToolchainProfile>,
) -> Result<CacheLayout> {
    let root = default_cache_root()
        .context("failed to resolve FlowRT cache directory; set FLOWRT_CACHE_DIR or HOME")?;
    let (rustc_identity, host_target_triple) = rustc_toolchain_identity()?;
    let target_triple = target_profile
        .map(|profile| profile.profile.rust_target.clone())
        .unwrap_or(host_target_triple);
    let rust_runtime_dir = rust_runtime_dir_for_generated_build()?;
    let vendor_hash = flowrt_vendor_hash(rust_runtime_dir.as_deref())?;
    let key = DepsCacheKey::new(
        env!("CARGO_PKG_VERSION"),
        rustc_identity,
        target_triple,
        vendor_hash,
        build_mode,
        features,
    );
    Ok(CacheLayout::new(root, &key))
}

fn prepare_deps_cache(
    layout: &CacheLayout,
    build_mode: BuildMode,
    features: &RuntimeFeatureSet,
    target_profile: Option<&BuildToolchainProfile>,
) -> Result<()> {
    let _lock = CacheLock::acquire(&layout.lock_file)?;
    if deps_ready(layout, build_mode, features)? {
        return Ok(());
    }
    let rust_runtime_dir = rust_runtime_dir_for_generated_build()?.context(
        "FlowRT Rust runtime directory not found; install FlowRT package, set FLOWRT_RUST_RUNTIME_DIR, or set FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=1 in repository development mode",
    )?;
    if is_repo_rust_runtime_dir(&rust_runtime_dir)? {
        run_repo_runtime_cargo_build(
            &layout.target_dir,
            build_mode,
            features,
            target_profile.and_then(|profile| profile.cargo_target_triple.as_deref()),
        )?;
    } else {
        write_deps_workspace(&layout.deps_workspace_dir, &rust_runtime_dir, features)?;
        run_deps_cargo_build(
            &layout.deps_workspace_dir,
            &layout.target_dir,
            build_mode,
            target_profile.and_then(|profile| profile.cargo_target_triple.as_deref()),
        )?;
    }
    write_deps_ready_marker(layout, build_mode, features)
}

fn ensure_deps_ready(
    layout: &CacheLayout,
    build_mode: BuildMode,
    features: &RuntimeFeatureSet,
    target_profile: Option<&BuildToolchainProfile>,
) -> Result<()> {
    if deps_ready(layout, build_mode, features)? {
        return Ok(());
    }
    let target_hint = target_profile
        .map(|profile| {
            format!(
                " for platform `{}` / Rust target `{}`",
                profile.profile.platform, profile.profile.rust_target
            )
        })
        .unwrap_or_else(|| format!(" for native Rust target `{}`", layout.target_triple));
    anyhow::bail!(
        "FlowRT dependency cache is missing{target_hint} for build_mode `{}` and backend features {:?}; run `flowrt deps --backend {} --build-mode {}{}` or `flowrt deps <rsdl> --build-mode {}{}` first",
        build_mode,
        features.canonical_names(),
        features.deps_backend_hint(),
        build_mode,
        target_profile
            .map(|profile| format!(" --target {}", profile.profile.platform))
            .unwrap_or_default(),
        build_mode,
        target_profile
            .map(|profile| format!(" --target {}", profile.profile.platform))
            .unwrap_or_default()
    )
}

fn select_ready_deps_cache_layout(
    build_mode: BuildMode,
    features: &RuntimeFeatureSet,
    target_profile: Option<&BuildToolchainProfile>,
) -> Result<CacheLayout> {
    let exact = deps_cache_layout(build_mode, features.clone(), target_profile)?;
    if deps_ready(&exact, build_mode, features)? {
        return Ok(exact);
    }

    let all_features = RuntimeFeatureSet::all();
    if features != &all_features && features.is_subset_of(&all_features) {
        let all = deps_cache_layout(build_mode, all_features.clone(), target_profile)?;
        if deps_ready(&all, build_mode, &all_features)? {
            return Ok(all);
        }
    }

    ensure_deps_ready(&exact, build_mode, features, target_profile)?;
    unreachable!("ensure_deps_ready must return an error when cache is absent")
}

fn deps_ready(
    layout: &CacheLayout,
    build_mode: BuildMode,
    features: &RuntimeFeatureSet,
) -> Result<bool> {
    if !layout.ready_file.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(&layout.ready_file)
        .with_context(|| format!("failed to read `{}`", layout.ready_file.display()))?;
    let marker: DepsReadyMarker = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse `{}`", layout.ready_file.display()))?;
    Ok(marker.schema_version == 1
        && marker.flowrt_version == env!("CARGO_PKG_VERSION")
        && marker.build_mode == build_mode
        && marker.features == feature_names_owned(features)
        && marker.target_triple.as_deref() == Some(layout.target_triple.as_str())
        && marker.target_dir == layout.target_dir)
}

#[derive(Debug, Serialize, Deserialize)]
struct DepsReadyMarker {
    schema_version: u32,
    flowrt_version: String,
    build_mode: BuildMode,
    features: Vec<String>,
    #[serde(default)]
    target_triple: Option<String>,
    target_dir: PathBuf,
}

fn write_deps_ready_marker(
    layout: &CacheLayout,
    build_mode: BuildMode,
    features: &RuntimeFeatureSet,
) -> Result<()> {
    let marker = DepsReadyMarker {
        schema_version: 1,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        build_mode,
        features: feature_names_owned(features),
        target_triple: Some(layout.target_triple.clone()),
        target_dir: layout.target_dir.clone(),
    };
    if let Some(parent) = layout.ready_file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    let mut content = serde_json::to_string_pretty(&marker)?;
    content.push('\n');
    fs::write(&layout.ready_file, content)
        .with_context(|| format!("failed to write `{}`", layout.ready_file.display()))
}

fn feature_names_owned(features: &RuntimeFeatureSet) -> Vec<String> {
    features
        .canonical_names()
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn write_deps_workspace(
    workspace_dir: &Path,
    rust_runtime_dir: &Path,
    features: &RuntimeFeatureSet,
) -> Result<()> {
    fs::create_dir_all(workspace_dir.join("src"))
        .with_context(|| format!("failed to create `{}`", workspace_dir.display()))?;
    let feature_args = features.cargo_feature_args();
    let feature_suffix = if feature_args.is_empty() {
        String::new()
    } else {
        format!(
            ", features = [{}]",
            feature_args
                .iter()
                .map(|feature| format!("\"{feature}\""))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let manifest = format!(
        "[package]\nname = \"flowrt-deps-prewarm\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[dependencies]\nflowrt = {{ path = {}{} }}\nserde = {{ version = \"1\", features = [\"derive\"] }}\nserde_json = \"1\"\n",
        toml_basic_string(rust_runtime_dir),
        feature_suffix
    );
    fs::write(workspace_dir.join("Cargo.toml"), manifest).with_context(|| {
        format!(
            "failed to write `{}`",
            workspace_dir.join("Cargo.toml").display()
        )
    })?;
    fs::write(
        workspace_dir.join("src").join("lib.rs"),
        "pub fn flowrt_deps_prewarm_marker() -> flowrt::Status {\n    flowrt::Status::Ok\n}\n",
    )
    .with_context(|| {
        format!(
            "failed to write `{}`",
            workspace_dir.join("src/lib.rs").display()
        )
    })?;

    if let Some(private_prefix) = flowrt_private_prefix_from_runtime_dir(rust_runtime_dir) {
        let vendor_dir = private_prefix.join("share").join("cargo").join("vendor");
        if vendor_dir.is_dir() {
            let cargo_dir = workspace_dir.join(".cargo");
            fs::create_dir_all(&cargo_dir)
                .with_context(|| format!("failed to create `{}`", cargo_dir.display()))?;
            let config = format!(
                "[source.crates-io]\nreplace-with = \"flowrt-vendor\"\n\n[source.flowrt-vendor]\ndirectory = {}\n\n[net]\noffline = true\n",
                toml_basic_string(&vendor_dir)
            );
            fs::write(cargo_dir.join("config.toml"), config).with_context(|| {
                format!(
                    "failed to write `{}`",
                    cargo_dir.join("config.toml").display()
                )
            })?;
        }
    }
    Ok(())
}

fn run_deps_cargo_build(
    workspace_dir: &Path,
    target_dir: &Path,
    build_mode: BuildMode,
    target_triple: Option<&str>,
) -> Result<()> {
    ensure_rust_target_available(target_triple)?;
    fs::create_dir_all(target_dir)
        .with_context(|| format!("failed to create `{}`", target_dir.display()))?;
    let mut command = ProcessCommand::new("cargo");
    command
        .current_dir(workspace_dir)
        .arg("build")
        .arg("--lib")
        .env("CARGO_TARGET_DIR", target_dir);
    for arg in build_mode.cargo_args() {
        command.arg(arg);
    }
    for arg in cargo_target_args(target_triple) {
        command.arg(arg);
    }
    if workspace_dir.join(".cargo").join("config.toml").exists() {
        command.arg("--offline");
    }
    let status = command.status().with_context(|| {
        format!(
            "failed to spawn cargo for dependency prewarm in `{}`",
            workspace_dir.display()
        )
    })?;
    if !status.success() {
        bail_cargo_status("FlowRT dependency prewarm", status, target_triple)?;
    }
    Ok(())
}

fn run_repo_runtime_cargo_build(
    target_dir: &Path,
    build_mode: BuildMode,
    features: &RuntimeFeatureSet,
    target_triple: Option<&str>,
) -> Result<()> {
    ensure_rust_target_available(target_triple)?;
    let repo_root = repo_root_dir()?;
    fs::create_dir_all(target_dir)
        .with_context(|| format!("failed to create `{}`", target_dir.display()))?;
    let mut command = ProcessCommand::new("cargo");
    command
        .current_dir(&repo_root)
        .arg("build")
        .arg("-p")
        .arg("flowrt")
        .arg("--lib")
        .arg("--locked")
        .env("CARGO_TARGET_DIR", target_dir);
    for arg in build_mode.cargo_args() {
        command.arg(arg);
    }
    for arg in cargo_target_args(target_triple) {
        command.arg(arg);
    }
    let feature_args = features.cargo_feature_args();
    if !feature_args.is_empty() {
        command.arg("--features").arg(feature_args.join(","));
    }
    let status = command.status().with_context(|| {
        format!(
            "failed to spawn cargo for repository dependency prewarm in `{}`",
            repo_root.display()
        )
    })?;
    if !status.success() {
        bail_cargo_status(
            "FlowRT repository dependency prewarm",
            status,
            target_triple,
        )?;
    }
    Ok(())
}

fn ensure_rust_target_available(target_triple: Option<&str>) -> Result<()> {
    let Some(target_triple) = target_triple else {
        return Ok(());
    };
    let output = match ProcessCommand::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).context("failed to run `rustup target list --installed`");
        }
    };
    if !output.status.success() {
        return Ok(());
    }
    let installed = String::from_utf8_lossy(&output.stdout);
    if installed.lines().any(|line| line.trim() == target_triple) {
        return Ok(());
    }
    anyhow::bail!(
        "Rust target `{target_triple}` is not installed; run `rustup target add {target_triple}` or configure the Rust toolchain before running FlowRT cross build"
    );
}

fn bail_cargo_status(
    context: &str,
    status: std::process::ExitStatus,
    target_triple: Option<&str>,
) -> Result<()> {
    if let Some(target_triple) = target_triple {
        anyhow::bail!(
            "{context} failed with status {status} for Rust target `{target_triple}`; run `rustup target add {target_triple}` if the target std library is missing, then retry"
        );
    }
    anyhow::bail!("{context} failed with status {status}");
}

fn is_repo_rust_runtime_dir(path: &Path) -> Result<bool> {
    let Some(repo_runtime) = repo_runtime_dir("runtime/rust", "Cargo.toml") else {
        return Ok(false);
    };
    let repo_runtime = fs::canonicalize(repo_runtime)
        .context("failed to canonicalize repository Rust runtime directory")?;
    let candidate = fs::canonicalize(path)
        .with_context(|| format!("failed to canonicalize `{}`", path.display()))?;
    Ok(candidate == repo_runtime)
}

#[derive(Debug)]
struct CacheLock {
    file: File,
}

impl CacheLock {
    fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create `{}`", parent.display()))?;
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("failed to open cache lock `{}`", path.display()))?;
        if !try_lock_file(&file)? {
            anyhow::bail!(
                "FlowRT dependency cache `{}` is already in use by another flowrt command",
                path.display()
            );
        }
        Ok(Self { file })
    }
}

impl Drop for CacheLock {
    fn drop(&mut self) {
        let _ = unlock_file(&self.file);
    }
}

fn rustc_toolchain_identity() -> Result<(String, String)> {
    let output = ProcessCommand::new("rustc")
        .arg("-Vv")
        .output()
        .context("failed to spawn rustc -Vv")?;
    if !output.status.success() {
        anyhow::bail!("rustc -Vv failed with status {}", output.status);
    }
    let stdout = String::from_utf8(output.stdout).context("rustc -Vv output is not UTF-8")?;
    let identity = stdout
        .lines()
        .find(|line| line.starts_with("rustc "))
        .unwrap_or("rustc unknown")
        .to_string();
    let host = stdout
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap_or(std::env::consts::ARCH)
        .to_string();
    Ok((identity, host))
}

fn flowrt_vendor_hash(rust_runtime_dir: Option<&Path>) -> Result<String> {
    if let Some(runtime_dir) = rust_runtime_dir {
        if let Some(private_prefix) = flowrt_private_prefix_from_runtime_dir(runtime_dir) {
            let hash_file = private_prefix
                .join("share")
                .join("cargo")
                .join("vendor")
                .join(".flowrt-vendor.sha256");
            if hash_file.exists() {
                let content = fs::read_to_string(&hash_file)
                    .with_context(|| format!("failed to read `{}`", hash_file.display()))?;
                if let Some(hash) = content.split_whitespace().next() {
                    return Ok(hash.to_string());
                }
                anyhow::bail!(
                    "FlowRT vendor hash marker `{}` is empty; reinstall the FlowRT package",
                    hash_file.display()
                );
            }
            anyhow::bail!(
                "FlowRT vendor hash marker is missing at `{}`; reinstall the FlowRT package",
                hash_file.display()
            );
        }
    }
    let repo_root = repo_root_dir()?;
    let mut hasher = Sha256::new();
    for relative in ["Cargo.lock", "runtime/rust/Cargo.toml", "scripts/deps.lock"] {
        let path = repo_root.join(relative);
        if path.exists() {
            hasher.update(relative.as_bytes());
            hasher.update(fs::read(&path).with_context(|| {
                format!("failed to read `{}` for FlowRT vendor hash", path.display())
            })?);
        }
    }
    Ok(hex_lower(&hasher.finalize())[..16].to_string())
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn build_workspace(
    contract: &ContractIr,
    out_dir: &Path,
    include_launcher: bool,
    build_mode: BuildMode,
    target_profile: Option<&BuildToolchainProfile>,
) -> Result<()> {
    ensure_backend_runtime_supported(contract, "build")?;
    let rust_runtime_dir = rust_runtime_dir_for_generated_build()?;
    let mut build_info = build_info_for_contract(contract, build_mode)?;
    apply_build_target_metadata(&mut build_info, target_profile)?;
    let bin_target_identity = bin_target_identity(target_profile);
    let cargo_cache = if build_steps(contract, include_launcher)
        .iter()
        .any(|step| matches!(step, BuildStep::CargoApp | BuildStep::CargoSupervisor))
    {
        let features = RuntimeFeatureSet::from_contract(contract)?;
        let layout = select_ready_deps_cache_layout(build_mode, &features, target_profile)?;
        build_info.deps_target_dir = Some(layout.target_dir.clone());
        Some(layout)
    } else {
        None
    };
    let cargo_target_triple =
        target_profile.and_then(|profile| profile.cargo_target_triple.as_deref());
    for step in build_steps(contract, include_launcher) {
        match step {
            BuildStep::CargoApp => {
                let manifest =
                    cargo_manifest_with_runtime_patch(out_dir, rust_runtime_dir.as_deref())?;
                let target_dir = cargo_cache
                    .as_ref()
                    .map(|layout| layout.target_dir.as_path())
                    .context("internal error: Cargo app build missing dependency cache layout")?;
                let built = run_cargo_build_bin(
                    &manifest,
                    &app_bin_name(contract),
                    build_mode,
                    target_dir,
                    cargo_target_triple,
                )?;
                let local = copy_executable_to_local_bin(
                    out_dir,
                    build_mode,
                    bin_target_identity.as_deref(),
                    &built,
                )?;
                build_info.executables.rust_app = Some(relative_to_out_dir(out_dir, &local)?);
                record_build_artifact(&mut build_info, "rust_app", out_dir, &local)?;
            }
            BuildStep::CargoSupervisor => {
                let manifest =
                    cargo_manifest_with_runtime_patch(out_dir, rust_runtime_dir.as_deref())?;
                let target_dir = cargo_cache
                    .as_ref()
                    .map(|layout| layout.target_dir.as_path())
                    .context(
                        "internal error: Cargo supervisor build missing dependency cache layout",
                    )?;
                let built = run_cargo_build_bin(
                    &manifest,
                    &supervisor_bin_name(contract),
                    build_mode,
                    target_dir,
                    cargo_target_triple,
                )?;
                let local = copy_executable_to_local_bin(
                    out_dir,
                    build_mode,
                    bin_target_identity.as_deref(),
                    &built,
                )?;
                build_info.executables.supervisor = Some(relative_to_out_dir(out_dir, &local)?);
                record_build_artifact(&mut build_info, "supervisor", out_dir, &local)?;
            }
            BuildStep::CmakeApp => {
                let built =
                    run_cmake_configure_and_build(contract, out_dir, build_mode, target_profile)?;
                if let Some(cpp_app) = built.cpp_app {
                    let local = copy_executable_to_local_bin(
                        out_dir,
                        build_mode,
                        bin_target_identity.as_deref(),
                        &cpp_app,
                    )?;
                    build_info.executables.cpp_app = Some(relative_to_out_dir(out_dir, &local)?);
                    record_build_artifact(&mut build_info, "cpp_app", out_dir, &local)?;
                }
                if let Some(ros2_bridge) = built.ros2_bridge {
                    let local = copy_executable_to_local_bin(
                        out_dir,
                        build_mode,
                        bin_target_identity.as_deref(),
                        &ros2_bridge,
                    )?;
                    build_info.executables.ros2_bridge =
                        Some(relative_to_out_dir(out_dir, &local)?);
                    record_build_artifact(&mut build_info, "ros2_bridge", out_dir, &local)?;
                }
            }
        }
    }
    build_info.write(out_dir)?;
    Ok(())
}

fn run_workspace(
    contract: &ContractIr,
    out_dir: &Path,
    process: Option<&str>,
    run_ticks: Option<usize>,
    requested_build_mode: Option<BuildMode>,
) -> Result<()> {
    ensure_direct_runtime_supported(contract, "run")?;
    ensure_backend_runtime_supported(contract, "run")?;
    ensure_run_process_boundaries_supported(contract, process)?;
    let build_info = load_build_info(out_dir, requested_build_mode, false)?;
    match run_mode_for_process(contract, process)
        .context("contract does not contain runnable components")?
    {
        RunMode::CargoApp => {
            let bin = executable_from_build_info(
                out_dir,
                build_info.executables.rust_app.as_ref(),
                "Rust app",
                "flowrt build",
            )?;
            if !bin.exists() {
                anyhow::bail!(
                    "app binary `{}` not found; run `flowrt build` first",
                    bin.display()
                );
            }
            run_binary(&bin, process, run_ticks)?;
        }
        RunMode::CmakeApp => {
            let bin = executable_from_build_info(
                out_dir,
                build_info.executables.cpp_app.as_ref(),
                "C++ app",
                "flowrt build",
            )?;
            run_cmake_app(&bin, process, run_ticks)?;
        }
    }
    Ok(())
}

fn launch_workspace(
    contract: &ContractIr,
    out_dir: &Path,
    run_ticks: Option<usize>,
    requested_build_mode: Option<BuildMode>,
) -> Result<()> {
    ensure_direct_runtime_supported(contract, "launch")?;
    ensure_backend_runtime_supported(contract, "launch")?;
    ensure_launch_process_boundaries_supported(contract)?;
    let build_info = load_build_info(out_dir, requested_build_mode, true)?;
    let supervisor = executable_from_build_info(
        out_dir,
        build_info.executables.supervisor.as_ref(),
        "FlowRT supervisor",
        "flowrt build --launcher",
    )?;
    if !supervisor.exists() {
        anyhow::bail!(
            "FlowRT supervisor `{}` not found; run `flowrt build --launcher` first",
            supervisor.display()
        );
    }
    run_supervisor_binary(&supervisor, run_ticks)?;
    Ok(())
}

fn build_info_for_contract(
    contract: &ContractIr,
    build_mode: BuildMode,
) -> Result<build_model::BuildInfo> {
    let mut info = build_model::BuildInfo::new(
        env!("CARGO_PKG_VERSION"),
        selected_prepared_profile_name(contract).map(str::to_string),
        build_mode,
        None,
    );
    info.target = Some(bundle_target_name(contract));
    info.platform = bundle_target_platform(contract);
    Ok(info)
}

fn apply_build_target_metadata(
    build_info: &mut build_model::BuildInfo,
    target_profile: Option<&BuildToolchainProfile>,
) -> Result<()> {
    let (_, host_target_triple) = rustc_toolchain_identity()?;
    build_info.host_target_triple = Some(host_target_triple);
    build_info.target_identity = Some(build_target_identity(target_profile));
    if let Some(target_profile) = target_profile {
        build_info.platform = Some(target_profile.profile.platform.clone());
        build_info.rust_target_triple = Some(target_profile.profile.rust_target.clone());
    }
    Ok(())
}

fn record_build_artifact(
    build_info: &mut build_model::BuildInfo,
    kind: &str,
    out_dir: &Path,
    local: &Path,
) -> Result<()> {
    let relative = relative_to_out_dir(out_dir, local)?;
    build_info.artifacts.push(build_model::BuildArtifactInfo {
        kind: kind.to_string(),
        target: build_info
            .target
            .clone()
            .unwrap_or_else(|| "default".to_string()),
        platform: build_info.platform.clone(),
        path: relative,
        sha256: file_sha256(local)?,
    });
    Ok(())
}

fn load_build_info(
    out_dir: &Path,
    requested_build_mode: Option<BuildMode>,
    launcher: bool,
) -> Result<build_model::BuildInfo> {
    let info = build_model::BuildInfo::read(out_dir).with_context(|| {
        format!(
            "FlowRT build metadata is missing; run `{}` with FlowRT 0.6.1 or newer",
            if launcher {
                "flowrt build --launcher"
            } else {
                "flowrt build"
            }
        )
    })?;
    if info.flowrt_version != env!("CARGO_PKG_VERSION") {
        anyhow::bail!(
            "prepared FlowRT artifacts were built with FlowRT {}, but this CLI is {}; run `{}` again",
            info.flowrt_version,
            env!("CARGO_PKG_VERSION"),
            if launcher {
                "flowrt build --launcher"
            } else {
                "flowrt build"
            }
        );
    }
    if let Some(requested) = requested_build_mode {
        if info.build_mode != requested {
            anyhow::bail!(
                "prepared FlowRT artifacts use build mode `{}`, but command requested `{}`; run `{}` with `--build-mode {}` first",
                info.build_mode,
                requested,
                if launcher {
                    "flowrt build --launcher"
                } else {
                    "flowrt build"
                },
                requested
            );
        }
    }
    Ok(info)
}

fn executable_from_build_info(
    out_dir: &Path,
    relative: Option<&PathBuf>,
    label: &str,
    build_hint: &str,
) -> Result<PathBuf> {
    let relative =
        relative.with_context(|| format!("{label} was not built; run `{build_hint}` first"))?;
    ensure_safe_relative_path(relative)?;
    Ok(out_dir.join(relative))
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
            if bind.backend.0 != "inproc" {
                continue;
            }
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
    write_cargo_vendor_config(out_dir, runtime_dir)?;
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

fn write_cargo_vendor_config(out_dir: &Path, runtime_dir: &Path) -> Result<()> {
    let Some(private_prefix) = flowrt_private_prefix_from_runtime_dir(runtime_dir) else {
        return Ok(());
    };
    let vendor_dir = private_prefix.join("share").join("cargo").join("vendor");
    if !vendor_dir.is_dir() {
        return Ok(());
    }
    let cargo_dir = out_dir.join("build").join(".cargo");
    fs::create_dir_all(&cargo_dir)
        .with_context(|| format!("failed to create `{}`", cargo_dir.display()))?;
    let config = format!(
        "[source.crates-io]\nreplace-with = \"flowrt-vendor\"\n\n[source.flowrt-vendor]\ndirectory = {}\n\n[net]\noffline = true\n",
        toml_basic_string(&vendor_dir)
    );
    let config_path = cargo_dir.join("config.toml");
    fs::write(&config_path, config)
        .with_context(|| format!("failed to write `{}`", config_path.display()))?;
    Ok(())
}

fn flowrt_private_prefix_from_runtime_dir(runtime_dir: &Path) -> Option<PathBuf> {
    let share_flowrt = runtime_dir.parent()?.parent()?;
    if share_flowrt.file_name()? != OsStr::new("flowrt") {
        return None;
    }
    let share = share_flowrt.parent()?;
    if share.file_name()? != OsStr::new("share") {
        return None;
    }
    Some(share.parent()?.to_path_buf())
}

#[derive(Debug, Default)]
struct CmakeBuildOutputs {
    cpp_app: Option<PathBuf>,
    ros2_bridge: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CppTargetSdk {
    root: PathBuf,
    cmake_dir: Option<PathBuf>,
    pkgconfig_dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct CppTargetSdkManifest {
    platform: String,
    complete: bool,
    cmake_dir: Option<PathBuf>,
    pkgconfig_dir: Option<PathBuf>,
    include_dir: Option<PathBuf>,
    lib_dir: Option<PathBuf>,
}

fn run_cmake_configure_and_build(
    contract: &ContractIr,
    out_dir: &Path,
    build_mode: BuildMode,
    target_profile: Option<&BuildToolchainProfile>,
) -> Result<CmakeBuildOutputs> {
    let toolchain_profile = target_profile
        .filter(|profile| profile.cargo_target_triple.is_some())
        .map(|profile| &profile.profile);
    let source_dir = out_dir.join("build");
    let build_dir = source_dir
        .join("cmake")
        .join(build_mode.cargo_profile_dir());
    let runtime_dir = cpp_runtime_dir_for_generated_build()?;
    let existing_prefix_paths = cmake_prefix_path_from_env();
    let target_sdk = toolchain_profile
        .map(|profile| resolve_cpp_target_sdk_root(runtime_dir.as_deref(), &profile.platform))
        .transpose()?;
    let cmake_runtime_dir = target_sdk
        .as_ref()
        .map(|sdk| sdk.root.as_path())
        .or(runtime_dir.as_deref());
    let cmake_prefix_paths = if let Some(sdk) = &target_sdk {
        cmake_prefix_paths_for_target_sdk(sdk, &existing_prefix_paths)
    } else {
        cmake_prefix_paths_for_runtime(runtime_dir.as_deref(), &existing_prefix_paths)
    };
    run_cmake_configure(
        &source_dir,
        &build_dir,
        cmake_runtime_dir,
        &cmake_prefix_paths,
        build_mode,
        toolchain_profile,
        target_sdk.as_ref(),
    )?;
    run_cmake_build(&build_dir)?;
    let cpp_app = build_dir.join(cpp_app_executable_name(contract));
    let ros2_bridge = build_dir.join(ros2_bridge_executable_name(contract));
    Ok(CmakeBuildOutputs {
        cpp_app: has_component_language(contract, LanguageKind::Cpp)
            .then_some(cpp_app)
            .and_then(existing_executable),
        ros2_bridge: has_ros2_bridge(contract)
            .then_some(ros2_bridge)
            .and_then(existing_executable),
    })
}

fn existing_executable(path: PathBuf) -> Option<PathBuf> {
    path.is_file().then_some(path)
}

fn run_cmake_configure(
    source_dir: &Path,
    build_dir: &Path,
    runtime_dir: Option<&Path>,
    cmake_prefix_paths: &[PathBuf],
    build_mode: BuildMode,
    toolchain_profile: Option<&ToolchainProfile>,
    target_sdk: Option<&CppTargetSdk>,
) -> Result<()> {
    let args = cmake_configure_args(
        source_dir,
        build_dir,
        runtime_dir,
        cmake_prefix_paths,
        build_mode,
        toolchain_profile,
    );
    let configure_env = cmake_configure_env(toolchain_profile, target_sdk);
    let mut command = ProcessCommand::new("cmake");
    command.args(args);
    for (key, value) in configure_env {
        command.env(key, value);
    }
    let status = command
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
    cmake_prefix_paths: &[PathBuf],
    build_mode: BuildMode,
    toolchain_profile: Option<&ToolchainProfile>,
) -> Vec<String> {
    let mut args = vec![
        "-S".to_string(),
        source_dir.to_string_lossy().into_owned(),
        "-B".to_string(),
        build_dir.to_string_lossy().into_owned(),
        format!("-DCMAKE_BUILD_TYPE={}", build_mode.cmake_build_type()),
    ];
    if let Some(runtime_dir) = runtime_dir {
        args.push(format!(
            "-DFLOWRT_CPP_RUNTIME_DIR={}",
            runtime_dir.to_string_lossy()
        ));
    }
    if !cmake_prefix_paths.is_empty() {
        args.push(format!(
            "-DCMAKE_PREFIX_PATH={}",
            join_cmake_prefix_paths(cmake_prefix_paths)
        ));
    }
    if repo_runtime_fallback_allowed() {
        args.push("-DFLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=ON".to_string());
    }
    if let Some(profile) = toolchain_profile {
        if let Some(cmake_toolchain) = &profile.cmake_toolchain {
            args.push(format!(
                "-DCMAKE_TOOLCHAIN_FILE={}",
                cmake_toolchain.to_string_lossy()
            ));
        } else {
            args.push(format!("-DCMAKE_C_COMPILER={}", profile.c_compiler));
            args.push(format!("-DCMAKE_CXX_COMPILER={}", profile.cpp_compiler));
            if let Some(sysroot) = &profile.sysroot {
                args.push(format!("-DCMAKE_SYSROOT={}", sysroot.to_string_lossy()));
            }
        }
    }
    args
}

fn cmake_configure_env(
    toolchain_profile: Option<&ToolchainProfile>,
    target_sdk: Option<&CppTargetSdk>,
) -> BTreeMap<&'static str, OsString> {
    let mut values = BTreeMap::new();
    let mut pkg_config_paths = Vec::new();
    if let Some(profile) = toolchain_profile
        && let Some(pkg_config_libdir) = &profile.pkg_config_libdir
    {
        push_unique_path(&mut pkg_config_paths, pkg_config_libdir);
    }
    if let Some(sdk) = target_sdk
        && let Some(pkgconfig_dir) = &sdk.pkgconfig_dir
        && pkgconfig_dir.is_dir()
    {
        push_unique_path(&mut pkg_config_paths, pkgconfig_dir);
    }
    if !pkg_config_paths.is_empty() {
        let joined = env::join_paths(&pkg_config_paths)
            .expect("PKG_CONFIG_LIBDIR paths should be joinable on this host");
        values.insert("PKG_CONFIG_LIBDIR", joined);
    }
    values
}

fn cmake_prefix_path_from_env() -> Vec<PathBuf> {
    let Some(raw) = env::var_os("CMAKE_PREFIX_PATH") else {
        return Vec::new();
    };
    env::split_paths(&raw).collect()
}

fn cmake_prefix_paths_for_runtime(
    runtime_dir: Option<&Path>,
    existing: &[PathBuf],
) -> Vec<PathBuf> {
    let mut prefixes = Vec::new();
    for prefix in existing {
        push_unique_path(&mut prefixes, prefix);
    }
    if let Some(runtime_dir) = runtime_dir {
        push_unique_path(&mut prefixes, runtime_dir);
        if let Some(private_prefix) = flowrt_private_prefix_from_cpp_runtime_dir(runtime_dir) {
            push_unique_path(&mut prefixes, &private_prefix);
        }
    }
    prefixes
}

fn cmake_prefix_paths_for_target_sdk(sdk: &CppTargetSdk, existing: &[PathBuf]) -> Vec<PathBuf> {
    let mut prefixes = Vec::new();
    push_unique_path(&mut prefixes, &sdk.root);
    if let Some(cmake_dir) = &sdk.cmake_dir {
        push_unique_path(&mut prefixes, cmake_dir);
    }
    for prefix in existing {
        push_unique_path(&mut prefixes, prefix);
    }
    prefixes
}

fn resolve_cpp_target_sdk_root(runtime_dir: Option<&Path>, platform: &str) -> Result<CppTargetSdk> {
    let runtime_dir = runtime_dir.with_context(|| {
        format!(
            "FlowRT target SDK for {platform} is missing; install a package that embeds this target SDK or configure FLOWRT_CPP_RUNTIME_DIR / toolchain profile to a complete SDK"
        )
    })?;
    let candidates = cpp_target_sdk_root_candidates(runtime_dir, platform);
    for candidate in &candidates {
        let manifest = candidate.join("flowrt-target-sdk.toml");
        if manifest.exists() {
            return read_cpp_target_sdk_manifest(candidate, platform);
        }
    }
    let searched = candidates
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::bail!(
        "FlowRT target SDK for {platform} is missing at {searched}; install a package that embeds this target SDK or configure FLOWRT_CPP_RUNTIME_DIR / toolchain profile to a complete SDK"
    );
}

fn cpp_target_sdk_root_candidates(runtime_dir: &Path, platform: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if runtime_dir.file_name() == Some(OsStr::new(platform))
        || runtime_dir.join("flowrt-target-sdk.toml").exists()
    {
        push_unique_path(&mut candidates, runtime_dir);
    }
    push_unique_path(&mut candidates, &runtime_dir.join("targets").join(platform));
    if let Some(private_prefix) = flowrt_private_prefix_from_runtime_dir(runtime_dir) {
        push_unique_path(
            &mut candidates,
            &private_prefix.join("targets").join(platform),
        );
    }
    if let Some(private_prefix) = flowrt_private_prefix_from_cpp_runtime_dir(runtime_dir) {
        push_unique_path(
            &mut candidates,
            &private_prefix.join("targets").join(platform),
        );
    }
    candidates
}

fn read_cpp_target_sdk_manifest(root: &Path, platform: &str) -> Result<CppTargetSdk> {
    let manifest_path = root.join("flowrt-target-sdk.toml");
    let source = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read `{}`", manifest_path.display()))?;
    let manifest: CppTargetSdkManifest = toml::from_str(&source)
        .with_context(|| format!("failed to parse `{}`", manifest_path.display()))?;
    if manifest.platform != platform {
        anyhow::bail!(
            "FlowRT target SDK manifest platform `{}` does not match requested `{platform}` at {}",
            manifest.platform,
            manifest_path.display()
        );
    }
    if !manifest.complete {
        anyhow::bail!(
            "FlowRT target SDK for {platform} is incomplete at {}; install a package that embeds this target SDK or configure FLOWRT_CPP_RUNTIME_DIR / toolchain profile to a complete SDK",
            root.display()
        );
    }
    let _include_dir = manifest
        .include_dir
        .as_ref()
        .map(|path| target_sdk_manifest_path(root, path));
    let _lib_dir = manifest
        .lib_dir
        .as_ref()
        .map(|path| target_sdk_manifest_path(root, path));
    Ok(CppTargetSdk {
        root: root.to_path_buf(),
        cmake_dir: manifest
            .cmake_dir
            .as_ref()
            .map(|path| target_sdk_manifest_path(root, path)),
        pkgconfig_dir: manifest
            .pkgconfig_dir
            .as_ref()
            .map(|path| target_sdk_manifest_path(root, path)),
    })
}

fn target_sdk_manifest_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn flowrt_private_prefix_from_cpp_runtime_dir(runtime_dir: &Path) -> Option<PathBuf> {
    if runtime_dir.join("include/flowrt/runtime.hpp").exists()
        && runtime_dir.join("lib").is_dir()
        && runtime_dir.join("share").is_dir()
    {
        return Some(runtime_dir.to_path_buf());
    }
    None
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: &Path) {
    if !paths.iter().any(|existing| existing == path) {
        paths.push(path.to_path_buf());
    }
}

fn join_cmake_prefix_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join(";")
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

fn run_cmake_app(app: &Path, process: Option<&str>, run_ticks: Option<usize>) -> Result<()> {
    if !app.exists() {
        anyhow::bail!(
            "C++ app executable `{}` not found; run `flowrt build` first",
            app.display()
        );
    }
    let mut command = ProcessCommand::new(app);
    if let Some(process) = process {
        command.arg("--process").arg(process);
    }
    if let Some(run_ticks) = run_ticks {
        command.arg("--flowrt-run-steps").arg(run_ticks.to_string());
    }
    let status = command
        .status()
        .with_context(|| format!("failed to spawn C++ app `{}`", app.display()))?;
    if !status.success() {
        anyhow::bail!("C++ app invocation failed with status {status}");
    }
    Ok(())
}

fn cpp_app_executable_name(contract: &ContractIr) -> String {
    format!(
        "{}_cpp_app{}",
        sanitize_package_name(&contract.package.name).replace('-', "_"),
        std::env::consts::EXE_SUFFIX
    )
}

fn ros2_bridge_executable_name(contract: &ContractIr) -> String {
    format!(
        "{}_ros2_bridge{}",
        sanitize_package_name(&contract.package.name).replace('-', "_"),
        std::env::consts::EXE_SUFFIX
    )
}

fn build_target_identity(target_profile: Option<&BuildToolchainProfile>) -> String {
    bin_target_identity(target_profile).unwrap_or_else(|| "native".to_string())
}

fn bin_target_identity(target_profile: Option<&BuildToolchainProfile>) -> Option<String> {
    target_profile
        .filter(|profile| profile.cargo_target_triple.is_some())
        .map(|profile| profile.profile.platform.clone())
}

fn copy_executable_to_local_bin(
    out_dir: &Path,
    build_mode: BuildMode,
    target_identity: Option<&str>,
    built: &Path,
) -> Result<PathBuf> {
    let file_name = built
        .file_name()
        .context("built executable path has no file name")?;
    let mut destination = out_dir.join("build").join("bin");
    if let Some(target_identity) = target_identity {
        destination = destination.join(target_identity);
    }
    let destination = destination
        .join(build_mode.cargo_profile_dir())
        .join(file_name);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    fs::copy(built, &destination).with_context(|| {
        format!(
            "failed to copy built executable `{}` to `{}`",
            built.display(),
            destination.display()
        )
    })?;
    Ok(destination)
}

fn relative_to_out_dir(out_dir: &Path, path: &Path) -> Result<PathBuf> {
    path.strip_prefix(out_dir)
        .map(Path::to_path_buf)
        .with_context(|| {
            format!(
                "built executable `{}` is not under FlowRT output directory `{}`",
                path.display(),
                out_dir.display()
            )
        })
}

fn run_binary(binary: &Path, process: Option<&str>, run_ticks: Option<usize>) -> Result<()> {
    let mut command = ProcessCommand::new(binary);
    if let Some(process) = process {
        command.arg("--process").arg(process);
    }
    if let Some(run_ticks) = run_ticks {
        command.arg("--flowrt-run-steps").arg(run_ticks.to_string());
    }
    let status = command
        .status()
        .with_context(|| format!("failed to spawn `{}`", binary.display()))?;
    if !status.success() {
        anyhow::bail!("app invocation failed with status {status}");
    }
    Ok(())
}

fn run_cargo_build_bin(
    manifest: &Path,
    bin_name: &str,
    build_mode: BuildMode,
    target_dir: &Path,
    target_triple: Option<&str>,
) -> Result<PathBuf> {
    let invocation =
        cargo_build_invocation(manifest, bin_name, build_mode, target_dir, target_triple)?;
    remove_stale_generated_binary_outputs(&invocation)?;
    ensure_rust_target_available(invocation.target_triple.as_deref())?;
    clean_generated_cargo_package(&invocation)?;
    let mut command = ProcessCommand::new("cargo");
    command
        .current_dir(&invocation.current_dir)
        .env("CARGO_TARGET_DIR", &invocation.target_dir)
        .args(&invocation.args);
    let status = command.status().context("failed to spawn cargo")?;
    if !status.success() {
        bail_cargo_status(
            "cargo invocation",
            status,
            invocation.target_triple.as_deref(),
        )?;
    }
    Ok(invocation.executable_path())
}

fn remove_stale_generated_binary_outputs(invocation: &CargoBuildInvocation) -> Result<()> {
    let profile_dir = invocation.profile_dir();
    remove_file_if_exists(&profile_dir.join(format!(
        "{}{}",
        invocation.bin_name,
        std::env::consts::EXE_SUFFIX
    )))?;
    remove_file_if_exists(&profile_dir.join(format!("{}.d", invocation.bin_name)))?;

    let deps_dir = profile_dir.join("deps");
    if deps_dir.is_dir() {
        let mut dep_prefixes = vec![invocation.bin_name.replace('-', "_")];
        if let Some(lib_name) = cargo_manifest_lib_name(&invocation.manifest_path)? {
            dep_prefixes.push(lib_name.clone());
            dep_prefixes.push(format!("lib{lib_name}"));
        }
        for entry in fs::read_dir(&deps_dir)
            .with_context(|| format!("failed to read `{}`", deps_dir.display()))?
        {
            let entry = entry
                .with_context(|| format!("failed to read entry in `{}`", deps_dir.display()))?;
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if dep_prefixes
                .iter()
                .any(|prefix| file_name.starts_with(prefix))
            {
                remove_file_if_exists(&entry.path())?;
            }
        }
    }
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("failed to remove `{}`", path.display())),
    }
}

fn clean_generated_cargo_package(invocation: &CargoBuildInvocation) -> Result<()> {
    let package_name = cargo_manifest_package_name(&invocation.manifest_path)?;
    let mut command = ProcessCommand::new("cargo");
    command
        .current_dir(&invocation.current_dir)
        .arg("clean")
        .arg("--manifest-path")
        .arg(&invocation.manifest_path)
        .arg("-p")
        .arg(&package_name)
        .env("CARGO_TARGET_DIR", &invocation.target_dir);
    for arg in cargo_target_args(invocation.target_triple.as_deref()) {
        command.arg(arg);
    }
    let status = command.status().with_context(|| {
        format!(
            "failed to spawn cargo clean for generated package `{}`",
            package_name
        )
    })?;
    if !status.success() {
        anyhow::bail!(
            "cargo clean for generated package `{package_name}` failed with status {status}"
        );
    }
    Ok(())
}

fn cargo_manifest_package_name(manifest: &Path) -> Result<String> {
    let content = fs::read_to_string(manifest)
        .with_context(|| format!("failed to read `{}`", manifest.display()))?;
    let value: toml::Value = toml::from_str(&content)
        .with_context(|| format!("failed to parse `{}`", manifest.display()))?;
    value
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .with_context(|| {
            format!(
                "Cargo manifest `{}` is missing package.name",
                manifest.display()
            )
        })
}

fn cargo_manifest_lib_name(manifest: &Path) -> Result<Option<String>> {
    let content = fs::read_to_string(manifest)
        .with_context(|| format!("failed to read `{}`", manifest.display()))?;
    let value: toml::Value = toml::from_str(&content)
        .with_context(|| format!("failed to parse `{}`", manifest.display()))?;
    Ok(value
        .get("lib")
        .and_then(|lib| lib.get("name"))
        .and_then(toml::Value::as_str)
        .map(str::to_string))
}

struct CargoBuildInvocation {
    manifest_path: PathBuf,
    current_dir: PathBuf,
    args: Vec<String>,
    target_dir: PathBuf,
    target_triple: Option<String>,
    bin_name: String,
    build_mode: BuildMode,
}

impl CargoBuildInvocation {
    fn profile_dir(&self) -> PathBuf {
        let target_dir = if let Some(target_triple) = &self.target_triple {
            self.target_dir.join(target_triple)
        } else {
            self.target_dir.clone()
        };
        target_dir.join(self.build_mode.cargo_profile_dir())
    }

    fn executable_path(&self) -> PathBuf {
        self.profile_dir()
            .join(format!("{}{}", self.bin_name, std::env::consts::EXE_SUFFIX))
    }
}

fn cargo_build_invocation(
    manifest: &Path,
    bin_name: &str,
    build_mode: BuildMode,
    target_dir: &Path,
    target_triple: Option<&str>,
) -> Result<CargoBuildInvocation> {
    let manifest = fs::canonicalize(manifest)
        .with_context(|| format!("failed to resolve `{}`", manifest.display()))?;
    let manifest_dir = manifest
        .parent()
        .with_context(|| format!("manifest path has no parent: `{}`", manifest.display()))?
        .to_path_buf();
    let mut args = vec![
        "build".to_string(),
        "--manifest-path".to_string(),
        manifest.to_string_lossy().into_owned(),
        "--bin".to_string(),
        bin_name.to_string(),
    ];
    args.extend(build_mode.cargo_args().iter().map(|arg| (*arg).to_string()));
    args.extend(cargo_target_args(target_triple));
    if manifest_dir.join(".cargo").join("config.toml").exists() {
        args.push("--offline".to_string());
    }
    Ok(CargoBuildInvocation {
        manifest_path: manifest,
        current_dir: manifest_dir,
        args,
        target_dir: target_dir.to_path_buf(),
        target_triple: target_triple.map(str::to_string),
        bin_name: bin_name.to_string(),
        build_mode,
    })
}

fn run_supervisor_binary(binary: &Path, run_ticks: Option<usize>) -> Result<()> {
    let mut command = ProcessCommand::new(binary);
    if let Some(run_ticks) = run_ticks {
        command.arg("--flowrt-run-steps").arg(run_ticks.to_string());
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
    if repo_runtime_fallback_allowed() {
        return Ok(repo_runtime_dir("runtime/rust", "Cargo.toml"));
    }
    Ok(None)
}

fn cpp_runtime_dir_for_generated_build() -> Result<Option<PathBuf>> {
    if let Some(runtime_dir) = cpp_runtime_dir_from_env()? {
        return Ok(Some(runtime_dir));
    }
    if let Some(runtime_dir) = installed_runtime_dir("runtime/cpp", "include/flowrt/runtime.hpp")? {
        return Ok(Some(runtime_dir));
    }
    if repo_runtime_fallback_allowed() {
        return Ok(repo_runtime_dir(
            "runtime/cpp",
            "include/flowrt/runtime.hpp",
        ));
    }
    Ok(None)
}

fn cpp_runtime_dir_from_env() -> Result<Option<PathBuf>> {
    let Some(raw) = env::var_os("FLOWRT_CPP_RUNTIME_DIR") else {
        return Ok(None);
    };
    let runtime_dir = PathBuf::from(raw);
    if runtime_dir.join("include/flowrt/runtime.hpp").exists() {
        return Ok(Some(runtime_dir));
    }
    let nested_runtime_dir = runtime_dir.join("runtime/cpp");
    if nested_runtime_dir
        .join("include/flowrt/runtime.hpp")
        .exists()
    {
        return Ok(Some(nested_runtime_dir));
    }
    anyhow::bail!(
        "FLOWRT_CPP_RUNTIME_DIR points to `{}`, but neither `{}` nor `{}` exists; set it to a valid FlowRT C++ runtime directory or private FlowRT prefix",
        runtime_dir.display(),
        runtime_dir.join("include/flowrt/runtime.hpp").display(),
        nested_runtime_dir
            .join("include/flowrt/runtime.hpp")
            .display()
    );
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
    let current_exe = fs::canonicalize(&current_exe).unwrap_or(current_exe);
    for runtime_dir in installed_runtime_candidates(&current_exe, relative) {
        if runtime_dir.join(marker).exists() {
            return Ok(Some(runtime_dir));
        }
    }
    Ok(None)
}

fn installed_runtime_candidates(current_exe: &Path, relative: &str) -> Vec<PathBuf> {
    let Some(bin_dir) = current_exe.parent() else {
        return Vec::new();
    };
    let Some(prefix) = bin_dir.parent() else {
        return Vec::new();
    };
    let mut candidates = vec![
        prefix.join("share").join("flowrt").join(relative),
        prefix
            .join("share")
            .join("flowrt")
            .join(relative.strip_prefix("runtime/cpp").unwrap_or(relative)),
        prefix
            .parent()
            .map(|usr| usr.join("share").join("flowrt").join(relative))
            .unwrap_or_else(|| prefix.join("__missing__")),
    ];
    if relative == "runtime/cpp" {
        candidates.insert(0, prefix.to_path_buf());
    }
    candidates
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

fn repo_runtime_fallback_allowed() -> bool {
    env::var_os("FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK")
        .map(|v| v == "1" || v == "ON" || v == "on" || v == "true" || v == "TRUE")
        .unwrap_or(false)
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

fn require_image_for_remote(image: Option<&Path>) -> Result<PathBuf> {
    image.map(Path::to_path_buf).context(
        "`--remote` requires an image path to extract the self-description hash; \
         pass `--image <path>`",
    )
}

fn require_image_for_local(image: Option<&Path>) -> Result<PathBuf> {
    image.map(Path::to_path_buf).context(
        "missing required argument `<image>`; \
         pass a FlowRT application binary or selfdesc.json path",
    )
}

fn params_remote_runtime_arg(
    remote: bool,
    socket: Option<&Path>,
    runtime: Option<&str>,
) -> Result<Option<String>> {
    if remote {
        if socket.is_some() {
            anyhow::bail!(
                "`--socket` selects a local Unix socket and cannot be used with `--remote`; \
                 use `--runtime <key_expr>` to select a remote FlowRT runtime"
            );
        }
        Ok(runtime.map(str::to_string))
    } else {
        if runtime.is_some() {
            anyhow::bail!(
                "`--runtime` can only be used with `--remote`; \
                 use `--socket <path>` for local params"
            );
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
