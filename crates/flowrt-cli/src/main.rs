use std::env;
use std::io;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(test)]
use std::process::Command as ProcessCommand;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{ArgGroup, Parser, Subcommand};
use flowrt_codegen::{explain_report, format_explain_report_text, handler_signature_summary};
#[cfg(test)]
use flowrt_ir::{ContractIr, GraphMode, LanguageKind, hash_source};
#[cfg(test)]
use flowrt_validate::validate_contract;

mod app_add;
mod boundary_pub;
mod build_model;
mod cache;
mod fault_matrix;
mod frame_json;
mod introspection;
mod project_manifest;
mod record;
mod replay;
mod toolchain;
mod workflows;

use app_add::{
    AddCommand, AddComponentSpec, add_component_to_rsdl, add_message_to_rsdl, add_module_to_rsdl,
};
use boundary_pub::{boundary_publish, boundary_publish_from_file};
use build_model::BuildMode;
#[cfg(test)]
use build_model::{CacheLayout, DepsCacheKey, RuntimeFeatureSet};
use cache::{CacheCleanOptions, cache_clean_for_cwd, cache_status_summary_for_cwd};
use introspection::{
    EchoFormatOptions, EchoSelection, echo_channel, echo_channel_follow, echo_channels,
    echo_channels_follow, live_hz_summary, live_status_json, live_status_summary,
    load_self_description, operation_cancel, operation_cancel_json, operation_follow,
    operation_follow_json, operation_list, operation_list_json, operation_result,
    operation_result_json, operation_start, operation_start_json, operation_status_json,
    operation_status_summary, params_get, params_list, params_set, params_set_from_file,
    remote_operation_cancel, remote_operation_cancel_json, remote_operation_start,
    remote_operation_start_json, remote_operation_status, remote_operation_status_json,
    remote_params_get, remote_params_list, remote_params_set, remote_params_set_from_file,
    self_description_nodes, self_description_summary,
};
use record::{RecordOptions, record_runtime};
use replay::replay_fixture;
#[cfg(test)]
use toolchain::{RuntimeDependencyPolicy, ToolchainProfile};
use workflows::*;

#[cfg(test)]
use app_add::AppAddLanguage;

#[cfg(test)]
use flowrt_selfdesc::SelfDescription;
#[cfg(test)]
use introspection::{
    EchoTarget, echo_channel_follow_for_polls, echo_channel_from_image,
    echo_channel_from_image_with_options, echo_channel_snapshot_from_image, find_echo_channel,
    format_hz_summary_from_status_pair, live_hz_summary_for_sockets,
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
    /// 初始化现代 FlowRT app 项目骨架。
    Init {
        /// 项目根目录；省略时初始化当前目录。
        #[arg(default_value = ".")]
        path: PathBuf,

        /// 初始 RSDL 目标语言；当前开放 Rust/C++ 和 C callback table v0。
        #[arg(long = "lang", value_enum, default_value = "rust")]
        language: AppInitLanguage,
    },

    /// 向当前 FlowRT app 追加 message、module 或 component 骨架。
    Add {
        #[command(subcommand)]
        command: AddCommand,
    },

    /// 解析、归一化并校验一个 RSDL 文件。
    Check {
        /// .rsdl 文件路径；省略时从 flowrt.toml 的 project.main 发现。
        rsdl: Option<PathBuf>,
    },

    /// 验证或运行 test-only fault matrix。
    FaultMatrix {
        #[command(subcommand)]
        command: FaultMatrixCommand,
    },

    /// 展示组件实现 API、task 和 handle 详情。
    Explain {
        /// .rsdl 文件路径；省略时从 flowrt.toml 的 project.main 发现。
        rsdl: Option<PathBuf>,

        /// 输出格式。
        #[arg(long, default_value_t, value_enum)]
        format: ExplainFormat,
    },

    /// 准备 FlowRT 管理的应用产物。
    Prepare {
        /// .rsdl 文件路径；省略时从 flowrt.toml 的 project.main 发现。
        rsdl: Option<PathBuf>,

        /// FlowRT 管理产物输出目录。
        #[arg(long, default_value = "flowrt")]
        out_dir: PathBuf,

        /// 选择用于生成产物的 profile 名称。
        #[arg(long)]
        profile: Option<String>,

        /// 生成一次性 test-only island projection，不修改源 RSDL。
        #[arg(long)]
        temporary_island: bool,

        /// 临时 boundary input 映射，格式为 `name=instance.port`。
        #[arg(long = "boundary-input")]
        boundary_input: Vec<String>,

        /// 临时 boundary output 映射，格式为 `name=instance.port`。
        #[arg(long = "boundary-output")]
        boundary_output: Vec<String>,

        /// test-only 故障注入场景文件（TOML）；命中 task 调用序号时强制 Status::Error。
        #[arg(long)]
        inject: Option<PathBuf>,
    },

    /// 准备并构建 FlowRT 管理的应用产物。
    Build {
        /// .rsdl 文件路径；省略时从 flowrt.toml 的 project.main 发现。
        rsdl: Option<PathBuf>,

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

        /// 生成一次性 test-only island projection，不修改源 RSDL。
        #[arg(long)]
        temporary_island: bool,

        /// 临时 boundary input 映射，格式为 `name=instance.port`。
        #[arg(long = "boundary-input")]
        boundary_input: Vec<String>,

        /// 临时 boundary output 映射，格式为 `name=instance.port`。
        #[arg(long = "boundary-output")]
        boundary_output: Vec<String>,

        /// test-only 故障注入场景文件（TOML）；命中 task 调用序号时强制 Status::Error。
        #[arg(long)]
        inject: Option<PathBuf>,
    },

    /// 补全并预热 FlowRT 底层依赖缓存。
    Deps {
        /// 可选 RSDL 文件路径；省略时优先从 flowrt.toml 发现，仍找不到则进入无契约预热模式。
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

    /// 查看或安全清理 FlowRT cache。
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },

    /// 预检 FlowRT 本机或交叉编译环境。
    Doctor {
        /// 可选 .rsdl 文件路径；省略时优先从 flowrt.toml 发现，仍找不到则只检查基础环境。
        rsdl: Option<PathBuf>,

        /// 目标 platform；例如 linux-arm64。省略时检查 native 基础环境。
        #[arg(long)]
        target: Option<String>,
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

        /// 允许打包 island 脚手架产物；默认拒绝，避免误发为生产 bundle。
        #[arg(long)]
        allow_island: bool,
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

        /// 允许部署 island 脚手架 bundle；默认拒绝，避免误部署为生产系统。
        #[arg(long)]
        allow_island: bool,
    },

    /// 输出远端部署前置校验指纹；供 `flowrt deploy` 通过 SSH 调用。
    #[command(hide = true)]
    DeployProbe {
        /// 要部署的 canonical target platform。
        #[arg(long)]
        target_platform: String,
    },

    /// 准备并运行 FlowRT 管理的应用 crate。
    Run {
        /// .rsdl 文件路径；省略时从 flowrt.toml 的 project.main 发现。
        rsdl: Option<PathBuf>,

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

        /// 先生成一次性 test-only island projection 并构建运行，不修改源 RSDL。
        #[arg(long)]
        temporary_island: bool,

        /// 临时 boundary input 映射，格式为 `name=instance.port`。
        #[arg(long = "boundary-input")]
        boundary_input: Vec<String>,

        /// 临时 boundary output 映射，格式为 `name=instance.port`。
        #[arg(long = "boundary-output")]
        boundary_output: Vec<String>,

        /// test-only 故障注入场景文件（TOML）；命中 task 调用序号时强制 Status::Error。
        #[arg(long)]
        inject: Option<PathBuf>,

        /// 运行时原生确定性回放源（MCAP 录制）。设置后以 FLOWRT_REPLAY_SOURCE 注入生成运行时；
        /// 仅 simulated_replay 时钟源（temporary island）消费，realtime 忽略。
        #[arg(long)]
        replay: Option<PathBuf>,
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

        /// 额外 channel 名称；旧式兼容用法中单个值表示 `<image> <channel>`。
        channel: Vec<String>,

        /// 显式提供 FlowRT 应用二进制或 selfdesc.json；省略时从 live runtime 请求 self-description。
        #[arg(long)]
        image: Option<PathBuf>,

        /// 显式指定 runtime introspection socket；省略时按 selfdesc hash 自动匹配。
        #[arg(long)]
        socket: Option<PathBuf>,

        /// 持续轮询该 channel；按 Ctrl-C 结束。
        #[arg(long)]
        follow: bool,

        /// 完整输出 payload，不对长 sequence 做摘要。
        #[arg(long)]
        raw: bool,

        /// `--follow` 模式下的轮询间隔，单位毫秒。
        #[arg(long, default_value_t = 250, value_parser = clap::value_parser!(u64).range(1..))]
        interval_ms: u64,
    },

    /// 向 island boundary input 注入 typed JSON 数据。
    #[command(group(
        ArgGroup::new("pub-input")
            .required(true)
            .args(["json", "file"])
    ))]
    Pub {
        /// boundary input endpoint 名称，例如 `sample_in`。
        endpoint: String,

        /// JSON 对象或 primitive，按 self-description Message ABI 编码后注入。
        #[arg(long, group = "pub-input")]
        json: Option<String>,

        /// JSONL、JSON array 或单个 JSON value 文件，逐条按 self-description 编码后注入。
        #[arg(long, group = "pub-input")]
        file: Option<PathBuf>,

        /// `--file` 模式下的 wall-clock 注入频率，单位 Hz；不解释或修改消息字段时间戳。
        #[arg(long, value_parser = parse_positive_f64)]
        freq: Option<f64>,

        /// FlowRT 管理应用二进制，或 flowrt/selfdesc/selfdesc.json；省略时从 live runtime 请求。
        #[arg(long)]
        image: Option<PathBuf>,

        /// 显式指定 runtime introspection socket；省略时按 selfdesc hash 自动匹配。
        #[arg(long)]
        socket: Option<PathBuf>,

        /// 覆盖注入样本的 runtime 毫秒时间戳；省略时由 runtime 作为无时间戳样本处理。
        #[arg(long)]
        published_at_ms: Option<u64>,
    },

    /// 按 FlowRT-native fixture 回放事件，驱动多个 island boundary input。
    Replay {
        /// FlowRT replay fixture，支持 JSONL 或 JSON array。
        #[arg(long)]
        file: PathBuf,

        /// FlowRT 管理应用二进制，或 flowrt/selfdesc/selfdesc.json。
        #[arg(long)]
        image: PathBuf,

        /// 显式指定 runtime introspection socket；省略时按 selfdesc hash 自动匹配。
        #[arg(long)]
        socket: Option<PathBuf>,

        /// 按事件 at_ms/dt_ms 的回放速度倍率。
        #[arg(long, default_value_t = 1.0, value_parser = parse_positive_f64)]
        speed: f64,

        /// 忽略事件时间，尽快注入所有事件。
        #[arg(long)]
        as_fast_as_possible: bool,
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

        /// 输出格式。text 面向人读，json 面向脚本消费完整 status/diagnostics。
        #[arg(long, value_enum, default_value_t = StatusFormat::Text)]
        format: StatusFormat,
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

    /// 管理 toolchain profile 配置。
    Toolchain {
        #[command(subcommand)]
        command: ToolchainCommand,
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

    /// 设置 live runtime 参数 pending 值。
    Set {
        /// 参数名，格式为 `<instance>.<param>`。
        #[arg(required_unless_present = "file", conflicts_with = "file")]
        name: Option<String>,

        /// JSON 参数值，例如 `2.5`、`true` 或 `"safe"`。
        #[arg(required_unless_present = "file", conflicts_with = "file")]
        value: Option<String>,

        /// 从 JSON 文件批量导入参数。
        #[arg(long, conflicts_with_all = ["name", "value"])]
        file: Option<PathBuf>,

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

        /// 输出格式。
        #[arg(long, value_enum, default_value_t = OperationOutputFormat::Text)]
        format: OperationOutputFormat,
    },

    /// 查看 live Operation 健康状态。
    Status {
        /// 可选 Operation 名称，格式 `<client_instance>.<client_port>`。
        name: Option<String>,

        /// FlowRT 管理应用二进制，或 flowrt/selfdesc/selfdesc.json。
        #[arg(long)]
        image: Option<PathBuf>,

        /// 显式指定 runtime introspection socket。
        #[arg(long)]
        socket: Option<PathBuf>,

        /// 精确选择远端 runtime key expression。
        #[arg(long)]
        runtime: Option<String>,

        /// 通过 zenoh control-plane 发现远端 runtime。
        #[arg(long)]
        remote: bool,

        /// 远程发现和请求超时毫秒。
        #[arg(long, default_value_t = 5000, value_parser = clap::value_parser!(u64).range(1..))]
        timeout_ms: u64,

        /// 输出格式。
        #[arg(long, value_enum, default_value_t = OperationOutputFormat::Text)]
        format: OperationOutputFormat,
    },

    /// 启动 live Operation invocation。
    Start {
        /// Operation 名称，格式 `<client_instance>.<client_port>`。
        name: String,

        /// goal JSON 文本。
        #[arg(long, conflicts_with = "file", required_unless_present = "file")]
        json: Option<String>,

        /// goal JSON 文件。
        #[arg(long, conflicts_with = "json", required_unless_present = "json")]
        file: Option<PathBuf>,

        /// FlowRT 管理应用二进制，或 flowrt/selfdesc/selfdesc.json。
        #[arg(long)]
        image: Option<PathBuf>,

        /// 显式指定 runtime introspection socket。
        #[arg(long)]
        socket: Option<PathBuf>,

        /// 精确选择远端 runtime key expression。
        #[arg(long)]
        runtime: Option<String>,

        /// 通过 zenoh control-plane 发现远端 runtime。
        #[arg(long)]
        remote: bool,

        /// Operation start 请求超时毫秒；省略时使用 contract 默认值。
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        timeout_ms: Option<u64>,

        /// start accepted 后持续输出 progress/state/result，直到 terminal。
        #[arg(long)]
        follow: bool,

        /// 输出格式。
        #[arg(long, value_enum, default_value_t = OperationOutputFormat::Text)]
        format: OperationOutputFormat,
    },

    /// 取消 live Operation invocation。
    Cancel {
        /// `flowrt op status` 输出中的 operation id。
        operation_id: String,

        /// FlowRT 管理应用二进制，或 flowrt/selfdesc/selfdesc.json。
        #[arg(long)]
        image: Option<PathBuf>,

        /// 显式指定 runtime introspection socket。
        #[arg(long)]
        socket: Option<PathBuf>,

        /// 精确选择远端 runtime key expression。
        #[arg(long)]
        runtime: Option<String>,

        /// 通过 zenoh control-plane 发现远端 runtime。
        #[arg(long)]
        remote: bool,

        /// 远程发现和请求超时毫秒。
        #[arg(long, default_value_t = 5000, value_parser = clap::value_parser!(u64).range(1..))]
        timeout_ms: u64,

        /// 输出格式。
        #[arg(long, value_enum, default_value_t = OperationOutputFormat::Text)]
        format: OperationOutputFormat,
    },

    /// 读取 live Operation invocation 的 retained result。
    Result {
        /// `flowrt op start/status` 输出中的 operation id。
        operation_id: String,

        /// FlowRT 管理应用二进制，或 flowrt/selfdesc/selfdesc.json。
        #[arg(long)]
        image: Option<PathBuf>,

        /// 显式指定 runtime introspection socket。
        #[arg(long)]
        socket: Option<PathBuf>,

        /// 精确选择远端 runtime key expression。
        #[arg(long)]
        runtime: Option<String>,

        /// 通过 zenoh control-plane 发现远端 runtime。
        #[arg(long)]
        remote: bool,

        /// 远程发现和请求超时毫秒。
        #[arg(long, default_value_t = 5000, value_parser = clap::value_parser!(u64).range(1..))]
        timeout_ms: u64,

        /// 输出格式。
        #[arg(long, value_enum, default_value_t = OperationOutputFormat::Text)]
        format: OperationOutputFormat,
    },

    /// 跟随 live Operation invocation 的 progress/state/result。
    Follow {
        /// `flowrt op start/status` 输出中的 operation id。
        operation_id: String,

        /// FlowRT 管理应用二进制，或 flowrt/selfdesc/selfdesc.json。
        #[arg(long)]
        image: Option<PathBuf>,

        /// 显式指定 runtime introspection socket。
        #[arg(long)]
        socket: Option<PathBuf>,

        /// 精确选择远端 runtime key expression。
        #[arg(long)]
        runtime: Option<String>,

        /// 通过 zenoh control-plane 发现远端 runtime。
        #[arg(long)]
        remote: bool,

        /// 远程发现和请求超时毫秒。
        #[arg(long, default_value_t = 5000, value_parser = clap::value_parser!(u64).range(1..))]
        timeout_ms: u64,

        /// 输出格式。
        #[arg(long, value_enum, default_value_t = OperationOutputFormat::Text)]
        format: OperationOutputFormat,
    },
}

#[derive(Debug, Subcommand)]
enum ToolchainCommand {
    /// 展示指定 platform 的合并后 toolchain profile。
    Show {
        /// 目标 platform，例如 linux-arm64。
        #[arg(long)]
        target: String,
    },

    /// 初始化 workspace 级 toolchain 配置文件。
    Init {
        /// 目标 platform，当前支持 linux-arm64。
        #[arg(long)]
        target: String,

        /// 显式 SDK overlay 路径，可重复。
        #[arg(long)]
        sdk_overlay: Vec<PathBuf>,

        /// 覆盖已有配置文件。
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Subcommand)]
enum CacheCommand {
    /// 展示当前 FlowRT cache、项目 build 和临时候选占用。
    Status,

    /// 按显式范围安全清理 FlowRT cache。
    Clean {
        /// 目标 platform；例如 linux-arm64。用于过滤 deps cache 与项目 build 子目录。
        #[arg(long)]
        target: Option<String>,

        /// 构建模式过滤；省略时匹配全部模式。
        #[arg(long, value_enum)]
        build_mode: Option<BuildMode>,

        /// 只输出计划，不执行删除。
        #[arg(long)]
        dry_run: bool,

        /// 清理 FlowRT deps cache、deps workspace 和 ready marker。
        #[arg(long)]
        flowrt_deps: bool,

        /// 清理当前项目 `flowrt/build` 可重建目录。
        #[arg(long)]
        project_build: bool,

        /// 只清理 Cargo incremental 目录，保留其余 deps cache。
        #[arg(long)]
        incremental: bool,

        /// 清理已确认 stale 的 FlowRT/zenoh 临时目录或 socket 候选。
        #[arg(long)]
        stale_temp: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum AppInitLanguage {
    Rust,
    C,
    Cpp,
}

impl AppInitLanguage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::C => "c",
            Self::Cpp => "cpp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum DepsBackend {
    Inproc,
    Iox2,
    Zenoh,
    All,
}

#[derive(Debug, Subcommand)]
enum FaultMatrixCommand {
    /// 验证 test-only fault matrix，不构建、不运行。
    Check {
        /// fault-matrix.toml 路径。
        matrix: PathBuf,
    },

    /// 运行 test-only fault matrix 并校验最终 status 证据。
    Run {
        /// fault-matrix.toml 路径。
        matrix: PathBuf,

        /// 每个 case 的 FlowRT 管理产物输出根目录。
        #[arg(long, default_value = "target/flowrt-fault-matrix")]
        out_dir: PathBuf,

        /// JSON report 输出路径；省略时打印到 stdout。
        #[arg(long)]
        report: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
enum ExplainFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
enum StatusFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
enum OperationOutputFormat {
    #[default]
    Text,
    Json,
}

fn resolve_required_cli_rsdl(rsdl: Option<PathBuf>) -> Result<PathBuf> {
    let cwd = env::current_dir().context("读取当前目录失败")?;
    project_manifest::resolve_rsdl_arg(rsdl, &cwd)
}

fn resolve_optional_cli_rsdl(rsdl: Option<PathBuf>) -> Result<Option<PathBuf>> {
    let cwd = env::current_dir().context("读取当前目录失败")?;
    project_manifest::resolve_optional_rsdl_arg(rsdl, &cwd)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { path, language } => {
            println!("{}", init_app_project(&path, language)?);
        }
        Command::Add { command } => match command {
            AddCommand::Message { name, fields, rsdl } => {
                let rsdl = resolve_required_cli_rsdl(rsdl)?;
                println!("{}", add_message_to_rsdl(&rsdl, &name, &fields)?);
            }
            AddCommand::Module { name, rsdl } => {
                let rsdl = resolve_required_cli_rsdl(rsdl)?;
                println!("{}", add_module_to_rsdl(&rsdl, &name)?);
            }
            AddCommand::Component {
                name,
                language,
                inputs,
                outputs,
                rsdl,
            } => {
                let rsdl = resolve_required_cli_rsdl(rsdl)?;
                println!(
                    "{}",
                    add_component_to_rsdl(
                        &rsdl,
                        AddComponentSpec {
                            name,
                            language,
                            inputs,
                            outputs,
                        },
                    )?
                );
            }
        },
        Command::Check { rsdl } => {
            let rsdl = resolve_required_cli_rsdl(rsdl)?;
            let contract = load_contract_from_rsdl(&rsdl)?;
            println!("OK {}", summary(&contract));
            println!("{}", handler_signature_summary(&contract));
        }
        Command::FaultMatrix { command } => match command {
            FaultMatrixCommand::Check { matrix } => {
                let report = fault_matrix::check::check_matrix(&matrix)?;
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
            FaultMatrixCommand::Run {
                matrix,
                out_dir,
                report,
            } => {
                let report_value = fault_matrix::runner::run_matrix(&matrix, &out_dir)?;
                let json = serde_json::to_string_pretty(&report_value)?;
                if let Some(report) = report {
                    std::fs::write(&report, format!("{json}\n")).with_context(|| {
                        format!("failed to write fault matrix report `{}`", report.display())
                    })?;
                } else {
                    println!("{json}");
                }
                report_value.ensure_passed()?;
            }
        },
        Command::Explain { rsdl, format } => {
            let rsdl = resolve_required_cli_rsdl(rsdl)?;
            let contract = load_contract_from_rsdl(&rsdl)?;
            let report = explain_report(&contract);
            match format {
                ExplainFormat::Text => println!("{}", format_explain_report_text(&report)),
                ExplainFormat::Json => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&report)
                            .context("序列化 explain JSON 失败")?
                    );
                }
            }
        }
        Command::Prepare {
            rsdl,
            out_dir,
            profile,
            temporary_island,
            boundary_input,
            boundary_output,
            inject,
        } => {
            let rsdl = resolve_required_cli_rsdl(rsdl)?;
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let _lock = WorkspaceLock::acquire(&out_dir)?;
            let overlay =
                TemporaryIslandCliOptions::new(temporary_island, boundary_input, boundary_output);
            let prepared = prepare_workspace_with_options(
                &rsdl,
                &out_dir,
                profile.as_deref(),
                &overlay,
                inject.as_deref(),
            )?;
            println!(
                "prepared {} and {} artifact(s)",
                prepared.contract_path.display(),
                prepared.artifact_count
            );
            if let Some(summary) = overlay_summary(&prepared.selected_contract) {
                println!("{summary}");
            }
        }
        Command::Build {
            rsdl,
            out_dir,
            launcher,
            profile,
            target,
            build_mode,
            temporary_island,
            boundary_input,
            boundary_output,
            inject,
        } => {
            let rsdl = resolve_required_cli_rsdl(rsdl)?;
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let _lock = WorkspaceLock::acquire(&out_dir)?;
            let overlay =
                TemporaryIslandCliOptions::new(temporary_island, boundary_input, boundary_output);
            let prepared = prepare_workspace_with_options(
                &rsdl,
                &out_dir,
                profile.as_deref(),
                &overlay,
                inject.as_deref(),
            )?;
            let workspace_root = application_root_from_rsdl(&rsdl)?;
            let target_profile = resolve_build_toolchain_profile(
                &prepared.selected_contract,
                target.as_deref(),
                &workspace_root,
            )?;
            let build_info = build_workspace(
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
            if let Some(summary) = overlay_summary(&prepared.selected_contract) {
                println!("{summary}");
            }
            println!(
                "{}",
                format_build_success_summary(
                    &prepared.selected_contract,
                    &build_info,
                    target_profile.as_ref(),
                    &out_dir,
                )
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
            let rsdl = resolve_optional_cli_rsdl(rsdl)?;
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
        Command::Cache { command } => match command {
            CacheCommand::Status => {
                println!("{}", cache_status_summary_for_cwd(&env::current_dir()?)?);
            }
            CacheCommand::Clean {
                target,
                build_mode,
                dry_run,
                flowrt_deps,
                project_build,
                incremental,
                stale_temp,
            } => {
                println!(
                    "{}",
                    cache_clean_for_cwd(
                        &env::current_dir()?,
                        CacheCleanOptions {
                            target,
                            build_mode,
                            dry_run,
                            flowrt_deps,
                            project_build,
                            incremental,
                            stale_temp,
                        },
                    )?
                );
            }
        },
        Command::Doctor { rsdl, target } => {
            let rsdl = resolve_optional_cli_rsdl(rsdl)?;
            run_doctor(rsdl.as_deref(), target.as_deref())?;
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
            allow_island,
        } => {
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let build_hint = build_command_hint(&rsdl, profile.as_deref(), true);
            let contract = load_prepared_contract(&out_dir, &build_hint)?;
            ensure_prepared_profile_matches(&contract, profile.as_deref(), &build_hint)?;
            println!(
                "{}",
                bundle_workspace(
                    &rsdl,
                    &contract,
                    &out_dir,
                    &output,
                    build_mode,
                    allow_island
                )?
            );
        }
        Command::Deploy {
            bundle,
            host,
            target,
            remote_dir,
            dry_run,
            allow_island,
        } => {
            println!(
                "{}",
                deploy_bundle(&bundle, &host, &target, &remote_dir, dry_run, allow_island)?
            );
        }
        Command::DeployProbe { target_platform } => {
            println!("{}", deploy_probe(&target_platform)?);
        }
        Command::Run {
            rsdl,
            out_dir,
            process,
            run_ticks,
            profile,
            build_mode,
            temporary_island,
            boundary_input,
            boundary_output,
            inject,
            replay,
        } => {
            let rsdl = resolve_required_cli_rsdl(rsdl)?;
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let overlay =
                TemporaryIslandCliOptions::new(temporary_island, boundary_input, boundary_output);
            let contract = if overlay.enabled || inject.is_some() {
                let _lock = WorkspaceLock::acquire(&out_dir)?;
                let prepared = prepare_workspace_with_options(
                    &rsdl,
                    &out_dir,
                    profile.as_deref(),
                    &overlay,
                    inject.as_deref(),
                )?;
                let workspace_root = application_root_from_rsdl(&rsdl)?;
                let target_profile = resolve_build_toolchain_profile(
                    &prepared.selected_contract,
                    None,
                    &workspace_root,
                )?;
                let build_info = build_workspace(
                    &prepared.selected_contract,
                    &out_dir,
                    false,
                    build_mode.unwrap_or_default(),
                    target_profile.as_ref(),
                )?;
                if let Some(summary) = overlay_summary(&prepared.selected_contract) {
                    println!("{summary}");
                }
                println!(
                    "{}",
                    format_build_success_summary(
                        &prepared.selected_contract,
                        &build_info,
                        target_profile.as_ref(),
                        &out_dir,
                    )
                );
                prepared.selected_contract
            } else {
                let build_hint = build_command_hint(&rsdl, profile.as_deref(), false);
                let contract = load_prepared_contract(&out_dir, &build_hint)?;
                ensure_prepared_profile_matches(&contract, profile.as_deref(), &build_hint)?;
                contract
            };
            if replay.is_some() && contract.artifact.clock_source.is_realtime() {
                eprintln!(
                    "FlowRT: --replay 仅用于 simulated_replay 时钟源（temporary island）；当前为 realtime，回放源被忽略"
                );
            }
            run_workspace(
                &contract,
                &out_dir,
                process.as_deref(),
                run_ticks,
                build_mode,
                replay.as_deref(),
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
            raw,
            interval_ms,
        } => {
            let echo_target = EchoSelection::from_cli(target, channel, image)?;
            let echo_options = EchoFormatOptions { raw };
            if follow {
                if echo_target.channels.len() == 1 {
                    echo_channel_follow(
                        &echo_target.to_single_target()?,
                        socket.as_deref(),
                        Duration::from_millis(interval_ms),
                        echo_options,
                        &mut io::stdout(),
                    )?;
                } else {
                    echo_channels_follow(
                        &echo_target,
                        socket.as_deref(),
                        Duration::from_millis(interval_ms),
                        echo_options,
                        &mut io::stdout(),
                    )?;
                }
            } else if echo_target.channels.len() == 1 {
                println!(
                    "{}",
                    echo_channel(
                        &echo_target.to_single_target()?,
                        socket.as_deref(),
                        echo_options
                    )?
                );
            } else {
                println!(
                    "{}",
                    echo_channels(&echo_target, socket.as_deref(), echo_options)?
                );
            }
        }
        Command::Pub {
            endpoint,
            json,
            file,
            freq,
            image,
            socket,
            published_at_ms,
        } => {
            let output = match (json, file) {
                (Some(json), None) => {
                    if freq.is_some() {
                        anyhow::bail!("flowrt pub --freq is only valid with --file");
                    }
                    boundary_publish(
                        &endpoint,
                        &json,
                        image.as_deref(),
                        socket.as_deref(),
                        published_at_ms,
                    )?
                }
                (None, Some(file)) => boundary_publish_from_file(
                    &endpoint,
                    &file,
                    image.as_deref(),
                    socket.as_deref(),
                    published_at_ms,
                    freq,
                )?,
                (None, None) => {
                    anyhow::bail!("flowrt pub requires exactly one of --json or --file");
                }
                (Some(_), Some(_)) => unreachable!("clap enforces pub input exclusivity"),
            };
            println!("{output}");
        }
        Command::Replay {
            file,
            image,
            socket,
            speed,
            as_fast_as_possible,
        } => {
            println!(
                "{}",
                replay_fixture(&file, &image, socket.as_deref(), speed, as_fast_as_possible)?
            );
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
                file,
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
                    if let Some(file) = file.as_deref() {
                        let result = remote_params_set_from_file(
                            &hash,
                            file,
                            remote_runtime.as_deref(),
                            timeout_ms,
                        )?;
                        println!("{}", result.output);
                        if result.has_errors {
                            anyhow::bail!("one or more FlowRT parameters failed to apply");
                        }
                    } else {
                        let name = name.as_deref().context(
                            "missing required argument `<name>` for `flowrt params set`",
                        )?;
                        let value = value.as_deref().context(
                            "missing required argument `<value>` for `flowrt params set`",
                        )?;
                        println!(
                            "{}",
                            remote_params_set(
                                &hash,
                                name,
                                value,
                                remote_runtime.as_deref(),
                                timeout_ms
                            )?
                        );
                    }
                } else {
                    let image = require_image_for_local(image.as_deref())?;
                    if let Some(file) = file.as_deref() {
                        let result = params_set_from_file(&image, file, socket.as_deref())?;
                        println!("{}", result.output);
                        if result.has_errors {
                            anyhow::bail!("one or more FlowRT parameters failed to apply");
                        }
                    } else {
                        let name = name.as_deref().context(
                            "missing required argument `<name>` for `flowrt params set`",
                        )?;
                        let value = value.as_deref().context(
                            "missing required argument `<value>` for `flowrt params set`",
                        )?;
                        println!("{}", params_set(&image, name, value, socket.as_deref())?);
                    }
                }
            }
        },
        Command::Op { command } => match command {
            OpCommand::List {
                image,
                socket,
                format,
            } => {
                let output = match format {
                    OperationOutputFormat::Text => {
                        operation_list(image.as_deref(), socket.as_deref())?
                    }
                    OperationOutputFormat::Json => {
                        operation_list_json(image.as_deref(), socket.as_deref())?
                    }
                };
                println!("{output}");
            }
            OpCommand::Status {
                name,
                image,
                socket,
                runtime,
                remote,
                timeout_ms,
                format,
            } => {
                let remote_runtime = control_plane_remote_runtime_arg(
                    "op status",
                    remote,
                    socket.as_deref(),
                    runtime.as_deref(),
                )?;
                if remote {
                    let image = require_image_for_remote(image.as_deref())?;
                    let hash = introspection::self_description_hash_for_image(&image)?;
                    let operation_id = name
                        .as_deref()
                        .context("`flowrt op status --remote` requires `<operation_id>`")?;
                    let output = match format {
                        OperationOutputFormat::Text => remote_operation_status(
                            &hash,
                            operation_id,
                            remote_runtime.as_deref(),
                            timeout_ms,
                        )?,
                        OperationOutputFormat::Json => remote_operation_status_json(
                            &hash,
                            operation_id,
                            remote_runtime.as_deref(),
                            timeout_ms,
                        )?,
                    };
                    println!("{output}");
                } else {
                    if image.is_some() {
                        anyhow::bail!(
                            "`--image` is only used by `flowrt op status --remote`; \
                             omit it for local live status"
                        );
                    }
                    let output = match format {
                        OperationOutputFormat::Text => {
                            operation_status_summary(socket.as_deref(), name.as_deref())?
                        }
                        OperationOutputFormat::Json => {
                            operation_status_json(socket.as_deref(), name.as_deref())?
                        }
                    };
                    println!("{output}");
                }
            }
            OpCommand::Start {
                name,
                json,
                file,
                image,
                socket,
                runtime,
                remote,
                timeout_ms,
                follow,
                format,
            } => {
                let remote_runtime = control_plane_remote_runtime_arg(
                    "op start",
                    remote,
                    socket.as_deref(),
                    runtime.as_deref(),
                )?;
                let image = if remote {
                    require_image_for_remote(image.as_deref())?
                } else {
                    require_image_for_local(image.as_deref())?
                };
                let raw_json = match (json, file) {
                    (Some(json), None) => json,
                    (None, Some(file)) => std::fs::read_to_string(&file)
                        .with_context(|| format!("failed to read `{}`", file.display()))?,
                    _ => anyhow::bail!("pass exactly one of `--json` or `--file`"),
                };
                if remote {
                    let output = match format {
                        OperationOutputFormat::Text => remote_operation_start(
                            &image,
                            &name,
                            &raw_json,
                            remote_runtime.as_deref(),
                            timeout_ms,
                        )?,
                        OperationOutputFormat::Json => remote_operation_start_json(
                            &image,
                            &name,
                            &raw_json,
                            remote_runtime.as_deref(),
                            timeout_ms,
                        )?,
                    };
                    if follow {
                        let operation_id = parse_started_operation_id_for_format(&output, format)?;
                        let follow_output = match format {
                            OperationOutputFormat::Text => operation_follow(
                                &image,
                                &operation_id,
                                None,
                                true,
                                remote_runtime.as_deref(),
                                5000,
                            )?,
                            OperationOutputFormat::Json => operation_follow_json(
                                &image,
                                &operation_id,
                                None,
                                true,
                                remote_runtime.as_deref(),
                                5000,
                            )?,
                        };
                        match format {
                            OperationOutputFormat::Text => {
                                println!("{output}");
                                println!("{follow_output}");
                            }
                            OperationOutputFormat::Json => {
                                println!(
                                    "{}",
                                    combine_operation_start_follow_json(&output, &follow_output)?
                                );
                            }
                        }
                    } else {
                        println!("{output}");
                    }
                } else {
                    let output = match format {
                        OperationOutputFormat::Text => operation_start(
                            &image,
                            &name,
                            &raw_json,
                            socket.as_deref(),
                            timeout_ms,
                        )?,
                        OperationOutputFormat::Json => operation_start_json(
                            &image,
                            &name,
                            &raw_json,
                            socket.as_deref(),
                            timeout_ms,
                        )?,
                    };
                    if follow {
                        let operation_id = parse_started_operation_id_for_format(&output, format)?;
                        let follow_output = match format {
                            OperationOutputFormat::Text => operation_follow(
                                &image,
                                &operation_id,
                                socket.as_deref(),
                                false,
                                None,
                                5000,
                            )?,
                            OperationOutputFormat::Json => operation_follow_json(
                                &image,
                                &operation_id,
                                socket.as_deref(),
                                false,
                                None,
                                5000,
                            )?,
                        };
                        match format {
                            OperationOutputFormat::Text => {
                                println!("{output}");
                                println!("{follow_output}");
                            }
                            OperationOutputFormat::Json => {
                                println!(
                                    "{}",
                                    combine_operation_start_follow_json(&output, &follow_output)?
                                );
                            }
                        }
                    } else {
                        println!("{output}");
                    }
                }
            }
            OpCommand::Cancel {
                operation_id,
                image,
                socket,
                runtime,
                remote,
                timeout_ms,
                format,
            } => {
                let remote_runtime = control_plane_remote_runtime_arg(
                    "op cancel",
                    remote,
                    socket.as_deref(),
                    runtime.as_deref(),
                )?;
                if remote {
                    let image = require_image_for_remote(image.as_deref())?;
                    let hash = introspection::self_description_hash_for_image(&image)?;
                    let output = match format {
                        OperationOutputFormat::Text => remote_operation_cancel(
                            &hash,
                            &operation_id,
                            remote_runtime.as_deref(),
                            timeout_ms,
                        )?,
                        OperationOutputFormat::Json => remote_operation_cancel_json(
                            &hash,
                            &operation_id,
                            remote_runtime.as_deref(),
                            timeout_ms,
                        )?,
                    };
                    println!("{output}");
                } else {
                    if image.is_some() {
                        anyhow::bail!(
                            "`--image` is only used by `flowrt op cancel --remote`; \
                             omit it for local live cancel"
                        );
                    }
                    let output = match format {
                        OperationOutputFormat::Text => {
                            operation_cancel(&operation_id, socket.as_deref())?
                        }
                        OperationOutputFormat::Json => {
                            operation_cancel_json(&operation_id, socket.as_deref())?
                        }
                    };
                    println!("{output}");
                }
            }
            OpCommand::Result {
                operation_id,
                image,
                socket,
                runtime,
                remote,
                timeout_ms,
                format,
            } => {
                let remote_runtime = control_plane_remote_runtime_arg(
                    "op result",
                    remote,
                    socket.as_deref(),
                    runtime.as_deref(),
                )?;
                let image = if remote {
                    require_image_for_remote(image.as_deref())?
                } else {
                    require_image_for_local(image.as_deref())?
                };
                let output = match format {
                    OperationOutputFormat::Text => operation_result(
                        &image,
                        &operation_id,
                        socket.as_deref(),
                        remote,
                        remote_runtime.as_deref(),
                        timeout_ms,
                    )?,
                    OperationOutputFormat::Json => operation_result_json(
                        &image,
                        &operation_id,
                        socket.as_deref(),
                        remote,
                        remote_runtime.as_deref(),
                        timeout_ms,
                    )?,
                };
                println!("{output}");
            }
            OpCommand::Follow {
                operation_id,
                image,
                socket,
                runtime,
                remote,
                timeout_ms,
                format,
            } => {
                let remote_runtime = control_plane_remote_runtime_arg(
                    "op follow",
                    remote,
                    socket.as_deref(),
                    runtime.as_deref(),
                )?;
                let image = if remote {
                    require_image_for_remote(image.as_deref())?
                } else {
                    require_image_for_local(image.as_deref())?
                };
                let output = match format {
                    OperationOutputFormat::Text => operation_follow(
                        &image,
                        &operation_id,
                        socket.as_deref(),
                        remote,
                        remote_runtime.as_deref(),
                        timeout_ms,
                    )?,
                    OperationOutputFormat::Json => operation_follow_json(
                        &image,
                        &operation_id,
                        socket.as_deref(),
                        remote,
                        remote_runtime.as_deref(),
                        timeout_ms,
                    )?,
                };
                println!("{output}");
            }
        },
        Command::Status { live_only, format } => {
            let output = match format {
                StatusFormat::Text => live_status_summary(live_only)?,
                StatusFormat::Json => live_status_json(live_only)?,
            };
            println!("{output}");
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
        Command::Toolchain { command } => match command {
            ToolchainCommand::Show { target } => {
                let workspace_root =
                    env::current_dir().context("failed to resolve current working directory")?;
                println!("{}", toolchain_show(&target, &workspace_root)?);
            }
            ToolchainCommand::Init {
                target,
                sdk_overlay,
                force,
            } => {
                let workspace_root =
                    env::current_dir().context("failed to resolve current working directory")?;
                println!(
                    "{}",
                    toolchain_init(&target, &sdk_overlay, force, &workspace_root)?
                );
            }
        },
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

fn parse_started_operation_id(output: &str) -> Result<&str> {
    output
        .split_whitespace()
        .find_map(|field| field.strip_prefix("operation_id="))
        .context("operation start output did not contain operation_id")
}

fn parse_started_operation_id_for_format(
    output: &str,
    format: OperationOutputFormat,
) -> Result<String> {
    match format {
        OperationOutputFormat::Text => parse_started_operation_id(output).map(str::to_string),
        OperationOutputFormat::Json => {
            let value: serde_json::Value =
                serde_json::from_str(output).context("operation start JSON output is invalid")?;
            value
                .get("operation_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .context("operation start JSON output did not contain operation_id")
        }
    }
}

fn combine_operation_start_follow_json(started: &str, follow: &str) -> Result<String> {
    let started: serde_json::Value =
        serde_json::from_str(started).context("operation start JSON output is invalid")?;
    let follow: serde_json::Value =
        serde_json::from_str(follow).context("operation follow JSON output is invalid")?;
    serde_json::to_string_pretty(&serde_json::json!({
        "response": "operation_start_follow",
        "started": started,
        "follow": follow,
    }))
    .context("序列化 Operation start/follow JSON 失败")
}

#[cfg(test)]
mod tests;
