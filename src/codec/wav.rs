use anyhow::{Context, Result};
use hound::WavReader;
use std::path::Path;

pub struct WavInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_sec: f64,
    pub pcm_data: Vec<i16>,
}

/// Read a WAV file and extract PCM samples as i16.
///
/// # Errors
///
/// Returns an error if the file cannot be read or is not a valid WAV.
pub fn read_wav(path: &Path) -> Result<WavInfo> {
    let mut reader = WavReader::open(path).context("failed to open WAV file")?;

    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let channels = spec.channels;

    let samples: Vec<i16> = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .context("failed to read WAV samples")?;

    let total_samples = samples.len() as u64;
    let duration_sec = total_samples as f64 / f64::from(sample_rate) / f64::from(channels);

    Ok(WavInfo {
        sample_rate,
        channels,
        duration_sec,
        pcm_data: samples,
    })
}

/// Convert i16 PCM samples to a byte slice (little-endian s16le).
pub fn pcm_to_bytes(samples: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}
