//! `orderup setup` (install wizard) and `orderup uninstall`.
//!
//! The wizard asks every question first, shows a summary, and only then changes anything.
//! The program is one self-contained .exe, so "installing" means copying it somewhere
//! permanent, saving the settings, optionally registering it to start at login, and
//! optionally calibrating the bell.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::settings::{self, Settings};
use crate::{calibrate, profile, prompt, startup};

const DETACHED_PROCESS: u32 = 0x0000_0008;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const EXAMPLE_SCRIPT: &[u8] = include_bytes!("../examples/ship-zed.ahk");

struct Plan {
    ahk_exe: String,
    /// None = write out the bundled example script.
    script: Option<String>,
    install_dir: PathBuf,
    at_login: bool,
    calibrate: bool,
    start_now: bool,
}

pub fn run(args: &[String]) -> Result<(), String> {
    let result = wizard(args);
    if let Err(e) = &result {
        eprintln!("\nSetup stopped: {e}");
    }
    prompt::pause();
    result
}

fn wizard(args: &[String]) -> Result<(), String> {
    let settings_path = settings::path(args);
    let profile_path = profile::path(args);
    let old = settings::load(&settings_path).ok().flatten();
    let have_profiles = profile::load(&profile_path).ok().flatten().is_some();

    println!("orderup setup");
    println!("=============");
    println!("I'll ask everything first, then show a summary before changing anything.\n");

    // ---- 1) ask everything ----------------------------------------------------------
    let exe_default = old.as_ref().map(|s| s.ahk_exe.clone()).filter(|s| !s.is_empty()).or_else(settings::detect_ahk);
    let ahk_exe = prompt::file("1. Path to the AutoHotkey executable", exe_default.as_deref(), false)?
        .expect("empty not allowed");

    let script_default = old.as_ref().map(|s| s.ahk_script.clone()).filter(|s| !s.is_empty());
    let script = prompt::file(
        "2. Path to the .ahk script to run on a double press (Enter = install the bundled example)",
        script_default.as_deref(),
        true,
    )?;

    let dir_default = old
        .as_ref()
        .and_then(|s| Path::new(&s.installed_exe).parent().map(|p| p.to_string_lossy().into_owned()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(default_install_dir);
    let install_dir = PathBuf::from(prompt::text("3. Install orderup to", &dir_default)?);

    let at_login = prompt::yes_no("4. Start orderup automatically when you log in?", true)?;
    let calibrate = prompt::yes_no(
        "5. Calibrate the bell now? (you'll ring it 8 times when prompted)",
        !have_profiles,
    )?;
    let start_now = if calibrate || have_profiles {
        prompt::yes_no("6. Start listening right now?", true)?
    } else {
        println!("6. (Not offering to start it: the bell isn't calibrated yet.)");
        false
    };
    let plan = Plan { ahk_exe, script, install_dir, at_login, calibrate, start_now };

    // ---- 2) summary + confirm -------------------------------------------------------
    println!("\nSummary");
    println!("  AutoHotkey:      {}", plan.ahk_exe);
    match &plan.script {
        Some(p) => println!("  Script:          {p}"),
        None => println!("  Script:          bundled example -> {}", example_path().display()),
    }
    println!("  Install to:      {}", plan.install_dir.join("orderup.exe").display());
    println!("  Start at login:  {}", yn(plan.at_login));
    println!("  Calibrate now:   {}", yn(plan.calibrate));
    println!("  Start now:       {}", yn(plan.start_now));
    println!("  Settings:        {}", settings_path.display());
    if !prompt::yes_no("\nProceed?", true)? {
        println!("Nothing was changed.");
        return Ok(());
    }

    // ---- 3) do it -------------------------------------------------------------------
    println!();
    let script_path = match &plan.script {
        Some(p) => p.clone(),
        None => {
            let p = example_path();
            if let Some(dir) = p.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
            }
            std::fs::write(&p, EXAMPLE_SCRIPT).map_err(|e| format!("can't write {}: {e}", p.display()))?;
            println!("Wrote the example script to {}", p.display());
            p.to_string_lossy().into_owned()
        }
    };

    let installed = install_exe(&plan.install_dir)?;
    settings::save(
        &settings_path,
        &Settings {
            ahk_exe: plan.ahk_exe.clone(),
            ahk_script: script_path,
            installed_exe: installed.to_string_lossy().into_owned(),
        },
    )?;
    println!("Saved settings to {}", settings_path.display());

    if plan.at_login {
        startup::enable(&installed)?;
        println!("Registered to start at login.");
    } else {
        startup::disable()?;
    }

    let mut calibrated = have_profiles;
    if plan.calibrate {
        println!();
        match calibrate::run(args) {
            Ok(()) => calibrated = true,
            Err(e) => println!("\nCalibration didn't finish: {e}\nYou can retry with: orderup calibrate"),
        }
    }

    if plan.start_now && calibrated {
        Command::new(&installed)
            .arg("--background")
            .creation_flags(DETACHED_PROCESS)
            .spawn()
            .map_err(|e| format!("couldn't start {}: {e}", installed.display()))?;
        println!("\norderup is now running in the background.");
    }

    println!("\nDone. To change any of this, run setup again; to remove it, run: orderup uninstall");
    Ok(())
}

fn yn(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn default_install_dir() -> String {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Programs").join("orderup").to_string_lossy().into_owned()
}

fn example_path() -> PathBuf {
    profile::app_dir().join("ship-zed.ahk")
}

/// Copies this program into `dir` (unless it's already running from there).
fn install_exe(dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    let dest = dir.join("orderup.exe");
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let same = match (me.canonicalize(), dest.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if same {
        println!("Already running from {}", dest.display());
    } else {
        std::fs::copy(&me, &dest).map_err(|e| {
            format!(
                "can't copy to {}: {e}\n(If orderup is running, stop it in Task Manager and run setup again.)",
                dest.display()
            )
        })?;
        println!("Installed {}", dest.display());
    }
    Ok(dest)
}

pub fn uninstall(args: &[String]) -> Result<(), String> {
    let settings_path = settings::path(args);
    let profile_path = profile::path(args);
    let old = settings::load(&settings_path).ok().flatten();
    let installed = old.as_ref().map(|s| PathBuf::from(&s.installed_exe)).filter(|p| p.is_file());

    println!("orderup uninstall");
    println!("=================");
    println!("This will:");
    println!("  - remove it from starting at login{}", if startup::is_enabled() { "" } else { " (it isn't registered)" });
    match &installed {
        Some(p) => println!("  - delete {}", p.display()),
        None => println!("  - (no installed copy found to delete)"),
    }
    let purge = prompt::yes_no("Also delete your calibration and settings?", false)?;
    if !prompt::yes_no("Proceed?", true)? {
        println!("Nothing was changed.");
        return Ok(());
    }

    startup::disable()?;
    println!("Removed from login startup.");

    if let Some(exe) = installed {
        remove_exe(&exe)?;
    }
    if purge {
        for f in [&settings_path, &profile_path, &example_path()] {
            let _ = std::fs::remove_file(f);
        }
        // only succeeds if now empty, so it can never delete anything we didn't create
        let _ = std::fs::remove_dir(profile::app_dir());
        println!("Deleted calibration and settings.");
    } else {
        println!("Kept your calibration and settings in {}", profile::app_dir().display());
    }
    Ok(())
}

fn remove_exe(exe: &Path) -> Result<(), String> {
    let dir = exe.parent().unwrap_or(Path::new("."));
    let running_this = std::env::current_exe().ok().and_then(|m| m.canonicalize().ok())
        == exe.canonicalize().ok();
    if running_this {
        // Windows won't delete a running exe: let a detached cmd do it a moment after we exit.
        let script = format!(
            r#""ping -n 3 127.0.0.1 >nul & del /f /q "{}" & rmdir "{}"""#,
            exe.display(),
            dir.display()
        );
        Command::new("cmd")
            .arg("/C")
            .raw_arg(script)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("couldn't schedule deleting {}: {e}", exe.display()))?;
        println!("Deleting {} once this window closes.", exe.display());
    } else {
        std::fs::remove_file(exe).map_err(|e| {
            format!("can't delete {}: {e}\n(If orderup is running, stop it in Task Manager first.)", exe.display())
        })?;
        let _ = std::fs::remove_dir(dir); // only if empty
        println!("Deleted {}", exe.display());
    }
    Ok(())
}
