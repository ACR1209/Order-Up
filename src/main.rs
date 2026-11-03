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
                println!("orderup isn't set up yet, so here is the setup wizard.\n");
                return setup::run(args);
            }
            None => return Err("not set up: run `orderup setup`".into()),
        }
    };
    if background {
        // SAFETY: plain Win32 call with no arguments; detaches us from the console.
        unsafe { FreeConsole() };
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
