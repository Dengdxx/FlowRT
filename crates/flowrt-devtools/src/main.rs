use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};

mod release_gate;

use release_gate::checks::check_registry_for_version;
use release_gate::registry::{REGISTRY_RELATIVE_PATH, ReleaseGateRegistry};

#[derive(Debug, Parser)]
#[command(name = "flowrt-devtools")]
#[command(version)]
#[command(about = "FlowRT 仓库维护工具")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 查询和校验发布门禁事实源。
    #[command(name = "release-gate")]
    ReleaseGate(ReleaseGateArgs),
}

#[derive(Debug, Args)]
struct ReleaseGateArgs {
    #[command(subcommand)]
    command: ReleaseGateCommand,
}

#[derive(Debug, Subcommand)]
enum ReleaseGateCommand {
    /// 输出指定版本的 focused smoke 脚本路径。
    #[command(name = "focused-smoke")]
    FocusedSmoke { version: String },

    /// 校验指定版本的 release gate registry 条目。
    #[command(name = "check-registry")]
    CheckRegistry { version: String },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("错误: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::ReleaseGate(args) => run_release_gate(args.command),
    }
}

fn run_release_gate(command: ReleaseGateCommand) -> Result<()> {
    let repo_root = find_repo_root()?;
    let registry_path = repo_root.join(REGISTRY_RELATIVE_PATH);
    let registry = ReleaseGateRegistry::load_from_path(&registry_path)?;

    match command {
        ReleaseGateCommand::FocusedSmoke { version } => {
            let gate = registry.checked_focused_smoke(&repo_root, &version)?;
            println!("{}", gate.script().display());
        }
        ReleaseGateCommand::CheckRegistry { version } => {
            check_registry_for_version(&registry, &repo_root, &version)?;
            println!(
                "release gate registry 校验通过: v{}",
                version.strip_prefix('v').unwrap_or(&version)
            );
        }
    }

    Ok(())
}

fn find_repo_root() -> Result<PathBuf> {
    let mut dir = env::current_dir().context("无法读取当前工作目录")?;
    loop {
        if dir.join(REGISTRY_RELATIVE_PATH).is_file() {
            return Ok(dir);
        }
        if !dir.pop() {
            bail!("无法找到 {REGISTRY_RELATIVE_PATH}；请在 FlowRT 仓库内运行 flowrt-devtools");
        }
    }
}
