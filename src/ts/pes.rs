use super::packet::TsPacket;
use super::packet::{AdaptationField, AdaptationFieldControl};
use super::{AUDIO_PID, PCR_CLOCK_HZ};

const PES_START_CODE: [u8; 3] = [0x00, 0x00, 0x01];
const PES_STREAM_ID_AUDIO: u8 = 0xC0;

/// Build a PES packet containing audio elementary stream data.
///
/// `pts_90khz` is the presentation timestamp in 90 kHz ticks, calculated as:
/// `pts_90khz = (byte_offset * PCR_CLOCK_HZ) / (sample_rate * bytes_per_sample * channels)`
/// where `byte_offset` is the byte position in the raw audio stream.
pub fn build_pes_packet(data: &[u8], pts_90khz: u64, prev_cc: u8) -> Vec<TsPacket> {
    let pes_header = build_pes_header(data.len(), pts_90khz);
    let mut pes_data = pes_header;
    pes_data.extend_from_slice(data);

    let mut packets = Vec::new();
    let mut cc = prev_cc;
    let mut remaining = &pes_data[..];

    let mut first = true;
    while !remaining.is_empty() {
        let chunk_size = 184usize.min(remaining.len());
        let chunk = remaining[..chunk_size].to_vec();
        remaining = &remaining[chunk_size..];

        let pusi = first;
        first = false;

        cc = (cc + 1) & 0x0F;

        packets.push(TsPacket {
            pid: AUDIO_PID,
            continuity_counter: cc,
            payload_unit_start_indicator: pusi,
            adaptation_field_control: AdaptationFieldControl::PayloadOnly,
            adaptation_field: None,
            payload: chunk,
        });
    }

    packets
}

/// Build a PCR packet (adaptation field only, no payload).
/// Inserts a PCR timestamp to synchronize the decoder clock.
pub fn build_pcr_packet(pcr_90khz: u64, prev_cc: u8) -> TsPacket {
    let cc = (prev_cc + 1) & 0x0F;

    TsPacket {
        pid: AUDIO_PID,
        continuity_counter: cc,
        payload_unit_start_indicator: false,
        adaptation_field_control: AdaptationFieldControl::AdaptationOnly,
        adaptation_field: Some(AdaptationField::with_pcr(pcr_90khz)),
        payload: Vec::new(),
    }
}

fn build_pes_header(data_len: usize, pts_90khz: u64) -> Vec<u8> {
    let mut header = Vec::new();

    header.extend_from_slice(&PES_START_CODE);
    header.push(PES_STREAM_ID_AUDIO);

    let pes_packet_length = if data_len < 0xFFFF {
        data_len as u16 + 8
    } else {
        0
    };

    let pts_data_bits = pts_90khz & 0x1_FFFF_FFFF;
    let pts_encoded = (0x21u64 << 40)
        | ((pts_data_bits >> 30) & 0x07) << 37
        | 0x01u64 << 36
        | ((pts_data_bits >> 15) & 0x7FFF) << 22
        | 0x01u64 << 21
        | (pts_data_bits & 0x7FFF) << 7
        | 0x01u64 << 6;

    header.push((pes_packet_length >> 8) as u8);
    header.push((pes_packet_length & 0xFF) as u8);

    header.push(0x80);
    header.push(0x80);
    header.push(0x05);

    header.push((pts_encoded >> 32) as u8);
    header.push((pts_encoded >> 24) as u8);
    header.push((pts_encoded >> 16) as u8);
    header.push((pts_encoded >> 8) as u8);
    header.push(pts_encoded as u8);

    header
}

/// Compute PTS in 90 kHz ticks from a byte position in a raw audio stream.
///
/// # Arguments
///
/// * `byte_position` - The byte offset from the start of the raw audio stream.
///   For AAC, this is the sum of raw AAC frame sizes (excluding ADTS headers).
///   For MP3, this is the sum of raw MP3 frame sizes.
/// * `bitrate_bps` - The bitrate in bits per second.
pub fn compute_pts(byte_position: u64, bitrate_bps: u64) -> u64 {
    if bitrate_bps == 0 {
        return 0;
    }
    (byte_position * 8 * PCR_CLOCK_HZ) / bitrate_bps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_pes_packet_produces_188_byte_packets() {
        let data = vec![0xAA; 500];
        let packets = build_pes_packet(&data, 0, 0);
        for pkt in &packets {
            let bytes = pkt.to_bytes();
            assert_eq!(bytes.len(), 188);
            assert_eq!(bytes[0], 0x47);
        }
    }

    #[test]
    fn test_first_pes_packet_has_pusi() {
        let data = vec![0xAA; 100];
        let packets = build_pes_packet(&data, 0, 0);
        assert!(packets[0].payload_unit_start_indicator);
    }

    #[test]
    fn test_continuity_counter_increments() {
        let data = vec![0xAA; 500];
        let packets = build_pes_packet(&data, 0, 5);
        assert!(packets.len() > 1);
        for i in 1..packets.len() {
            assert_eq!(
                packets[i].continuity_counter,
                (packets[i - 1].continuity_counter.wrapping_add(1)) & 0x0F
            );
        }
    }

    #[test]
    fn test_pcr_packet_has_adaptation_only() {
        let pcr_pkt = build_pcr_packet(90000, 0);
        assert_eq!(
            pcr_pkt.adaptation_field_control,
            AdaptationFieldControl::AdaptationOnly
        );
        assert!(pcr_pkt.adaptation_field.is_some());
        let af = pcr_pkt.adaptation_field.as_ref().unwrap();
        assert!(af.pcr_flag);
        assert!(af.pcr_value.is_some());
    }

    #[test]
    fn test_compute_pts() {
        let pts = compute_pts(0, 128_000);
        assert_eq!(pts, 0);

        let pts = compute_pts(16000, 128_000);
        assert_eq!(pts, 90_000);
    }
}
