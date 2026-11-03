//! `orderup calibrate`: record the bell on every mic at once and build one profile per mic.
//!
//!     orderup calibrate              # all input devices; results merged into the profile file
//!     orderup calibrate --device N   # only that device
//!     orderup calibrate --fresh      # throw away previously saved profiles first
//!
//! Ring the bell ONCE each time the RING prompt appears, then let it fade.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;

use crate::audio;
use crate::dsp::{self, Analyzer, BINS, BLOCK};
use crate::profile::{self, Config, Profile};

const N_RINGS: u32 = 8; // how many single rings to ask for
const GAP: f64 = 2.0; // seconds between ring prompts (lets the bell fade)
const LEAD: f64 = 2.0; // seconds to get ready before the first prompt
const MIN_SIGNAL: f32 = 0.002; // a mic that never gets above this didn't hear the bell
const MIN_TEMPLATE_TONALITY: f64 = 0.10; // the averaged spectrum must show clear tonal peaks

pub fn run(args: &[String]) -> Result<(), String> {
    let only = crate::device_arg(args)?;
    let fresh = args.iter().any(|a| a == "--fresh");
    let path = profile::path(args);

    let mut buffers: Vec<(usize, String, u32, Arc<Mutex<Vec<Vec<f32>>>>)> = Vec::new();
    let mut streams = Vec::new();
    println!("Opening input devices:");
    for input in audio::inputs(only).map_err(|e| e.to_string())? {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let sink = buf.clone();
        match input.open(move |block| {
            if let Ok(mut b) = sink.lock() {
                b.push(block.to_vec());
            }
        }) {
            Ok(stream) => {
                println!("  recording on [{}] {} ({} Hz)", input.index, input.name, input.sr);
                streams.push(stream);
                buffers.push((input.index, input.name.clone(), input.sr, buf));
            }
            Err(e) => println!("  skipped [{}] {}: {e}", input.index, input.name),
        }
    }
    if streams.is_empty() {
        return Err("No usable input device found.".into());
    }

    println!("\nGet ready: ring the bell ONCE at each RING prompt, then let it fade.");
    println!("First prompt in {LEAD:.0} seconds...");
    sleep(Duration::from_secs_f64(LEAD));
    for i in 1..=N_RINGS {
        println!("  >>> RING {i}/{N_RINGS} <<<");
        sleep(Duration::from_secs_f64(GAP));
    }
    println!("Done ringing. Analysing...");
    drop(streams);

    let mut devices = BTreeMap::new();
    if !fresh {
        if let Some(old) = profile::load(&path).ok().flatten() {
            devices = old.devices;
        }
    }

    println!("\nResults:");
    for (index, name, sr, buf) in &buffers {
        let blocks = std::mem::take(&mut *buf.lock().unwrap_or_else(|e| e.into_inner()));
        match analyse(&blocks, *sr) {
            Ok((prof, note)) => {
                println!(
                    "  [{index}] {name}\n      OK ({note}); peaks {:?} Hz; sim>{} tonal>{} min_rms={}",
                    prof.peak_hz, prof.sim_t, prof.tonal_t, prof.min_rms
                );
                devices.insert(name.clone(), prof);
            }
            Err(why) => println!("  [{index}] {name}\n      SKIPPED: {why}"),
        }
    }
    if devices.is_empty() {
        return Err("No device heard the bell clearly. Nothing saved.".into());
    }
    profile::save(&path, &Config::new(devices.clone()))?;
    println!("\nSaved {} device profile(s) to {}:", devices.len(), path.display());
    for name in devices.keys() {
        println!("  - {name}");
    }
    println!("Now run: orderup --debug --dry");
    Ok(())
}

fn round(x: f64, places: i32) -> f32 {
    let k = 10f64.powi(places);
    ((x * k).round() / k) as f32
}

/// Linear-interpolated percentile (numpy's default).
fn percentile(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    let pos = (v.len() - 1) as f64 * p / 100.0;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    v[lo] + (v[hi] - v[lo]) * (pos - lo as f64)
}

fn analyse(blocks: &[Vec<f32>], sr: u32) -> Result<(Profile, String), String> {
    let blocks: Vec<&Vec<f32>> = blocks.iter().filter(|b| b.len() == BLOCK).collect();
    if blocks.len() < 10 {
        return Err("too little audio recorded".into());
    }

    let rms: Vec<f32> = blocks.iter().map(|b| dsp::rms(b)).collect();
    let top_level = rms.iter().copied().fold(0.0, f32::max);
    if top_level < MIN_SIGNAL {
        return Err(format!("no signal (max level {top_level:.4}): muted, blocked, or too far from the bell"));
    }

    let floor = top_level * 0.3;
    let mut an = Analyzer::new(sr);
    let cut = an.cut_bin();
    let loud: Vec<Vec<f32>> = blocks
        .iter()
        .zip(&rms)
        .filter(|(_, r)| **r > floor)
        .map(|(b, _)| an.spectrum(b, false).to_vec())
        .collect();
    if loud.len() < 5 {
        return Err("too few loud frames; ring the bell more steadily".into());
    }

    // Template: average spectrum of the loud frames, ignoring rumble/voice fundamentals
    let mut tpl = vec![0f32; BINS];
    for s in &loud {
        for (t, &x) in tpl.iter_mut().zip(s) {
            *t += x / loud.len() as f32;
        }
    }
    tpl[..cut].fill(0.0);
    let n = dsp::norm(&tpl);
    if n < 1e-9 {
        return Err("all the energy is below 500 Hz; this doesn't look like the bell".into());
    }
    tpl.iter_mut().for_each(|t| *t /= n as f32);

    // Local maxima above 20% of the strongest peak, keep the 5 biggest
    let tmax = tpl.iter().copied().fold(0.0, f32::max);
    let mut peaks: Vec<usize> = (1..BINS - 1)
        .filter(|&i| tpl[i] > tpl[i - 1] && tpl[i] > tpl[i + 1] && tpl[i] > tmax * 0.2)
        .collect();
    if peaks.is_empty() {
        return Err("no clear tonal peaks found".into());
    }
    peaks.sort_by(|&a, &b| tpl[a].total_cmp(&tpl[b]));
    let top: Vec<usize> = peaks.split_off(peaks.len().saturating_sub(5));
    let mask = dsp::peak_mask(&top);

    let tonality = dsp::masked_sum(&tpl, &mask) / dsp::sum(&tpl);
    if tonality < MIN_TEMPLATE_TONALITY {
        return Err(format!(
            "doesn't hear a clear bell tone (tonality {tonality:.2}); move it closer or ring louder"
        ));
    }

    // How the real strikes score on this mic -> thresholds for this mic
    let (mut sims, mut tonals) = (Vec::new(), Vec::new());
    for s in &loud {
        let mut s = s.clone();
        s[..cut].fill(0.0);
        sims.push(dsp::similarity(&s, &tpl));
        tonals.push(dsp::tonality(&s, &mask));
    }
    let sim_p10 = percentile(&mut sims, 10.0);
    let tonal_p10 = percentile(&mut tonals, 10.0);

    let mut peak_hz: Vec<u32> = top
        .iter()
        .map(|&k| (k as f64 * f64::from(sr) / BLOCK as f64).round() as u32)
        .collect();
    peak_hz.sort_unstable();
    let mut top_sorted = top;
    top_sorted.sort_unstable();

    let profile = Profile {
        sr,
        tpl,
        peaks: top_sorted,
        peak_hz,
        sim_t: round((sim_p10 * 0.9).max(0.5), 2),
        tonal_t: round((tonal_p10 * 0.8).max(0.2), 2),
        min_rms: round((f64::from(floor) * 0.2).max(0.001), 4),
        max_level: round(f64::from(top_level), 4),
    };
    Ok((profile, format!("{} loud frames", loud.len())))
}
