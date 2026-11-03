# Order Up: Ring & Ship

Listens on your microphones for a double press of a reception bell and, when it hears one,
runs an AutoHotkey script. Written in Rust, so it can stay running all day at a few MB of RAM.

## Install (Windows)

Download `orderup.exe` from the [Releases](../../releases) page and run it. That's it: no
Rust, no Python, nothing to build. It starts a setup wizard that asks everything up front:

1. the AutoHotkey executable (an installed AutoHotkey v2 is detected as the default)
2. the `.ahk` script to run on a double press (Enter installs the bundled example)
3. where to install orderup (default `%LOCALAPPDATA%\Programs\orderup`)
4. whether to start it automatically when you log in
5. whether to calibrate the bell now (you ring it when prompted)
6. whether to start listening right away

It then shows a summary and changes nothing until you confirm. Run `orderup setup` again any
time to change an answer; `orderup uninstall` removes it (and optionally your calibration).

The bundled example, [`examples/ship-zed.ahk`](examples/ship-zed.ahk), types a prompt into
Zed's agent panel after a 5 s cancel countdown. Point step 2 at your own script to run
something else.

## How it works

Each calibrated microphone gets its own profile (the bell's spectrum plus per-mic
thresholds). A strike is a sudden jump in loudness that, within 150 ms, also sounds like the
bell on that mic. Two strikes within 1.2 s start the script; a 15 s cooldown stops repeats.
Several mics hearing the same strike count once.

## Commands

| | |
|---|---|
| `orderup setup` | the install wizard (also what a first launch runs) |
| `orderup uninstall` | stop starting at login and delete the installed copy |
| `orderup calibrate [--device N] [--fresh]` | record the bell and build profiles (merged with existing ones unless `--fresh`) |
| `orderup [--debug] [--dry] [--device N] [--background]` | listen; `--dry` launches nothing, `--background` detaches from the console |
| `orderup --list-devices` | show the input devices |
| `--config PATH`, `--settings PATH` | use other profile / settings files |

Data lives in `%APPDATA%\orderup\`: `profiles.json` (calibration), `settings.json` (paths) and
the example script if you chose it. A profile is tied to the device's sample rate, so
recalibrate if you change it. Start at login is a per-user entry under
`HKCU\...\CurrentVersion\Run` (visible in Task Manager > Startup apps).

## Building from source

Only needed to hack on it. Requires Rust and the MSVC Build Tools (`mise install` gets Rust).

```powershell
cargo build --release     # -> target\release\orderup.exe, the same self-contained exe
cargo test --release
```

The detection thresholds (`RISE`, `CONFIRM`, `DOUBLE_WINDOW`, `COOLDOWN`, ...) are constants at
the top of `src/listen.rs`. Pushing a tag like `v0.1.0` runs `.github/workflows/release.yml`,
which builds the exe and attaches it to a GitHub release.
