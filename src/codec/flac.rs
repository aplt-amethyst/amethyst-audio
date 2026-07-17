use anyhow::{Context, Result};
use claxon::FlacReader;
use std::path::Path;

pub struct FlacInfo {
    pub sample_rate: u32,
    pub channels: u32,
    pub duration_sec: f64,
    pub pcm_data: Vec<i16>,
}

/// Read a FLAC file and decode PCM samples as i16.
///
/// # Errors
///
/// Returns an error if the file cannot be read or is not valid FLAC.
pub fn read_flac(path: &Path) -> Result<FlacInfo> {
    let mut reader = FlacReader::open(path).context("failed to open FLAC file")?;

    let streaminfo = reader.streaminfo();
    let sample_rate = streaminfo.sample_rate;
    let channels = streaminfo.channels;
    let total_samples = streaminfo.samples.unwrap_or(0);

    let mut pcm_data = Vec::new();

    if reader.streaminfo().bits_per_sample == 16 {
        for sample_result in reader.samples() {
            let sample: i32 = sample_result.context("failed to read FLAC sample")?;
            let sample: i16 = i16::try_from(sample).unwrap_or(0);
            pcm_data.push(sample);
        }
    } else {
        for sample_result in reader.samples() {
            let sample: i32 = sample_result.context("failed to read FLAC sample")?;
            pcm_data.push(sample as i16);
        }
    }

    let duration_sec = total_samples as f64 / f64::from(sample_rate);

    Ok(FlacInfo {
        sample_rate,
        channels,
        duration_sec,
        pcm_data,
    })
}
