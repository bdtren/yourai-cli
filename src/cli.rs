use std::path::PathBuf;

use clap::{ArgAction, Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "yourai",
    version,
    about = "Build and run your own local AI",
    long_about = "Build and run your own local AI.\n\n\
        Typical flow:\n  yourai doctor      check this computer\n  yourai init        create a project with a guided wizard\n  \
        cd my-ai && yourai build && yourai start\n  yourai chat        talk to it in the terminal",
    disable_version_flag = true,
    propagate_version = true
)]
pub struct Cli {
    /// Print version
    #[arg(short = 'v', short_alias = 'V', long = "version", action = ArgAction::Version)]
    _version: (),

    /// Show full error details
    #[arg(long, global = true)]
    pub debug: bool,

    /// Project folder (defaults to the nearest folder containing yourai.toml)
    #[arg(long, global = true, value_name = "PATH", env = "YOURAI_PROJECT")]
    pub project: Option<PathBuf>,

    /// Disable colored output (also honors NO_COLOR)
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Also keep a copy in the shared ~/.yourai cache (and prefer copying from it)
    #[arg(short = 's', long = "save", global = true)]
    pub save: bool,

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
        after_help = "Files are installed under yourai_module/ (gitignored). yourai.lock records them.\n\n\
        Flag combos:\n  yourai build          use ~/.yourai cache if present, else download into yourai_module/\n  \
        yourai build -s       same, and also write new downloads into ~/.yourai\n  \
        yourai build -f       always download online into yourai_module/\n  \
        yourai build -f -s    download online and update the saved ~/.yourai copy\n\n\
        Examples:\n  yourai build\n  yourai build --accept-license\n  yourai build -s\n  yourai build -f -s"
    )]
    Build {
        /// Accept the model license without asking
        #[arg(long)]
        accept_license: bool,
        /// Download from the network even if a shared-cache copy exists
        #[arg(short = 'f', long = "force")]
        force: bool,
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
        /// Download from the network even if a shared-cache copy exists
        #[arg(short = 'f', long = "force")]
        force: bool,
    },

    /// Refresh the model catalog and update the engine or model
    #[command(
        after_help = "Without flags, only the catalog is refreshed and available updates are listed.\n\
        Use -s/--save and -f/--force the same way as `yourai build`.\n\n\
        Examples:\n  yourai update\n  yourai update --engine\n  yourai update --model\n  yourai update --model -f -s"
    )]
    Update {
        /// Install the newer engine version for this project
        #[arg(long)]
        engine: bool,
        /// Download the newer model file for this project
        #[arg(long)]
        model: bool,
        /// Download from the network even if a shared-cache copy exists
        #[arg(short = 'f', long = "force")]
        force: bool,
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

    /// Create an image from a description (needs [image] enabled)
    #[command(
        after_help = "The image is saved in outputs/images/.\n\nExamples:\n  yourai image \"a red fox in the snow\"\n  yourai image \"a logo for a bakery\" --size 768x768 --n 2"
    )]
    Image {
        prompt: String,
        /// Width x height, for example 768x512 (default: the model's size)
        #[arg(long)]
        size: Option<String>,
        /// Number of images (1-4)
        #[arg(long, default_value_t = 1)]
        n: u32,
        /// Sampling steps (default: the model's)
        #[arg(long)]
        steps: Option<u32>,
        /// Seed for repeatable results
        #[arg(long)]
        seed: Option<i64>,
        /// Things to avoid in the image
        #[arg(long)]
        negative: Option<String>,
    },

    /// Create a short video from a description (needs [video] enabled)
    #[command(
        after_help = "The video is saved in outputs/videos/. A clip takes minutes, even on a fast GPU.\n\nExamples:\n  yourai video \"waves rolling onto a beach at sunset\"\n  yourai video \"a cat walking\" --frames 49 --size 832x480"
    )]
    Video {
        prompt: String,
        /// Width x height, for example 832x480
        #[arg(long)]
        size: Option<String>,
        /// Number of frames (rounded to 4n+1)
        #[arg(long)]
        frames: Option<u32>,
        /// Frames per second
        #[arg(long)]
        fps: Option<u32>,
        /// Sampling steps
        #[arg(long)]
        steps: Option<u32>,
        /// Seed for repeatable results
        #[arg(long)]
        seed: Option<i64>,
    },

    /// Read text aloud and save it as a WAV file (needs [text_to_speech] enabled)
    #[command(
        after_help = "The audio is saved in outputs/audio/.\n\nExamples:\n  yourai speak \"Hello, world\"\n  yourai speak \"Bonjour\" --language fr --out hello.wav"
    )]
    Speak {
        text: String,
        /// Language code, for example en, fr, zh (default: text_to_speech.language)
        #[arg(long)]
        language: Option<String>,
        /// Also copy the WAV file here
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// Turn a recording into text (needs [speech_to_text] enabled)
    #[command(after_help = "Examples:\n  yourai transcribe meeting.wav\n  yourai transcribe note.mp3 --out note.txt")]
    Transcribe {
        file: PathBuf,
        /// Language code, or auto (default: speech_to_text.language)
        #[arg(long)]
        language: Option<String>,
        /// Write the text to this file instead of printing it
        #[arg(long)]
        out: Option<PathBuf>,
    },

    #[command(name = "__gateway", hide = true)]
    Gateway,
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
    /// Extra capabilities, comma separated: agent,vision,image,stt,tts,video (or none)
    #[arg(long)]
    pub capabilities: Option<String>,
    /// Image model id, for example sd-turbo
    #[arg(long)]
    pub image_model: Option<String>,
    /// Video model id, for example wan2.1-t2v-1.3b
    #[arg(long)]
    pub video_model: Option<String>,
    /// Speech-to-text model id, for example whisper-base
    #[arg(long)]
    pub stt_model: Option<String>,
    /// Text-to-speech model id, for example qwen3-tts-1.7b
    #[arg(long)]
    pub tts_model: Option<String>,
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
