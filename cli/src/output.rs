use colored::Colorize;
use serde::Serialize;
use std::str::FromStr;

#[derive(Clone, Debug)]
pub enum OutputFormat {
    Text,
    Json,
}

impl FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" => Ok(OutputFormat::Text),
            "json" => Ok(OutputFormat::Json),
            _ => Err(format!("Invalid output format: {}. Use 'text' or 'json'", s)),
        }
    }
}

pub fn print_json<T: Serialize>(value: &T) {
    if let Ok(json) = serde_json::to_string_pretty(value) {
        println!("{}", json);
    }
}

pub fn print_success(message: &str) {
    println!("{} {}", "✓".green(), message);
}

pub fn print_error(message: &str) {
    eprintln!("{} {}", "✗".red(), message);
}

pub fn print_header(title: &str) {
    println!("\n{}", title.bold().underline());
}

pub fn format_short_id(id: &str) -> String {
    id[..8.min(id.len())].to_string()
}

pub fn format_status(status: &str) -> String {
    match status.to_lowercase().as_str() {
        "running" | "active" => status.yellow().to_string(),
        "completed" | "done" => status.green().to_string(),
        "failed" | "error" => status.red().to_string(),
        "queued" | "pending" => status.cyan().to_string(),
        "cancelled" => status.magenta().to_string(),
        _ => status.to_string(),
    }
}
