//! orderup: listen for a double press of the reception bell and launch an AutoHotkey script.
//!
//!     orderup setup                              # install wizard (also runs on a first launch)
//!     orderup uninstall                          # undo setup
//!     orderup calibrate [--device N] [--fresh]   # (re)build per-mic profiles (ring the bell)
//!     orderup --debug --dry                      # tune thresholds, launches nothing
//!     orderup                                    # live
//!     orderup --background                       # live, detached from its console window
//! Other flags: --list-devices, --device N (one device only),
//!              --config PATH (profiles file), --settings PATH (settings file)

mod audio;
mod calibrate;
mod dsp;
mod listen;
mod profile;
mod prompt;
mod settings;
mod setup;
mod startup;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn FreeConsole() -> i32;
    fn SetStdHandle(std_handle: u32, handle: *mut std::ffi::c_void) -> i32;
}

const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
const STD_ERROR_HANDLE: u32 = -12i32 as u32;
const MAX_LOG_BYTES: u64 = 512 * 1024;

/// With no console, println!/eprintln! would vanish. Point them at %APPDATA%\orderup\orderup.log
/// instead so a background run can be diagnosed. Best effort: on any failure, stay silent.
fn redirect_output_to_log() {
    use std::os::windows::io::IntoRawHandle;
    let path = profile::app_dir().join("orderup.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
        let _ = std::fs::remove_file(&path);
    }
    let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    let handle = file.into_raw_handle(); // intentionally never closed: it lives as long as we do
    // SAFETY: `handle` is a valid, open file handle that we leak on purpose.
    unsafe {
        SetStdHandle(STD_OUTPUT_HANDLE, handle);
        SetStdHandle(STD_ERROR_HANDLE, handle);
    }
}

/// Parses `--device N`.
pub fn device_arg(args: &[String]) -> Result<Option<usize>, String> {
    match args.iter().position(|a| a == "--device") {
        None => Ok(None),
        Some(i) => args
            .get(i + 1)
            .and_then(|v| v.parse().ok())
            .map(Some)
            .ok_or_else(|| "--device needs a number (see --list-devices)".to_string()),
    }
}

fn list_devices() -> Result<(), String> {
    for input in audio::inputs(None).map_err(|e| e.to_string())? {
        println!("[{}] {} ({} Hz)", input.index, input.name, input.sr);
    }
    Ok(())
}

fn run_listener(args: &[String]) -> Result<(), String> {
    let background = args.iter().any(|a| a == "--background");
    // --dry launches nothing, so it needs no settings.
    let settings = if args.iter().any(|a| a == "--dry") {
        None
    } else {
        match settings::load(&settings::path(args))? {
            Some(s) => {
                settings::warn_if_missing(&s);
                Some(s)
            }
            // Never set up: walk through the wizard (can't do that with no console).
            None if !background => {
                println!("Welcome to Order Up! Let's get you set up.\n");
                return setup::run(args);
            }
            None => return Err("not set up: run `orderup setup`".into()),
        }
    };
    if background {
        // SAFETY: plain Win32 call with no arguments; detaches us from the console.
        unsafe { FreeConsole() };
        redirect_output_to_log();
        let unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        println!("--- orderup started (unix time {unix}) ---");
    }
    listen::run(args, settings)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    profile::migrate_legacy_dir();
    let result = if args.iter().any(|a| a == "--list-devices") {
        list_devices()
    } else {
        match args.first().map(String::as_str) {
            Some("setup") => setup::run(&args),
            Some("uninstall") => setup::uninstall(&args),
            Some("calibrate") => calibrate::run(&args),
            _ => run_listener(&args),
        }
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
