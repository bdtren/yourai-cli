use console::style;
use std::io::IsTerminal;

pub fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

pub fn heading(text: &str) {
    println!("\n{}", style(text).bold());
}

pub fn ok(text: impl AsRef<str>) {
    println!("{} {}", style("✔").green(), text.as_ref());
}

pub fn warn(text: impl AsRef<str>) {
    println!("{} {}", style("!").yellow().bold(), text.as_ref());
}

pub fn info(text: impl AsRef<str>) {
    println!("{} {}", style("•").cyan(), text.as_ref());
}

pub fn next(text: impl AsRef<str>) {
    println!("\n{} {}", style("Next:").bold(), text.as_ref());
}

pub fn table() -> comfy_table::Table {
    let mut t = comfy_table::Table::new();
    t.load_style(comfy_table::presets::UTF8_FULL_CONDENSED);
    let wide = console::Term::stdout()
        .size_checked()
        .is_some_and(|(_, cols)| cols >= 60);
    t.set_content_arrangement(if wide {
        comfy_table::ContentArrangement::Dynamic
    } else {
        comfy_table::ContentArrangement::Disabled
    });
    t
}

/// Yes/no question. Non-interactive sessions get `default_when_noninteractive`.
pub fn confirm(question: &str, default: bool, default_when_noninteractive: bool) -> anyhow::Result<bool> {
    if !interactive() {
        return Ok(default_when_noninteractive);
    }
    Ok(inquire::Confirm::new(question).with_default(default).prompt()?)
}

pub fn spinner(msg: impl Into<String>) -> indicatif::ProgressBar {
    let pb = indicatif::ProgressBar::new_spinner();
    if !interactive() {
        pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        println!("{}", msg.into());
        return pb;
    }
    pb.set_message(msg.into());
    pb.enable_steady_tick(std::time::Duration::from_millis(120));
    pb
}

pub fn gb(v: f64) -> String {
    if v < 1.0 {
        format!("{:.0} MB", v * 1000.0)
    } else {
        format!("{v:.1} GB")
    }
}
