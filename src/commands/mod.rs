pub mod build;
pub mod chat;
pub mod config;
pub mod doctor;
pub mod init;
pub mod lifecycle;
pub mod rebuild;
pub mod self_update;
pub mod uninstall;
pub mod update;

use crate::cli::{Cli, Command};
use crate::core::project::Project;
use clap::CommandFactory;

pub fn dispatch(cli: Cli) -> anyhow::Result<()> {
    let explicit = cli.project.clone();
    let project = || Project::discover(explicit.as_deref());
    match cli.command {
        Command::Doctor { fix, network } => doctor::run(fix, network, explicit.as_deref()),
        Command::Init(args) => init::run(args),
        Command::Build { accept_license } => build::run(&project()?, accept_license),
        Command::Start => lifecycle::start(&project()?),
        Command::Stop => lifecycle::stop(&project()?),
        Command::Pause => lifecycle::pause(&project()?),
        Command::Resume => lifecycle::resume(&project()?),
        Command::Status => lifecycle::status(&project()?),
        Command::Logs { follow, lines } => lifecycle::logs(&project()?, follow, lines),
        Command::Chat => chat::run(&project()?),
        Command::Config(cmd) => config::run(&project()?, cmd),
        Command::Rebuild { accept_license } => rebuild::run(&project()?, accept_license),
        Command::Update { engine, model } => update::run(explicit.as_deref(), engine, model),
        Command::SelfUpdate { check } => self_update::run(check),
        Command::Uninstall => uninstall::run(),
        Command::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "yourai", &mut std::io::stdout());
            Ok(())
        }
    }
}
