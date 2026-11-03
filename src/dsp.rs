//! Spectrum analysis shared by the listener and the calibrator.
//! Mirrors the numpy version: Hann-windowed rfft magnitude, bins below 500 Hz ignored.

use std::f32::consts::PI;
use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

pub const BLOCK: usize = 2048;
pub const BINS: usize = BLOCK / 2 + 1;
const LOW_CUT_HZ: f32 = 500.0;

pub struct Analyzer {
    r2c: Arc<dyn RealToComplex<f32>>,
    win: Vec<f32>,
    input: Vec<f32>,
    spec: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    mag: Vec<f32>,
    cut: usize, // first bin that is >= 500 Hz
}

impl Analyzer {
    pub fn new(sr: u32) -> Self {
        let r2c = RealFftPlanner::<f32>::new().plan_fft_forward(BLOCK);
        // numpy.hanning is the symmetric window: 0.5 - 0.5*cos(2*pi*n/(N-1))
        let win = (0..BLOCK)
            .map(|n| 0.5 - 0.5 * (2.0 * PI * n as f32 / (BLOCK - 1) as f32).cos())
            .collect();
        let cut = (0..BINS)
            .take_while(|&k| (k as f32) * sr as f32 / (BLOCK as f32) < LOW_CUT_HZ)
            .count();
        Self {
            input: r2c.make_input_vec(),
            spec: r2c.make_output_vec(),
            scratch: r2c.make_scratch_vec(),
            mag: vec![0.0; BINS],
            r2c,
            win,
            cut,
        }
    }

    /// Magnitude spectrum of one BLOCK of samples. With `cut_low`, bins under 500 Hz are zeroed.
    pub fn spectrum(&mut self, block: &[f32], cut_low: bool) -> &[f32] {
        debug_assert_eq!(block.len(), BLOCK);
        for ((o, &x), &w) in self.input.iter_mut().zip(block).zip(&self.win) {
            *o = x * w;
        }
        self.r2c
            .process_with_scratch(&mut self.input, &mut self.spec, &mut self.scratch)
            .expect("fft buffer sizes are fixed");
        for (m, c) in self.mag.iter_mut().zip(&self.spec) {
            *m = c.norm();
        }
        if cut_low {
            self.mag[..self.cut].fill(0.0);
        }
        &self.mag
    }

    pub fn cut_bin(&self) -> usize {
        self.cut
    }
}

pub fn rms(block: &[f32]) -> f32 {
    let sum: f64 = block.iter().map(|&x| f64::from(x) * f64::from(x)).sum();
    (sum / block.len() as f64).sqrt() as f32
}

pub fn dot(a: &[f32], b: &[f32]) -> f64 {
    a.iter().zip(b).map(|(&x, &y)| f64::from(x) * f64::from(y)).sum()
}

pub fn norm(a: &[f32]) -> f64 {
    dot(a, a).sqrt()
}

pub fn sum(a: &[f32]) -> f64 {
    a.iter().map(|&x| f64::from(x)).sum()
}

/// Marks the five bins either side of each peak (p-2 ..= p+2), like the Python mask.
pub fn peak_mask(peaks: &[usize]) -> Vec<bool> {
    let mut mask = vec![false; BINS];
    for &p in peaks {
        let lo = p.saturating_sub(2);
        let hi = (p + 3).min(BINS);
        mask[lo..hi].fill(true);
    }
    mask
}

pub fn masked_sum(mag: &[f32], mask: &[bool]) -> f64 {
    mag.iter()
        .zip(mask)
        .filter(|(_, m)| **m)
        .map(|(&x, _)| f64::from(x))
        .sum()
}

/// Cosine similarity of a spectrum against a unit-norm template.
pub fn similarity(mag: &[f32], tpl: &[f32]) -> f64 {
    dot(mag, tpl) / (norm(mag) + 1e-9)
}

/// Share of the spectrum's energy that sits on the template's peaks.
pub fn tonality(mag: &[f32], mask: &[bool]) -> f64 {
    masked_sum(mag, mask) / (sum(mag) + 1e-9)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference values computed with numpy (np.hanning + np.fft.rfft) for the same signal.
    #[test]
    fn matches_numpy() {
        let sr = 48000.0_f64;
        let x: Vec<f32> = (0..BLOCK)
            .map(|n| {
                let t = n as f64 / sr;
                (0.5 * (2.0 * std::f64::consts::PI * 3000.0 * t).sin()
                    + 0.2 * (2.0 * std::f64::consts::PI * 7000.5 * t).sin()
                    + 0.05 * (2.0 * std::f64::consts::PI * 200.0 * t).sin()) as f32
            })
            .collect();
        let mut an = Analyzer::new(48000);
        assert_eq!(an.cut_bin(), 22);
        assert!((f64::from(rms(&x)) - 0.38234904408454895).abs() < 1e-6);

        let raw = an.spectrum(&x, false).to_vec();
        for (k, want) in [(9, 22.19452869399008), (128, 255.8749998024763), (129, 128.0312084114964),
                          (299, 96.08992498057688), (300, 28.634479818564706), (301, 2.6916149776689653)] {
            let got = f64::from(raw[k]);
            assert!((got - want).abs() / want < 1e-3, "bin {k}: {got} vs {want}");
        }

        let mut tpl = vec![0f32; BINS];
        tpl[128..131].copy_from_slice(&[1.0, 2.0, 1.0]);
        let n = norm(&tpl) as f32;
        tpl.iter_mut().for_each(|t| *t /= n);
        let cut = an.spectrum(&x, true).to_vec();
        assert!(cut[..22].iter().all(|&v| v == 0.0));
        assert!((similarity(&cut, &tpl) - 0.6191104054573813).abs() < 1e-3);
        assert!((tonality(&cut, &peak_mask(&[128])) - 0.7035959313380746).abs() < 1e-3);
    }
}
