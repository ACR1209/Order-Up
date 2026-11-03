//! Tiny console prompts for the setup wizard.

use std::io::{self, BufRead, Write};
use std::path::Path;

fn read(label: &str, shown_default: Option<&str>) -> Result<String, String> {
    match shown_default {
        Some(d) => print!("{label} [{d}]: "),
        None => print!("{label}: "),
    }
    io::stdout().flush().ok();
    let mut line = String::new();
    let n = io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("setup needs a terminal to ask its questions (no input available)".into());
    }
    // tolerate pasted/dragged paths that come wrapped in quotes
    Ok(line.trim().trim_matches('"').trim().to_string())
}

/// Free text; Enter takes the default.
pub fn text(label: &str, default: &str) -> Result<String, String> {
    let a = read(label, Some(default))?;
    Ok(if a.is_empty() { default.to_string() } else { a })
}

/// An existing file, asked until valid. Enter takes the default; with no default and
/// `empty_ok`, Enter returns None.
pub fn file(label: &str, default: Option<&str>, empty_ok: bool) -> Result<Option<String>, String> {
    loop {
        let a = read(label, default)?;
        let value = if a.is_empty() { default.unwrap_or("").to_string() } else { a };
        if value.is_empty() && empty_ok {
            return Ok(None);
        }
        if Path::new(&value).is_file() {
            return Ok(Some(value));
        }
        println!("  not an existing file: {}", if value.is_empty() { "(empty)" } else { &value });
    }
}

pub fn yes_no(label: &str, default: bool) -> Result<bool, String> {
    loop {
        let hint = if default { "Y/n" } else { "y/N" };
        match read(label, Some(hint))?.to_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("  please answer y or n"),
        }
    }
}

/// Used at the end so a double-clicked console window doesn't vanish before it's read.
pub fn pause() {
    print!("\nPress Enter to close...");
    io::stdout().flush().ok();
    let _ = io::stdin().lock().read_line(&mut String::new());
}
