use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;
use std::process::Command;

/// Transcode PCM audio (from WAV/FLAC) to AAC ADTS via ffmpeg.
///
/// Writes raw PCM (s16le) to ffmpeg stdin, reads AAC ADTS from stdout.
///
/// # Errors
///
/// Returns an error if ffmpeg is not found or fails during transcoding.
pub fn transcode_to_aac(
    pcm_data: &[u8],
    sample_rate: u32,
    channels: u16,
    _bitrate_bps: u64,
) -> Result<Vec<u8>> {
    let mut child = Command::new("ffmpeg")
        .args([
            "-f",
            "s16le",
            "-ar",
            &sample_rate.to_string(),
            "-ac",
            &channels.to_string(),
            "-i",
            "pipe:0",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-f",
            "adts",
            "pipe:1",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("failed to spawn ffmpeg for AAC transcoding")?;

    if let Some(ref mut stdin) = child.stdin {
        stdin
            .write_all(pcm_data)
            .context("failed to write PCM to ffmpeg stdin")?;
    }

    let output = child.wait_with_output().context("ffmpeg process failed")?;

    Ok(output.stdout)
}

/// Transcode a WAV file to AAC ADTS via ffmpeg.
///
/// # Errors
///
/// Returns an error if ffmpeg is not found or transcoding fails.
pub fn transcode_file_to_aac(input_path: &Path) -> Result<Vec<u8>> {
    let output = Command::new("ffmpeg")
        .args([
            "-i",
            input_path.to_str().unwrap_or(""),
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-f",
            "adts",
            "pipe:1",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .context("failed to spawn ffmpeg for file transcoding")?;

    Ok(output.stdout)
}
