pub fn print_success(message: &str) {
    println!("OK {}", message);
}

pub fn print_error(message: &str) {
    eprintln!("ERR {}", message);
}

pub fn print_header(title: &str) {
    println!();
    println!("{}", title);
    println!("{}", "=".repeat(title.len()));
}

pub fn format_short_id(id: &str) -> String {
    common::short_id(id).to_string()
}
