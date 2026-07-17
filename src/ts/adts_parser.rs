use thiserror::Error;

#[derive(Debug, Clone)]
pub struct AdtsFrame {
    pub profile: u8,
    pub sample_rate_hz: u32,
    pub channels: u8,
    pub frame_length: usize,
    pub raw_aac: Vec<u8>,
}

#[derive(Error, Debug)]
pub enum AdtsError {
    #[error("sync word not found (expected 0xFFF)")]
    SyncNotFound,
    #[error("frame too short: {0} bytes (minimum 7)")]
    FrameTooShort(usize),
    #[error("invalid sampling frequency index: {0}")]
    InvalidSampleRate(u8),
}

/// Parse a single ADTS frame from a byte slice. Returns the parsed frame
/// and the number of bytes consumed (frame length including ADTS header).
///
/// # Errors
///
/// Returns `AdtsError::SyncNotFound` if the first 12 bits are not 0xFFF.
/// Returns `AdtsError::FrameTooShort` if the buffer is smaller than the declared frame length.
pub fn parse_adts_frame(data: &[u8]) -> Result<(AdtsFrame, usize), AdtsError> {
    if data.len() < 7 {
        return Err(AdtsError::FrameTooShort(data.len()));
    }

    let sync = u16::from(data[0]) << 4 | u16::from(data[1] >> 4);
    if sync != 0xFFF {
        return Err(AdtsError::SyncNotFound);
    }

    let id = (data[1] >> 3) & 0x01;
    let _layer = (data[1] >> 1) & 0x03;
    let protection_absent = data[1] & 0x01;

    let profile = (data[2] >> 6) & 0x03;
    let sampling_frequency_index = (data[2] >> 2) & 0x0F;
    let _private_bit = (data[2] >> 1) & 0x01;
    let channel_configuration = ((data[2] & 0x01) << 2) | ((data[3] >> 6) & 0x03);

    let _original = (data[3] >> 5) & 0x01;
    let _home = (data[3] >> 4) & 0x01;
    let _copyright_id_bit = (data[3] >> 3) & 0x01;
    let _copyright_id_start = (data[3] >> 2) & 0x01;
    let frame_length = ((usize::from(data[3] & 0x03)) << 11)
        | ((usize::from(data[4])) << 3)
        | (usize::from(data[5]) >> 5);

    if data.len() < frame_length {
        return Err(AdtsError::FrameTooShort(data.len()));
    }

    let sample_rate_hz = SAMPLE_TABLE_RATE[sampling_frequency_index as usize];
    if sample_rate_hz == 0 {
        return Err(AdtsError::InvalidSampleRate(sampling_frequency_index));
    }

    let channels = if id == 0 && channel_configuration == 7 {
        8u8
    } else {
        channel_configuration
    };

    let header_len = if protection_absent == 1 { 7 } else { 9 };

    let raw_aac = data[header_len..frame_length].to_vec();

    let profile_byte = ((profile + 1) << 3) | (sampling_frequency_index >> 1);
    let _profile_byte = profile_byte;

    Ok((
        AdtsFrame {
            profile,
            sample_rate_hz,
            channels,
            frame_length,
            raw_aac,
        },
        frame_length,
    ))
}

/// Find the next ADTS sync word (0xFFF) in the buffer and return its offset.
pub fn find_adts_sync(data: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i + 1 < data.len() {
        if data[i] == 0xFF && (data[i + 1] & 0xF0) == 0xF0 {
            return Some(i);
        }
        i += 1;
    }
    None
}

static SAMPLE_TABLE_RATE: [u32; 16] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350, 0, 0,
    0,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_adts_frame_valid() {
        let header: [u8; 7] = [0xFF, 0xF1, 0x4C, 0x80, 0x0D, 0x67, 0xFC];
        let raw_aac = vec![0x00; 100];
        let mut data = header.to_vec();
        data.extend_from_slice(&raw_aac);

        let (frame, consumed) = parse_adts_frame(&data).unwrap();
        assert_eq!(consumed, data.len());
        assert_eq!(frame.raw_aac.len(), raw_aac.len());
    }

    #[test]
    fn test_parse_adts_frame_no_sync() {
        let data = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let result = parse_adts_frame(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_adts_frame_too_short() {
        let data = [0xFF, 0xF0];
        let result = parse_adts_frame(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_find_adts_sync() {
        let data = [0x00, 0x00, 0xFF, 0xF1, 0x00, 0x00];
        let pos = find_adts_sync(&data, 0).unwrap();
        assert_eq!(pos, 2);
    }

    #[test]
    fn test_find_adts_sync_not_found() {
        let data = [0x00, 0x00, 0x00, 0x00];
        let pos = find_adts_sync(&data, 0);
        assert!(pos.is_none());
    }
}
