use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "yourai",
    version,
    about = "Build and run your own local AI",
    long_about = "Build and run your own local AI.\n\n\
        Typical flow:\n  yourai doctor      check this computer\n  yourai init        create a project with a guided wizard\n  \
        cd my-ai && yourai build && yourai start\n  yourai chat        talk to it in the terminal",
    propagate_version = true
)]
pub struct Cli {
    /// Show full error details
    #[arg(long, global = true)]
    pub debug: bool,

    /// Project folder (defaults to the nearest folder containing yourai.toml)
    #[arg(long, global = true, value_name = "PATH", env = "YOURAI_PROJECT")]
    pub project: Option<PathBuf>,

    /// Disable colored output (also honors NO_COLOR)
    #[arg(long, global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)] // parsed once per run
pub enum Command {
    /// Check your hardware and dependencies (read-only unless --fix)
    #[command(after_help = "Examples:\n  yourai doctor\n  yourai doctor --network\n  yourai doctor --fix")]
    Doctor {
        /// Apply safe fixes only: create folders, pick a free port
        #[arg(long)]
        fix: bool,
        /// Also test connectivity to yourai.bdtren.com and Hugging Face
        #[arg(long)]
        network: bool,
    },

    /// Create a new AI project with a guided wizard
    #[command(
        after_help = "Examples:\n  yourai init\n  yourai init ./my-ai\n  yourai init --yes --name my-ai\n  \
        yourai init --yes --model qwen2.5-7b --quant Q4_K_M --ctx 8192"
    )]
    Init(InitArgs),

    /// Download the engine and model for this project
    #[command(
        after_help = "Downloads go to ~/.yourai and are reused by every project.\n\nExamples:\n  yourai build\n  yourai build --accept-license"
    )]
    Build {
        /// Accept the model license without asking
        #[arg(long)]
        accept_license: bool,
    },

    /// Start the AI in the background
    Start,
    /// Stop the AI
    Stop,
    /// Unload the model and free RAM/VRAM (project stays configured)
    Pause,
    /// Reload the model after a pause
    Resume,
    /// Show running state, port, model and memory use
    Status,
    /// Show server logs
    Logs {
        /// Keep printing new log lines
        #[arg(long, short)]
        follow: bool,
        /// Number of lines to show
        #[arg(long, short = 'n', default_value_t = 50)]
        lines: usize,
    },
    /// Chat with your AI in the terminal
    Chat,

    /// Read or change project configuration
    #[command(subcommand)]
    Config(ConfigCmd),

    /// Apply config changes (only redoes what changed)
    Rebuild {
        /// Accept the model license without asking
        #[arg(long)]
        accept_license: bool,
    },

    /// Refresh the model catalog and update the engine or model
    #[command(
        after_help = "Without flags, only the catalog is refreshed and available updates are listed.\n\n\
        Examples:\n  yourai update\n  yourai update --engine\n  yourai update --model"
    )]
    Update {
        /// Install the newer engine version for this project
        #[arg(long)]
        engine: bool,
        /// Download the newer model file for this project
        #[arg(long)]
        model: bool,
    },

    /// Update the yourai CLI itself
    SelfUpdate {
        /// Only check, do not install
        #[arg(long)]
        check: bool,
    },

    /// Remove the yourai CLI (asks separately about models and engines)
    Uninstall,

    /// Print shell completions
    #[command(
        after_help = "Examples:\n  yourai completions zsh > ~/.zfunc/_yourai\n  yourai completions bash > ~/.local/share/bash-completion/completions/yourai"
    )]
    Completions { shell: clap_complete::Shell },
}

#[derive(Args, Debug, Default, Clone)]
pub struct InitArgs {
    /// Folder to create (defaults to ./<name>)
    pub dir: Option<PathBuf>,
    /// Accept the recommended answer for every question
    #[arg(long, short)]
    pub yes: bool,
    /// Project name
    #[arg(long)]
    pub name: Option<String>,
    /// Goal: chat | code
    #[arg(long)]
    pub goal: Option<String>,
    /// Model id from the catalog (see `yourai doctor` or the wizard)
    #[arg(long)]
    pub model: Option<String>,
    /// Quantization, for example Q4_K_M
    #[arg(long)]
    pub quant: Option<String>,
    /// Runtime: llamacpp
    #[arg(long)]
    pub runtime: Option<String>,
    /// Context length in tokens, for example 4096
    #[arg(long)]
    pub ctx: Option<String>,
    /// Interface: webui | api
    #[arg(long)]
    pub interface: Option<String>,
    /// Network access: local | lan
    #[arg(long)]
    pub access: Option<String>,
    /// Port for the local server
    #[arg(long)]
    pub port: Option<String>,
    /// GPU memory in GB, when it cannot be detected
    #[arg(long)]
    pub vram: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum ConfigCmd {
    /// Print yourai.toml
    Show,
    /// Print one value, for example: yourai config get server.port
    Get { key: String },
    /// Change one value, for example: yourai config set generation.temperature 0.5
    Set { key: String, value: String },
    /// Open yourai.toml in $EDITOR
    Edit,
    /// Check yourai.toml for errors without changing anything
    Validate,
}
