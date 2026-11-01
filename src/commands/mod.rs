pub mod agent;
pub mod build;
pub mod chat;
pub mod config;
pub mod doctor;
pub mod init;
pub mod lifecycle;
pub mod media;
pub mod rebuild;
pub mod self_update;
pub mod uninstall;
pub mod update;

use crate::cli::{Cli, Command};
use crate::core::project::Project;
use crate::core::store::Opts;
use clap::CommandFactory;

pub fn dispatch(cli: Cli) -> anyhow::Result<()> {
    let explicit = cli.project.clone();
    let save = cli.save;
    let project = || Project::discover(explicit.as_deref());
    let opts = |force: bool| Opts { save, force };
    match cli.command {
        Command::Doctor { fix, network } => doctor::run(fix, network, explicit.as_deref()),
        Command::Init(args) => init::run(args),
        Command::Build { accept_license, force } => build::run(&project()?, accept_license, opts(force)),
        Command::Start => lifecycle::start(&project()?),
        Command::Stop => lifecycle::stop(&project()?),
        Command::Pause => lifecycle::pause(&project()?),
        Command::Resume => lifecycle::resume(&project()?),
        Command::Status => lifecycle::status(&project()?),
        Command::Logs { follow, lines } => lifecycle::logs(&project()?, follow, lines),
        Command::Chat => chat::run(&project()?),
        Command::Config(cmd) => config::run(&project()?, cmd),
        Command::Rebuild { accept_license, force } => rebuild::run(&project()?, accept_license, opts(force)),
        Command::Update { engine, model, force } => update::run(explicit.as_deref(), engine, model, opts(force)),
        Command::SelfUpdate { check } => self_update::run(check),
        Command::Uninstall => uninstall::run(),
        Command::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "yourai", &mut std::io::stdout());
            Ok(())
        }
        Command::Image {
            prompt,
            size,
            n,
            steps,
            seed,
            negative,
        } => media::image(&project()?, &prompt, size, n, steps, seed, negative),
        Command::Video {
            prompt,
            size,
            frames,
            fps,
            steps,
            seed,
        } => media::video(&project()?, &prompt, size, frames, fps, steps, seed),
        Command::Speak { text, language, out } => media::speak(&project()?, &text, language, out),
        Command::Transcribe { file, language, out } => media::transcribe(&project()?, &file, language, out),
        Command::Gateway => crate::core::gateway::run(&project()?),
    }
}
