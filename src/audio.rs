//! Input-device discovery and capture (WASAPI via cpal). Delivers fixed BLOCK-sized mono
//! f32 blocks to a callback, whatever buffer size/channel count/sample format the device uses.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Error, FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig};

use crate::dsp::BLOCK;

/// Inputs that hear system audio or are just aliases of the default mic.
const EXCLUDE: [&str; 7] = [
    "stereo mix", "mezcla", "loopback", "what u hear", "sound mapper", "asignador", "mapeador",
];

pub struct Input {
    pub index: usize,
    pub name: String,
    pub sr: u32,
    device: Device,
    config: StreamConfig,
    format: SampleFormat,
}

/// Every usable input device. `only` restricts to a single index (the --device flag).
pub fn inputs(only: Option<usize>) -> Result<Vec<Input>, Error> {
    let host = cpal::default_host();
    let mut found = Vec::new();
    for (index, device) in host.input_devices()?.enumerate() {
        if only.is_some_and(|o| o != index) {
            continue;
        }
        let Ok(desc) = device.description() else { continue };
        let name = desc.name().to_string();
        if only.is_none() && EXCLUDE.iter().any(|x| name.to_lowercase().contains(x)) {
            continue;
        }
        let Ok(supported) = device.default_input_config() else { continue };
        found.push(Input {
            index,
            name,
            sr: supported.sample_rate(),
            format: supported.sample_format(),
            config: supported.into(),
            device,
        });
    }
    Ok(found)
}

impl Input {
    /// Starts capturing. `on_block` runs on the audio thread once per BLOCK samples.
    pub fn open<F>(&self, on_block: F) -> Result<Stream, String>
    where
        F: FnMut(&[f32]) + Send + 'static,
    {
        let channels = usize::from(self.config.channels);
        let name = self.name.clone();
        let err_fn = move |e: Error| eprintln!("[{name}] stream error: {e}");
        let cfg = self.config.clone();
        let stream = match self.format {
            SampleFormat::F32 => build::<f32, _>(&self.device, cfg, channels, on_block, err_fn),
            SampleFormat::I16 => build::<i16, _>(&self.device, cfg, channels, on_block, err_fn),
            SampleFormat::I32 => build::<i32, _>(&self.device, cfg, channels, on_block, err_fn),
            other => return Err(format!("unsupported sample format {other}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(stream)
    }
}

fn build<T, F>(
    device: &Device,
    config: StreamConfig,
    channels: usize,
    mut on_block: F,
    err_fn: impl FnMut(Error) + Send + 'static,
) -> Result<Stream, Error>
where
    T: SizedSample,
    f32: FromSample<T>,
    F: FnMut(&[f32]) + Send + 'static,
{
    let mut buf: Vec<f32> = Vec::with_capacity(BLOCK);
    device.build_input_stream(
        config,
        move |data: &[T], _| {
            for frame in data.chunks_exact(channels) {
                // downmix to mono
                let s: f32 = frame.iter().map(|&x| f32::from_sample(x)).sum::<f32>() / channels as f32;
                buf.push(s);
                if buf.len() == BLOCK {
                    on_block(&buf);
                    buf.clear();
                }
            }
        },
        err_fn,
        None,
    )
}
