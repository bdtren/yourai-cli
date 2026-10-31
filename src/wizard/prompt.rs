use console::style;
use inquire::Select;

use super::options::Question;
use crate::error::{CliError, Exit};
use crate::ui;

/// Renders the options card and asks. With `assume_yes`, returns the recommended value.
pub fn ask(q: &Question, assume_yes: bool) -> anyhow::Result<String> {
    let rec = q.recommended_opt();
    if assume_yes {
        return Ok(rec.value.clone());
    }

    println!("\n{}\n{}\n", style(&q.title).bold(), style(&q.why).dim());
    let mut table = ui::table();
    table.set_header(vec!["Option", "What it means"]);
    for o in q.available() {
        let label = if std::ptr::eq(o, rec) {
            format!("{}  {}", o.label, style("(Recommended)").green())
        } else {
            o.label.clone()
        };
        table.add_row(vec![label, o.explain.clone()]);
    }
    println!("{table}");

    let hidden: Vec<_> = q.options.iter().filter(|o| o.unavailable.is_some()).collect();
    if !hidden.is_empty() {
        println!("\n{}", style("Not available on this machine:").yellow());
        for o in hidden {
            println!("  {}: {}", o.label, o.unavailable.as_deref().unwrap_or(""));
        }
    }
    println!("\n{}", style(&q.reason).cyan());

    let avail: Vec<_> = q.available().collect();
    if avail.len() == 1 {
        println!("{} {}", style("→").green(), avail[0].label);
        return Ok(avail[0].value.clone());
    }
    let labels: Vec<String> = avail
        .iter()
        .map(|o| {
            if std::ptr::eq(*o, rec) {
                format!("{} (Recommended)", o.label)
            } else {
                o.label.clone()
            }
        })
        .collect();
    let start = avail.iter().position(|o| std::ptr::eq(*o, rec)).unwrap_or(0);
    let picked = Select::new("Choose one:", labels.clone())
        .with_starting_cursor(start)
        .prompt()?;
    let idx = labels.iter().position(|l| *l == picked).unwrap_or(start);
    Ok(avail[idx].value.clone())
}

/// Checks a value given by a command-line flag against the question's options.
pub fn check_flag(q: &Question, value: &str) -> anyhow::Result<String> {
    match q.find(value) {
        Some(o) => match &o.unavailable {
            Some(reason) => Err(CliError::new(
                Exit::Resources,
                format!("--{} {value} is not available: {reason}", q.key),
            )
            .hint(format!("the recommended value is `{}`", q.recommended_opt().value))
            .into()),
            None => Ok(o.value.clone()),
        },
        None => {
            let valid: Vec<&str> = q.available().map(|o| o.value.as_str()).collect();
            Err(CliError::new(
                Exit::InvalidConfig,
                format!("--{} {value} is not a valid choice", q.key),
            )
            .hint(format!("choose one of: {}", valid.join(", ")))
            .into())
        }
    }
}
