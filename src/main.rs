mod cli;
mod commands;
mod core;
mod error;
mod ui;
mod wizard;

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();
    if cli.no_color || std::env::var_os("NO_COLOR").is_some() {
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
    }
    let debug = cli.debug;
    let show_update_notice = !matches!(
        cli.command,
        cli::Command::SelfUpdate { .. }
            | cli::Command::Completions { .. }
            | cli::Command::Uninstall
            | cli::Command::Gateway
    );

    if show_update_notice {
        commands::self_update::spawn_background_check();
    }
    let result = commands::dispatch(cli);

    if show_update_notice {
        commands::self_update::notice_if_newer();
    }

    if let Err(e) = result {
        let (code, hint) = error::classify(&e);
        if code == error::Exit::Cancelled {
            let msg = e.to_string();
            if msg == "cancelled" || e.downcast_ref::<inquire::InquireError>().is_some() {
                eprintln!("cancelled, nothing was changed by the last step");
            } else {
                eprintln!("{} {msg}", console::style("stopped:").yellow().bold());
            }
        } else if debug {
            eprintln!("{} {e:?}", console::style("error:").red().bold());
        } else {
            eprintln!("{} {e}", console::style("error:").red().bold());
        }
        if let Some(h) = hint {
            eprintln!("{} {h}", console::style("hint:").cyan().bold());
        } else if !debug && code != error::Exit::Cancelled {
            eprintln!(
                "{} run again with --debug for details",
                console::style("hint:").cyan().bold()
            );
        }
        std::process::exit(code as i32);
    }
}
