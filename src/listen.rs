//! The always-on part: per-device detectors feeding one shared double-press state machine.

use std::process::Command;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use crate::audio;
use crate::dsp::{self, Analyzer};
use crate::profile::{self, Profile};
use crate::settings::Settings;

// ---- detection tuning -------------------------------------------------------
const RISE: f32 = 2.5; // a strike must be this many times louder than the last ~190 ms
const CONFIRM: f64 = 0.15; // seconds after an onset to see the bell's spectral signature
const MIN_GAP: f64 = 0.15; // strikes closer than this count as one (also dedupes multiple mics)
const DOUBLE_WINDOW: f64 = 1.2; // max seconds between the two presses
const COOLDOWN: f64 = 15.0; // seconds before another ship can start
const STALL_SECS: u64 = 3; // a stream that delivers no audio for this long is reopened
const RESCAN_SECS: u32 = 5; // how often to look for calibrated devices that aren't open yet
// -----------------------------------------------------------------------------

/// f32 shared between the audio thread and the main thread.
#[derive(Default)]
struct AtomicF32(AtomicU32);

impl AtomicF32 {
    fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    fn store(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }
}

#[derive(Default)]
struct Stats {
    peak: AtomicF32,     // loudest block since the last debug readout
    max_seen: AtomicF32, // loudest block ever (to spot dead/muted mics)
    last_block_ms: AtomicU64, // when the last block arrived (ms since start), to spot stalled streams
}

struct State {
    hits: Vec<f64>,
    last_hit: f64,
    last_ship: f64,
}

struct Shared {
    state: Mutex<State>,
    running: Arc<AtomicBool>,
    /// None in --dry mode: nothing is launched, so no paths are needed.
    settings: Option<Settings>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn last_hit(&self) -> f64 {
        self.lock().last_hit
    }

    fn register_hit(&self, now: f64, name: &str) {
        {
            let mut s = self.lock();
            if now - s.last_hit < MIN_GAP {
                return; // same strike heard by another mic
            }
            s.last_hit = now;
            s.hits.retain(|&h| now - h < DOUBLE_WINDOW);
            s.hits.push(now);
            println!("bell strike {}  (heard on {name})", s.hits.len());
            if s.hits.len() < 2 {
                return;
            }
            s.hits.clear();
            let remaining = COOLDOWN - (now - s.last_ship);
            if remaining > 0.0 {
                println!("double press ignored: cooldown, {remaining:.0}s left");
                return;
            }
            s.last_ship = now;
        }
        let (settings, running) = (self.settings.clone(), self.running.clone());
        thread::spawn(move || trigger(settings.as_ref(), &running));
    }
}

fn trigger(settings: Option<&Settings>, running: &AtomicBool) {
    println!(">>> DOUBLE PRESS - shipping");
    let Some(settings) = settings else { return }; // --dry
    if running.swap(true, Ordering::SeqCst) {
        println!("previous ship script is still running; skipping");
        return;
    }
    let launched = Command::new(&settings.ahk_exe).arg(&settings.ahk_script).spawn();
    match launched.and_then(|mut c| c.wait()) {
        Ok(status) => println!("ship script finished ({status})"),
        Err(e) => println!("couldn't run the ship script: {e}"),
    }
    running.store(false, Ordering::SeqCst);
}

struct Detector {
    label: String,
    index: usize,
    analyzer: Analyzer,
    prof: Profile,
    mask: Vec<bool>,
    rms_hist: [f32; 4],
    hist_pos: usize,
    pending_until: f64,
    stats: Arc<Stats>,
    shared: Arc<Shared>,
    start: Instant,
    debug: bool,
}

impl Detector {
    fn process(&mut self, block: &[f32]) {
        let now = self.start.elapsed().as_secs_f64();
        self.stats.last_block_ms.store((now * 1000.0) as u64, Ordering::Relaxed);
        let rms = dsp::rms(block);
        self.stats.peak.store(self.stats.peak.load().max(rms));
        self.stats.max_seen.store(self.stats.max_seen.load().max(rms));

        let mag = self.analyzer.spectrum(block, true);
        let sim = dsp::similarity(mag, &self.prof.tpl);
        let tonal = dsp::tonality(mag, &self.mask);

        // 1) onset: sudden jump in loudness
        let mean = self.rms_hist.iter().sum::<f32>() / self.rms_hist.len() as f32;
        let loud = rms > self.prof.min_rms;
        let onset = loud && rms > RISE * mean;
        self.rms_hist[self.hist_pos] = rms;
        self.hist_pos = (self.hist_pos + 1) % self.rms_hist.len();
        if onset && now - self.shared.last_hit() > MIN_GAP {
            self.pending_until = now + CONFIRM;
        }

        // 2) confirm: within the next few blocks it must sound like the bell on this mic
        let matched =
            loud && sim > f64::from(self.prof.sim_t) && tonal > f64::from(self.prof.tonal_t);
        if self.debug && loud {
            println!(
                "[{}] rms={rms:.3} sim={sim:.2} tonal={tonal:.2} onset={onset} match={matched}",
                self.index
            );
        }
        if now < self.pending_until && matched {
            self.pending_until = 0.0;
            self.shared.register_hit(now, &self.label);
        }
    }
}

/// `settings` is None for --dry runs, which launch nothing.
pub fn run(args: &[String], settings: Option<Settings>) -> Result<(), String> {
    let debug = args.iter().any(|a| a == "--debug");
    let dry = settings.is_none();
    let only = crate::device_arg(args)?;

    let path = profile::path(args);
    let cfg = profile::load(&path)?.ok_or_else(|| {
        format!("no usable profiles at {}. Run: orderup calibrate", path.display())
    })?;

    let shared = Arc::new(Shared {
        state: Mutex::new(State { hits: Vec::new(), last_hit: f64::NEG_INFINITY, last_ship: f64::NEG_INFINITY }),
        running: Arc::new(AtomicBool::new(false)),
        settings,
    });
    let start = Instant::now();

    // One open stream per calibrated device. A device that isn't there yet (a wireless
    // headset still connecting after boot) is picked up by the rescan below; a stream that
    // stops delivering audio is dropped and reopened.
    let mut slots: HashMap<String, Slot> = HashMap::new();
    let mut reported: HashSet<String> = HashSet::new();
    println!("Opening input devices:");
    open_missing(&cfg, only, &shared, start, debug, &mut slots, &mut reported);
    if slots.is_empty() {
        println!("No calibrated input device is available yet. Waiting for one (run orderup calibrate, or check --list-devices).");
    }

    println!("Ready (cooldown={COOLDOWN}s, dry={dry}). Ctrl+C to stop.");
    let mut seconds = 0u32;
    loop {
        thread::sleep(Duration::from_secs(1));
        seconds += 1;
        let now_ms = start.elapsed().as_millis() as u64;

        slots.retain(|name, slot| {
            let idle = now_ms.saturating_sub(slot.stats.last_block_ms.load(Ordering::Relaxed));
            if idle > STALL_SECS * 1000 {
                println!("[{name}] stopped delivering audio; reopening");
                reported.remove(name);
                false
            } else {
                true
            }
        });
        if seconds % RESCAN_SECS == 0 {
            open_missing(&cfg, only, &shared, start, debug, &mut slots, &mut reported);
        }

        if debug {
            let levels: Vec<String> = slots
                .values()
                .map(|s| format!("[{}]={:.4}", s.index, s.stats.peak.load()))
                .collect();
            println!("level (max rms last second): {}", levels.join("  "));
            slots.values().for_each(|s| s.stats.peak.store(0.0));
        }
        for (name, slot) in &mut slots {
            slot.age += 1;
            if slot.age == 5 && slot.stats.max_seen.load() == 0.0 {
                println!("note: [{}] {name} is delivering pure silence (muted or blocked); the other devices still work", slot.index);
            }
        }
    }
}

struct Slot {
    index: usize,
    stats: Arc<Stats>,
    age: u32, // seconds since opened
    _stream: cpal::Stream,
}

/// Opens every calibrated input device that isn't open yet. Reasons for skipping a device
/// are printed once, not on every rescan.
fn open_missing(
    cfg: &profile::Config,
    only: Option<usize>,
    shared: &Arc<Shared>,
    start: Instant,
    debug: bool,
    slots: &mut HashMap<String, Slot>,
    reported: &mut HashSet<String>,
) {
    const ENUM_KEY: &str = "<enumeration>";
    let inputs = match audio::inputs(only) {
        Ok(i) => i,
        Err(e) => {
            if reported.insert(ENUM_KEY.to_string()) {
                println!("  can't list input devices: {e}");
            }
            return;
        }
    };
    reported.remove(ENUM_KEY);
    for input in inputs {
        if slots.contains_key(&input.name) {
            continue;
        }
        let Some(prof) = cfg.devices.get(&input.name) else {
            if reported.insert(input.name.clone()) {
                println!("  skipped [{}] {}: no calibration profile (run orderup calibrate with it connected)",
                    input.index, input.name);
            }
            continue;
        };
        if prof.sr != input.sr {
            if reported.insert(input.name.clone()) {
                println!("  skipped [{}] {}: calibrated at {} Hz but the device now runs at {} Hz; recalibrate",
                    input.index, input.name, prof.sr, input.sr);
            }
            continue;
        }
        let stats = Arc::new(Stats::default());
        stats.last_block_ms.store(start.elapsed().as_millis() as u64, Ordering::Relaxed);
        let mut det = Detector {
            label: format!("[{}] {}", input.index, input.name),
            index: input.index,
            analyzer: Analyzer::new(input.sr),
            mask: dsp::peak_mask(&prof.peaks),
            prof: prof.clone(),
            rms_hist: [1e-6; 4],
            hist_pos: 0,
            pending_until: 0.0,
            stats: stats.clone(),
            shared: shared.clone(),
            start,
            debug,
        };
        match input.open(move |block| det.process(block)) {
            Ok(stream) => {
                println!("  listening on [{}] {}  (sim>{} tonal>{} min_rms={})", input.index,
                    input.name, prof.sim_t, prof.tonal_t, prof.min_rms);
                slots.insert(input.name.clone(), Slot { index: input.index, stats, age: 0, _stream: stream });
            }
            Err(e) => {
                if reported.insert(input.name.clone()) {
                    println!("  skipped [{}] {}: {e}", input.index, input.name);
                }
            }
        }
    }
}
