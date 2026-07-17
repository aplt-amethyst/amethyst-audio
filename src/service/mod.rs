use crate::codec::aac;
use crate::codec::flac;
use crate::codec::wav;
use crate::config::ServerConfig;
use crate::playlist::Playlist;
use crate::ts::adts_parser;
use crate::ts::mp3_parser;
use crate::ts::muxer::{write_packets, TsMuxer};
use crate::ts::STREAM_TYPE_AAC;
use crate::ts::STREAM_TYPE_MP3;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tokio::sync::RwLock;
use tracing::{debug, info};

#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub id: String,
    pub file_path: PathBuf,
    pub format: SourceFormat,
    pub sample_rate: u32,
    pub bitrate_bps: u64,
    pub duration_sec: f64,
    pub is_live: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    Aac,
    Mp3,
    Wav,
    Flac,
}

impl SourceFormat {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "aac" | "adts" => Some(Self::Aac),
            "mp3" => Some(Self::Mp3),
            "wav" => Some(Self::Wav),
            "flac" => Some(Self::Flac),
            _ => None,
        }
    }
}

struct IngestState {
    buffer: Vec<u8>,
    muxer: TsMuxer,
    segment_idx: u32,
    audio_elapsed_ms: u64,
    bitrate_bps: u64,
}

pub struct HlsService {
    config: ServerConfig,
    output_dir: PathBuf,
    sources: RwLock<HashMap<String, SourceInfo>>,
    playlists: RwLock<HashMap<String, Playlist>>,
    ingest_states: RwLock<HashMap<String, IngestState>>,
}

impl HlsService {
    pub fn new(config: ServerConfig) -> Self {
        let output_dir = PathBuf::from(&config.output_dir);
        let _ = fs::create_dir_all(&output_dir);

        Self {
            config,
            output_dir,
            sources: RwLock::new(HashMap::new()),
            playlists: RwLock::new(HashMap::new()),
            ingest_states: RwLock::new(HashMap::new()),
        }
    }

    pub async fn register_source(&self, id: String, file_path: PathBuf) -> Result<SourceInfo> {
        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let format = SourceFormat::from_extension(ext)
            .with_context(|| format!("unsupported format: {ext}"))?;

        let (sample_rate, bitrate_bps, duration_sec) = match format {
            SourceFormat::Wav => {
                let info = wav::read_wav(&file_path)?;
                (
                    info.sample_rate,
                    u64::from(info.sample_rate) * u64::from(info.channels) * 16,
                    info.duration_sec,
                )
            }
            SourceFormat::Flac => {
                let info = flac::read_flac(&file_path)?;
                (
                    info.sample_rate,
                    u64::from(info.sample_rate) * u64::from(info.channels) * 16,
                    info.duration_sec,
                )
            }
            SourceFormat::Aac | SourceFormat::Mp3 => {
                let file_data = fs::read(&file_path).context("failed to read source file")?;
                let (sr, br) = Self::probe_raw_audio(&file_data, format);
                let effective_len = if format == SourceFormat::Mp3 {
                    let offset = Self::skip_id3v2(&file_data);
                    file_data.len() - offset
                } else {
                    file_data.len()
                };
                let duration = if br > 0 {
                    effective_len as f64 * 8.0 / br as f64
                } else {
                    0.0
                };
                (sr, br, duration)
            }
        };

        let info = SourceInfo {
            id: id.clone(),
            file_path,
            format,
            sample_rate,
            bitrate_bps,
            duration_sec,
            is_live: false,
        };

        let mut sources = self.sources.write().await;
        sources.insert(id.clone(), info.clone());

        let mut playlists = self.playlists.write().await;
        playlists.insert(
            id.clone(),
            Playlist::new(self.config.segment_duration_sec, false),
        );

        Ok(info)
    }

    pub async fn register_live_source(&self, id: String, bitrate_bps: u64) -> SourceInfo {
        let info = SourceInfo {
            id: id.clone(),
            file_path: PathBuf::new(),
            format: SourceFormat::Aac,
            sample_rate: 44100,
            bitrate_bps,
            duration_sec: 0.0,
            is_live: true,
        };

        let mut sources = self.sources.write().await;
        sources.insert(id.clone(), info.clone());

        let mut playlists = self.playlists.write().await;
        playlists.insert(
            id.clone(),
            Playlist::new(self.config.segment_duration_sec, true),
        );

        let mut states = self.ingest_states.write().await;
        states.insert(
            id.clone(),
            IngestState {
                buffer: Vec::new(),
                muxer: TsMuxer::new(STREAM_TYPE_AAC, bitrate_bps),
                segment_idx: 0,
                audio_elapsed_ms: 0,
                bitrate_bps,
            },
        );

        info!(source_id = %id, bitrate_bps, "live source registered");
        info
    }

    pub async fn ingest_chunk(&self, id: &str, data: &[u8]) -> Result<()> {
        let mut states = self.ingest_states.write().await;
        let state = states
            .get_mut(id)
            .with_context(|| format!("live source not found: {id}"))?;

        state.buffer.extend_from_slice(data);

        let segment_bytes = (self.config.segment_duration_sec * state.bitrate_bps / 8) as usize;

        while state.buffer.len() >= segment_bytes.max(1024) {
            let chunk: Vec<u8> = state
                .buffer
                .drain(..segment_bytes.min(state.buffer.len()))
                .collect();
            self.write_live_segment(id, state, &chunk).await?;
        }

        Ok(())
    }

    pub async fn flush_live_buffer(&self, id: &str) -> Result<()> {
        let mut states = self.ingest_states.write().await;
        let state = states
            .get_mut(id)
            .with_context(|| format!("live source not found: {id}"))?;

        if !state.buffer.is_empty() {
            let chunk = std::mem::take(&mut state.buffer);
            self.write_live_segment(id, state, &chunk).await?;
        }

        Ok(())
    }

    async fn write_live_segment(
        &self,
        source_id: &str,
        state: &mut IngestState,
        raw_chunk: &[u8],
    ) -> Result<()> {
        let raw_aac = Self::strip_adts_frames(raw_chunk);
        if raw_aac.is_empty() {
            return Ok(());
        }

        let elapsed = state.audio_elapsed_ms % (self.config.segment_duration_sec * 1000);
        let elapsed_seconds = elapsed / 1000;

        let packets = if state.segment_idx == 0 || elapsed_seconds == 0 {
            let mut pkts = state.muxer.begin_segment();
            pkts.extend(state.muxer.mux(&raw_aac, elapsed));
            pkts
        } else {
            state.muxer.mux(&raw_aac, elapsed_seconds * 1000)
        };

        if packets.is_empty() {
            state.audio_elapsed_ms += (raw_aac.len() as u64 * 8 * 1000) / state.bitrate_bps.max(1);
            return Ok(());
        }

        let seg_filename = format!("{}-{:04}.ts", source_id, state.segment_idx);
        let seg_path = self.output_dir.join(&seg_filename);
        let seg_bytes = write_packets(&packets);
        fs::write(&seg_path, &seg_bytes)
            .with_context(|| format!("failed to write live segment: {seg_filename}"))?;

        debug!(
            source_id = %source_id,
            segment = state.segment_idx,
            size = seg_bytes.len(),
            "live segment written"
        );

        let filename = seg_filename;
        let duration = self.config.segment_duration_sec as f64;

        {
            let mut playlists = self.playlists.write().await;
            if let Some(pl) = playlists.get_mut(source_id) {
                pl.add_segment(filename, duration);
                pl.trim_to_window(self.config.max_live_segments);

                let removed_start = pl.media_sequence;
                for old_idx in 0..removed_start {
                    let old_name = format!("{}-{:04}.ts", source_id, old_idx);
                    let old_path = self.output_dir.join(&old_name);
                    let _ = fs::remove_file(&old_path);
                }
            }
        }

        state.segment_idx += 1;
        state.audio_elapsed_ms += (raw_aac.len() as u64 * 8 * 1000) / state.bitrate_bps.max(1);

        Ok(())
    }

    fn strip_adts_frames(data: &[u8]) -> Vec<u8> {
        let mut raw = Vec::new();
        let mut offset = 0;
        while offset < data.len() {
            if let Some(sync) = adts_parser::find_adts_sync(data, offset) {
                if sync > offset {
                    offset = sync;
                }
                match adts_parser::parse_adts_frame(&data[offset..]) {
                    Ok((frame, consumed)) => {
                        raw.extend_from_slice(&frame.raw_aac);
                        offset += consumed;
                    }
                    Err(_) => {
                        offset += 1;
                    }
                }
            } else {
                break;
            }
        }
        raw
    }

    fn skip_id3v2(data: &[u8]) -> usize {
        if data.len() >= 10 && &data[..3] == b"ID3" {
            let size = ((data[6] as usize & 0x7F) << 21)
                | ((data[7] as usize & 0x7F) << 14)
                | ((data[8] as usize & 0x7F) << 7)
                | (data[9] as usize & 0x7F);
            (10 + size).min(data.len())
        } else if data.len() >= 2 {
            crate::ts::mp3_parser::find_mp3_sync(data, 0).unwrap_or(0)
        } else {
            0
        }
    }

    fn probe_raw_audio(data: &[u8], format: SourceFormat) -> (u32, u64) {
        match format {
            SourceFormat::Aac => {
                if let Ok((frame, _)) = adts_parser::parse_adts_frame(data) {
                    let bitrate =
                        (frame.frame_length as u64 * 8 * u64::from(frame.sample_rate_hz)) / 1024;
                    (frame.sample_rate_hz, bitrate)
                } else {
                    (44100, 128_000)
                }
            }
            SourceFormat::Mp3 => {
                let offset = Self::skip_id3v2(data);
                if offset < data.len() {
                    let audio_data = &data[offset..];
                    if let Ok((frame, _)) = mp3_parser::parse_mp3_frame(audio_data) {
                        return (frame.sample_rate_hz, frame.bitrate_bps);
                    }
                }
                (44100, 128_000)
            }
            _ => (44100, 128_000),
        }
    }

    pub async fn generate_vod_segments(&self, source_id: &str) -> Result<Vec<PathBuf>> {
        let info = {
            let sources = self.sources.read().await;
            sources
                .get(source_id)
                .cloned()
                .with_context(|| format!("source not found: {source_id}"))?
        };

        let raw_audio = self.load_and_prepare_audio(&info)?;
        let stream_type = match info.format {
            SourceFormat::Aac | SourceFormat::Wav | SourceFormat::Flac => STREAM_TYPE_AAC,
            SourceFormat::Mp3 => STREAM_TYPE_MP3,
        };

        let segment_bytes =
            self.config.segment_duration_sec as usize * (info.bitrate_bps / 8) as usize;
        let mut muxer = TsMuxer::new(stream_type, info.bitrate_bps);
        let mut segment_paths = Vec::new();
        let mut elapsed_ms: u64 = 0;
        let segment_duration_ms = self.config.segment_duration_sec * 1000;

        let chunks: Vec<&[u8]> = raw_audio.chunks(segment_bytes.max(1024)).collect();

        let mut seg_idx = 0u32;
        for chunk in &chunks {
            if chunk.is_empty() {
                continue;
            }

            let seg_packets = if seg_idx == 0 {
                let mut pkts = muxer.begin_segment();
                pkts.extend(muxer.mux(chunk, elapsed_ms % segment_duration_ms));
                pkts
            } else if elapsed_ms >= u64::from(seg_idx) * segment_duration_ms {
                seg_idx += 1;
                let mut pkts = muxer.begin_segment();
                pkts.extend(muxer.mux(chunk, 0));
                pkts
            } else {
                muxer.mux(chunk, (elapsed_ms % segment_duration_ms) / 1000 * 1000)
            };

            if seg_packets.is_empty() {
                elapsed_ms += (chunk.len() as u64 * 8 * 1000) / info.bitrate_bps.max(1);
                continue;
            }

            let seg_filename = format!("{}-{:04}.ts", source_id, segment_paths.len());
            let seg_path = self.output_dir.join(&seg_filename);
            let seg_bytes = write_packets(&seg_packets);
            fs::write(&seg_path, &seg_bytes)
                .with_context(|| format!("failed to write segment: {seg_filename}"))?;
            segment_paths.push(seg_path);

            elapsed_ms += (chunk.len() as u64 * 8 * 1000) / info.bitrate_bps.max(1);
        }

        {
            let mut playlists = self.playlists.write().await;
            if let Some(pl) = playlists.get_mut(source_id) {
                pl.segments.clear();
                for (i, seg_path) in segment_paths.iter().enumerate() {
                    let filename = seg_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown.ts")
                        .to_string();
                    let duration = self.config.segment_duration_sec as f64;
                    pl.add_segment(filename, duration);

                    if i + 1 == segment_paths.len() {
                        let remaining = info.duration_sec
                            - (i as f64 * self.config.segment_duration_sec as f64);
                        if remaining > 0.0 && remaining < self.config.segment_duration_sec as f64 {
                            pl.segments.last_mut().unwrap().duration_sec = remaining.max(0.1);
                        }
                    }
                }
            }
        }

        Ok(segment_paths)
    }

    fn load_and_prepare_audio(&self, info: &SourceInfo) -> Result<Vec<u8>> {
        match info.format {
            SourceFormat::Aac => {
                let data = fs::read(&info.file_path).context("failed to read AAC file")?;
                Ok(Self::strip_adts_frames(&data))
            }
            SourceFormat::Mp3 => {
                let data = fs::read(&info.file_path).context("failed to read MP3 file")?;
                let offset = Self::skip_id3v2(&data);
                Ok(data[offset..].to_vec())
            }
            SourceFormat::Wav => {
                let wav_info = wav::read_wav(&info.file_path)?;
                let pcm_bytes = wav::pcm_to_bytes(&wav_info.pcm_data);
                let aac_data = aac::transcode_to_aac(
                    &pcm_bytes,
                    wav_info.sample_rate,
                    wav_info.channels,
                    128_000,
                )?;
                Ok(Self::strip_adts_frames(&aac_data))
            }
            SourceFormat::Flac => {
                let flac_info = flac::read_flac(&info.file_path)?;
                let pcm_bytes = {
                    let mut bytes = Vec::with_capacity(flac_info.pcm_data.len() * 2);
                    for sample in &flac_info.pcm_data {
                        bytes.extend_from_slice(&sample.to_le_bytes());
                    }
                    bytes
                };
                let aac_data = aac::transcode_to_aac(
                    &pcm_bytes,
                    flac_info.sample_rate,
                    flac_info.channels as u16,
                    128_000,
                )?;
                Ok(Self::strip_adts_frames(&aac_data))
            }
        }
    }

    pub async fn get_playlist(&self, source_id: &str) -> Result<String> {
        let playlists = self.playlists.read().await;
        let pl = playlists
            .get(source_id)
            .with_context(|| format!("playlist not found for source: {source_id}"))?;
        Ok(pl.to_m3u8())
    }

    pub async fn get_segment_data(&self, _source_id: &str, segment_name: &str) -> Result<Vec<u8>> {
        let seg_path = self.output_dir.join(segment_name);
        fs::read(&seg_path).with_context(|| format!("segment not found: {segment_name}"))
    }

    pub async fn list_sources(&self) -> Vec<SourceInfo> {
        let sources = self.sources.read().await;
        sources.values().cloned().collect()
    }
}
