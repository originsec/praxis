//! Startup banner for the Praxis service.

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Print the startup banner
pub fn print_banner(rabbitmq_url: &str) {
    //
    // ASCII creature - spectral entity.
    //
    let creature = [
        "     ▄▄▄███▄▄▄     ",
        "   ▄█▀▀     ▀▀█▄   ",
        "  ██  ●     ●  ██  ",
        "  ██     ▄     ██  ",
        "   ▀█▄ ▀▀▀▀▀ ▄█▀   ",
        "  ▄▄ ▀▀█████▀▀ ▄▄  ",
        " █▀▀█▄▄     ▄▄█▀▀█ ",
        " █▄▄█▀ ▀▀▀▀▀ ▀█▄▄█ ",
        "      ▀▀▀▀▀▀▀      ",
    ];

    let left_col_width = 28;
    let right_col_width = 46;

    //
    // +1 for middle separator.
    //
    let width = left_col_width + right_col_width + 1;

    //
    // Gather system information.
    //
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    //
    // Build right column content - truncate if needed.
    //
    let max_len = right_col_width - 2;
    let truncate = |s: String| -> String {
        if s.len() > max_len {
            format!("{}...", &s[..max_len - 3])
        } else {
            s
        }
    };

    let right_lines: Vec<String> = vec![
        "Server Information".to_string(),
        String::new(),
        truncate(format!("User: {}", user)),
        truncate(format!("Host: {}", hostname)),
        truncate(format!("Platform: {} ({})", os, arch)),
        String::new(),
        truncate(format!("RabbitMQ: {}", rabbitmq_url)),
        String::new(),
        String::new(),
    ];

    //
    // Helper to print a centered line.
    //
    let print_centered = |text: &str, color: &str, visible_len: usize| {
        let left_pad = (width - visible_len) / 2;
        let right_pad = width - left_pad - visible_len;
        println!(
            "\x1b[90m│\x1b[0m{}{}{}\x1b[0m{}\x1b[90m│\x1b[0m",
            " ".repeat(left_pad),
            color,
            text,
            " ".repeat(right_pad)
        );
    };

    //
    // Print top border.
    //
    println!("\x1b[90m╭{}╮\x1b[0m", "─".repeat(width));

    //
    // Empty line.
    //
    println!("\x1b[90m│\x1b[0m{}\x1b[90m│\x1b[0m", " ".repeat(width));

    //
    // Title - centered across full width.
    //
    let title = format!("Praxis C2 Server v{}", VERSION);
    print_centered(&title, "\x1b[1;36m", title.len());

    //
    // Subtitle - note: Ø is 1 display char but len() counts bytes.
    //
    let subtitle = "by [Ø] Origin";

    //
    // "by [Ø] Origin" = 13 visible characters.
    //
    let subtitle_visible_len = 13;
    print_centered(subtitle, "\x1b[35m", subtitle_visible_len);

    //
    // Empty line.
    //
    println!("\x1b[90m│\x1b[0m{}\x1b[90m│\x1b[0m", " ".repeat(width));

    //
    // Middle separator.
    //
    println!(
        "\x1b[90m├{}┬{}┤\x1b[0m",
        "─".repeat(left_col_width),
        "─".repeat(right_col_width)
    );

    //
    // Content rows.
    //
    for (i, creature_line) in creature.iter().enumerate() {
        let creature_len = creature_line.chars().count();
        let left_padding = (left_col_width - creature_len) / 2;
        let left_remainder = left_col_width - left_padding - creature_len;

        let right_text = right_lines.get(i).map(|s| s.as_str()).unwrap_or("");
        let right_visible_len = right_text.chars().count();
        let right_padding = if right_visible_len > 0 {
            right_col_width.saturating_sub(right_visible_len + 1)
        } else {
            right_col_width - 1
        };

        //
        // Build line with proper padding.
        //
        print!("\x1b[90m│\x1b[0m");
        print!("{}", " ".repeat(left_padding));
        print!("\x1b[35m{}\x1b[0m", creature_line);
        print!("{}", " ".repeat(left_remainder));
        print!("\x1b[90m│\x1b[0m ");
        if i == 0 {
            print!("\x1b[1;37m{}\x1b[0m", right_text);
        } else {
            print!("\x1b[90m{}\x1b[0m", right_text);
        }
        print!("{}", " ".repeat(right_padding));
        println!("\x1b[90m│\x1b[0m");
    }

    //
    // Bottom border.
    //
    println!(
        "\x1b[90m╰{}┴{}╯\x1b[0m",
        "─".repeat(left_col_width),
        "─".repeat(right_col_width)
    );
    println!();
}
