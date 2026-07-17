use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Mp3Frame {
    pub mpeg_version: MpegVersion,
    pub layer: MpegLayer,
    pub bitrate_bps: u64,
    pub sample_rate_hz: u32,
    pub channels: u8,
    pub frame_length: usize,
    pub raw_frame: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpegVersion {
    Mpeg1,
    Mpeg2,
    Mpeg25,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpegLayer {
    Layer1,
    Layer2,
    Layer3,
}

#[derive(Error, Debug)]
pub enum Mp3Error {
    #[error("sync word not found (expected at least 11 bits set)")]
    SyncNotFound,
    #[error("frame too short: {0} bytes (minimum 4)")]
    FrameTooShort(usize),
    #[error("invalid bitrate index: {0}")]
    InvalidBitrate(u8),
    #[error("invalid sample rate index: {0}")]
    InvalidSampleRate(u8),
    #[error("free format bitrate not supported")]
    FreeFormatNotSupported,
}

static BITRATE_TABLE: [[u64; 16]; 5] = [
    [
        0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448, 0,
    ],
    [
        0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 0,
    ],
    [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
    ],
    [
        0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256, 0,
    ],
    [
        0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
    ],
];

static SAMPLE_RATE_TABLE_MP3: [[u32; 4]; 3] = [
    [44100, 48000, 32000, 0],
    [22050, 24000, 16000, 0],
    [11025, 12000, 8000, 0],
];

/// Parse an MPEG audio frame header and extract the full frame.
///
/// # Errors
///
/// Returns `Mp3Error::SyncNotFound` if the sync word is not found.
/// Returns `Mp3Error::FrameTooShort` if the buffer is smaller than the declared frame length.
pub fn parse_mp3_frame(data: &[u8]) -> Result<(Mp3Frame, usize), Mp3Error> {
    if data.len() < 4 {
        return Err(Mp3Error::FrameTooShort(data.len()));
    }

    let sync = u16::from(data[0]) << 3 | u16::from(data[1] >> 5);
    if sync != 0x7FF {
        return Err(Mp3Error::SyncNotFound);
    }

    let version_idx = (data[1] >> 3) & 0x03;
    let layer_idx = (data[1] >> 1) & 0x03;
    let protection = data[1] & 0x01;
    let bitrate_idx = (data[2] >> 4) & 0x0F;
    let sample_rate_idx = (data[2] >> 2) & 0x03;
    let padding = (data[2] >> 1) & 0x01;
    let _private = data[2] & 0x01;
    let channel_mode = (data[3] >> 6) & 0x03;

    let mpeg_version = match version_idx {
        0x03 => MpegVersion::Mpeg1,
        0x02 => MpegVersion::Mpeg2,
        0x00 => MpegVersion::Mpeg25,
        _ => return Err(Mp3Error::SyncNotFound),
    };

    let layer = match layer_idx {
        0x01 => MpegLayer::Layer3,
        0x02 => MpegLayer::Layer2,
        0x03 => MpegLayer::Layer1,
        _ => return Err(Mp3Error::SyncNotFound),
    };

    let bitrate_table_row = match (mpeg_version, layer) {
        (MpegVersion::Mpeg1, MpegLayer::Layer1) => 0,
        (MpegVersion::Mpeg1, MpegLayer::Layer2) => 1,
        (MpegVersion::Mpeg1, MpegLayer::Layer3) => 2,
        (MpegVersion::Mpeg2 | MpegVersion::Mpeg25, MpegLayer::Layer1) => 3,
        (MpegVersion::Mpeg2 | MpegVersion::Mpeg25, MpegLayer::Layer2 | MpegLayer::Layer3) => 4,
    };

    let bitrate_kbps = BITRATE_TABLE[bitrate_table_row][bitrate_idx as usize];
    if bitrate_kbps == 0 {
        return Err(Mp3Error::InvalidBitrate(bitrate_idx));
    }

    let sample_rate_row = match mpeg_version {
        MpegVersion::Mpeg1 => 0,
        MpegVersion::Mpeg2 => 1,
        MpegVersion::Mpeg25 => 2,
    };
    let sample_rate_hz = SAMPLE_RATE_TABLE_MP3[sample_rate_row][sample_rate_idx as usize];
    if sample_rate_hz == 0 {
        return Err(Mp3Error::InvalidSampleRate(sample_rate_idx));
    }

    let channels: u8 = if channel_mode == 3 { 1 } else { 2 };

    let frame_length = match layer {
        MpegLayer::Layer1 => {
            ((12 * bitrate_kbps * 1000 / sample_rate_hz as u64) + u64::from(padding)) * 4
        }
        _ => (144 * bitrate_kbps * 1000 / sample_rate_hz as u64) + u64::from(padding),
    };

    let _crc_len = if protection == 0 { 2usize } else { 0usize };

    if data.len() < frame_length as usize {
        return Err(Mp3Error::FrameTooShort(data.len()));
    }

    let raw_frame = data[..frame_length as usize].to_vec();

    Ok((
        Mp3Frame {
            mpeg_version,
            layer,
            bitrate_bps: bitrate_kbps * 1000,
            sample_rate_hz,
            channels,
            frame_length: frame_length as usize,
            raw_frame,
        },
        frame_length as usize,
    ))
}

/// Find the next MPEG audio frame sync word in the buffer.
pub fn find_mp3_sync(data: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i + 1 < data.len() {
        if data[i] == 0xFF && (data[i + 1] & 0xE0) == 0xE0 {
            let version = (data[i + 1] >> 3) & 0x03;
            let layer = (data[i + 1] >> 1) & 0x03;
            let bitrate = (data[i + 2] >> 4) & 0x0F;
            let sample_rate = (data[i + 2] >> 2) & 0x03;

            if version != 0x01
                && layer != 0x00
                && bitrate != 0x00
                && bitrate != 0x0F
                && sample_rate != 0x03
            {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mp3_frame_valid() {
        let header = [0xFF, 0xFB, 0x90, 0x00];
        let rest = vec![0x00; 413];
        let mut data = header.to_vec();
        data.extend_from_slice(&rest);

        let (frame, consumed) = parse_mp3_frame(&data).unwrap();
        assert_eq!(consumed, data.len());
        assert_eq!(frame.layer, MpegLayer::Layer3);
        assert_eq!(frame.mpeg_version, MpegVersion::Mpeg1);
        assert_eq!(frame.bitrate_bps, 128_000);
    }

    #[test]
    fn test_parse_mp3_no_sync() {
        let data = [0x00, 0x00, 0x00, 0x00];
        let result = parse_mp3_frame(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_mp3_too_short() {
        let data = [0xFF, 0xFB];
        let result = parse_mp3_frame(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_find_mp3_sync() {
        let data = [0x11, 0x22, 0xFF, 0xFB, 0x90, 0x00];
        let pos = find_mp3_sync(&data, 0).unwrap();
        assert_eq!(pos, 2);
    }
}
