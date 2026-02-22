use anyhow::{anyhow, Result};
use std::collections::HashMap;

pub fn encode_text(input: &str, encoding: &str) -> Result<String> {
    match encoding {
        "braille_us_type2" => Ok(encode_braille_us_type2(input)),
        "unicode_tags" => Ok(encode_unicode_tags(input)),
        "fullwidth" => Ok(encode_fullwidth(input)),
        "morse" => Ok(encode_morse(input)),
        "rot13" => Ok(encode_rot13(input)),
        "base64" => Ok(encode_base64(input)),
        "hex" => Ok(encode_hex(input)),
        "upside_down" => Ok(encode_upside_down(input)),
        _ => Err(anyhow!("Unsupported encoding '{}'", encoding)),
    }
}

//
// Unicode Tags (ASCII Smuggling) — maps ASCII to the invisible Unicode Tags
// block (U+E0000). Each ASCII byte 0x20..0x7E becomes U+E0020..U+E007E.
// These characters are invisible in most renderers but interpreted by LLM
// tokenizers.
//

fn encode_unicode_tags(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            let cp = c as u32;
            if (0x20..=0x7E).contains(&cp) {
                char::from_u32(cp + 0xE0000).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

//
// Fullwidth — maps ASCII 0x21..0x7E to Unicode fullwidth forms (U+FF01..U+FF5E).
// Space (0x20) maps to ideographic space (U+3000). Visually distinct but
// semantically equivalent in many contexts.
//

fn encode_fullwidth(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            let cp = c as u32;
            if (0x21..=0x7E).contains(&cp) {
                char::from_u32(cp + 0xFEE0).unwrap_or(c)
            } else if cp == 0x20 {
                '\u{3000}'
            } else {
                c
            }
        })
        .collect()
}

//
// Morse code — standard ITU morse representation.
//

fn encode_morse(input: &str) -> String {
    input
        .chars()
        .map(|c| match c.to_ascii_uppercase() {
            'A' => ".-",
            'B' => "-...",
            'C' => "-.-.",
            'D' => "-..",
            'E' => ".",
            'F' => "..-.",
            'G' => "--.",
            'H' => "....",
            'I' => "..",
            'J' => ".---",
            'K' => "-.-",
            'L' => ".-..",
            'M' => "--",
            'N' => "-.",
            'O' => "---",
            'P' => ".--.",
            'Q' => "--.-",
            'R' => ".-.",
            'S' => "...",
            'T' => "-",
            'U' => "..-",
            'V' => "...-",
            'W' => ".--",
            'X' => "-..-",
            'Y' => "-.--",
            'Z' => "--..",
            '0' => "-----",
            '1' => ".----",
            '2' => "..---",
            '3' => "...--",
            '4' => "....-",
            '5' => ".....",
            '6' => "-....",
            '7' => "--...",
            '8' => "---..",
            '9' => "----.",
            ' ' => "/",
            _ => return c.to_string(),
        }.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

//
// ROT13 — simple letter rotation cipher. Rotates a-z/A-Z by 13 positions.
//

fn encode_rot13(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            'a'..='m' | 'A'..='M' => char::from(c as u8 + 13),
            'n'..='z' | 'N'..='Z' => char::from(c as u8 - 13),
            _ => c,
        })
        .collect()
}

//
// Base64.
//

fn encode_base64(input: &str) -> String {
    use base64::{Engine, engine::general_purpose::STANDARD};
    STANDARD.encode(input.as_bytes())
}

//
// Hex — each byte as two hex digits separated by spaces.
//

fn encode_hex(input: &str) -> String {
    input
        .as_bytes()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(" ")
}

//
// Upside-down — flips text using Unicode mathematical/symbol characters that
// visually resemble inverted Latin letters, then reverses the string.
//

fn encode_upside_down(input: &str) -> String {
    let flipped: String = input
        .chars()
        .map(|c| match c {
            'a' => '\u{0250}', 'b' => 'q', 'c' => '\u{0254}', 'd' => 'p',
            'e' => '\u{01DD}', 'f' => '\u{025F}', 'g' => '\u{0183}',
            'h' => '\u{0265}', 'i' => '\u{0131}', 'j' => '\u{027E}',
            'k' => '\u{029E}', 'l' => 'l', 'm' => '\u{026F}',
            'n' => 'u', 'o' => 'o', 'p' => 'd', 'q' => 'b',
            'r' => '\u{0279}', 's' => 's', 't' => '\u{0287}',
            'u' => 'n', 'v' => '\u{028C}', 'w' => '\u{028D}',
            'x' => 'x', 'y' => '\u{028E}', 'z' => 'z',
            'A' => '\u{2200}', 'B' => '\u{10412}', 'C' => '\u{0186}',
            'D' => '\u{15E1}', 'E' => '\u{018E}', 'F' => '\u{2132}',
            'G' => '\u{2141}', 'H' => 'H', 'I' => 'I',
            'J' => '\u{017F}', 'K' => '\u{029E}', 'L' => '\u{2142}',
            'M' => 'W', 'N' => 'N', 'O' => 'O', 'P' => '\u{0500}',
            'Q' => '\u{038C}', 'R' => '\u{1D1A}', 'S' => 'S',
            'T' => '\u{2534}', 'U' => '\u{2229}', 'V' => '\u{039B}',
            'W' => 'M', 'X' => 'X', 'Y' => '\u{2144}', 'Z' => 'Z',
            '1' => '\u{21C2}', '2' => '\u{218A}', '3' => '\u{218B}',
            '4' => '\u{3123}', '5' => '\u{078E}', '6' => '9',
            '7' => '\u{3125}', '8' => '8', '9' => '6', '0' => '0',
            '.' => '\u{02D9}', ',' => '\u{02BB}', '?' => '\u{00BF}',
            '!' => '\u{00A1}', '\'' => ',', '"' => '\u{201E}',
            '(' => ')', ')' => '(', '[' => ']', ']' => '[',
            '{' => '}', '}' => '{', '<' => '>', '>' => '<',
            '&' => '\u{214B}', '_' => '\u{203E}',
            _ => c,
        })
        .collect();
    flipped.chars().rev().collect()
}

fn encode_braille_us_type2(input: &str) -> String {
    let contractions: HashMap<&str, &str> = HashMap::from([
        ("and", "⠯"),
        ("for", "⠿"),
        ("of", "⠷"),
        ("the", "⠮"),
        ("with", "⠾"),
    ]);

    let mut out = String::new();
    for token in input.split_inclusive(char::is_whitespace) {
        let (word, trailing_ws) = match token.trim_end_matches(char::is_whitespace) {
            "" => ("", token),
            w => (w, &token[w.len()..]),
        };

        if word.is_empty() {
            out.push_str(token);
            continue;
        }

        let lower = word.to_lowercase();
        if let Some(c) = contractions.get(lower.as_str()) {
            out.push_str(c);
        } else {
            out.push_str(&lower.chars().map(letter_to_braille).collect::<String>());
        }
        out.push_str(trailing_ws);
    }
    out
}

fn letter_to_braille(c: char) -> char {
    match c {
        'a' => '⠁', 'b' => '⠃', 'c' => '⠉', 'd' => '⠙', 'e' => '⠑',
        'f' => '⠋', 'g' => '⠛', 'h' => '⠓', 'i' => '⠊', 'j' => '⠚',
        'k' => '⠅', 'l' => '⠇', 'm' => '⠍', 'n' => '⠝', 'o' => '⠕',
        'p' => '⠏', 'q' => '⠟', 'r' => '⠗', 's' => '⠎', 't' => '⠞',
        'u' => '⠥', 'v' => '⠧', 'w' => '⠺', 'x' => '⠭', 'y' => '⠽',
        'z' => '⠵',
        '0' => '⠚', '1' => '⠁', '2' => '⠃', '3' => '⠉', '4' => '⠙',
        '5' => '⠑', '6' => '⠋', '7' => '⠛', '8' => '⠓', '9' => '⠊',
        _ => c,
    }
}
