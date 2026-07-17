use super::packet::TsPacket;
use super::pat::build_pat;
use super::pes::{build_pcr_packet, build_pes_packet, compute_pts};
use super::pmt::build_pmt;
use super::{AUDIO_PID, PAT_INTERVAL_MS, PCR_CLOCK_HZ, PMT_INTERVAL_MS, TS_PACKET_SIZE};

pub struct TsMuxer {
    stream_type: u8,
    bitrate_bps: u64,
    pat_cc: u8,
    pmt_cc: u8,
    audio_cc: u8,
    byte_position: u64,
    last_pcr_pts: u64,
    last_pat_time_ms: u64,
    last_pmt_time_ms: u64,
}

impl TsMuxer {
    /// Create a new TS muxer for the given stream type.
    ///
    /// # Arguments
    ///
    /// * `stream_type` - `STREAM_TYPE_AAC` (0x0F) or `STREAM_TYPE_MP3` (0x03)
    /// * `bitrate_bps` - Audio bitrate in bits per second (used for PTS calculation)
    pub fn new(stream_type: u8, bitrate_bps: u64) -> Self {
        Self {
            stream_type,
            bitrate_bps,
            pat_cc: 0,
            pmt_cc: 0,
            audio_cc: 0,
            byte_position: 0,
            last_pcr_pts: 0,
            last_pat_time_ms: 0,
            last_pmt_time_ms: 0,
        }
    }

    /// Mux raw audio data into TS packets. `raw_data` is the elementary stream
    /// data (raw AAC without ADTS headers, or raw MP3 frames).
    ///
    /// `elapsed_ms` tracks how many milliseconds of audio have been produced so
    /// far in the current segment. The muxer inserts PAT, PMT, and PCR
    /// packets at the appropriate intervals.
    pub fn mux(&mut self, raw_data: &[u8], elapsed_ms: u64) -> Vec<TsPacket> {
        let mut packets = Vec::new();

        if self.should_insert_pat(elapsed_ms) {
            let pat = build_pat(1, super::PMT_PID);
            self.pat_cc = (self.pat_cc + 1) & 0x0F;
            let mut pkt = pat;
            pkt.continuity_counter = self.pat_cc;
            packets.push(pkt);
            self.last_pat_time_ms = elapsed_ms;
        }

        if self.should_insert_pmt(elapsed_ms) {
            let pmt = build_pmt(1, AUDIO_PID, self.stream_type, AUDIO_PID);
            self.pmt_cc = (self.pmt_cc + 1) & 0x0F;
            let mut pkt = pmt;
            pkt.continuity_counter = self.pmt_cc;
            packets.push(pkt);
            self.last_pmt_time_ms = elapsed_ms;
        }

        let pts = compute_pts(self.byte_position, self.bitrate_bps);

        if packets.is_empty() && pts > self.last_pcr_pts + PCR_CLOCK_HZ / 10 {
            let pcr_pkt = build_pcr_packet(pts, self.audio_cc);
            self.audio_cc = (self.audio_cc + 1) & 0x0F;
            self.last_pcr_pts = pts;
            packets.push(pcr_pkt);
        }

        let pes_packets = build_pes_packet(raw_data, pts, self.audio_cc);
        if let Some(last) = pes_packets.last() {
            self.audio_cc = last.continuity_counter;
        }

        self.byte_position += raw_data.len() as u64;

        packets.extend(pes_packets);
        packets
    }

    /// Begin a new segment. Outputs the initial PAT and PMT that must
    /// appear at the start of every TS segment.
    pub fn begin_segment(&mut self) -> Vec<TsPacket> {
        let mut packets = Vec::new();
        self.last_pat_time_ms = 0;
        self.last_pmt_time_ms = 0;
        self.last_pcr_pts = 0;
        self.byte_position = 0;

        let pat = build_pat(1, super::PMT_PID);
        self.pat_cc = (self.pat_cc + 1) & 0x0F;
        let mut pkt = pat;
        pkt.continuity_counter = self.pat_cc;
        packets.push(pkt);

        let mut pmt = build_pmt(1, AUDIO_PID, self.stream_type, AUDIO_PID);
        self.pmt_cc = (self.pmt_cc + 1) & 0x0F;
        pmt.continuity_counter = self.pmt_cc;
        packets.push(pmt);

        packets
    }

    fn should_insert_pat(&self, elapsed_ms: u64) -> bool {
        elapsed_ms >= self.last_pat_time_ms + PAT_INTERVAL_MS
    }

    fn should_insert_pmt(&self, elapsed_ms: u64) -> bool {
        elapsed_ms >= self.last_pmt_time_ms + PMT_INTERVAL_MS
    }
}

/// Write a slice of TS packets to a byte vector (for file output).
pub fn write_packets(packets: &[TsPacket]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(packets.len() * TS_PACKET_SIZE);
    for pkt in packets {
        buf.extend_from_slice(&pkt.to_bytes());
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_muxer() {
        let muxer = TsMuxer::new(0x0F, 128_000);
        assert_eq!(muxer.stream_type, 0x0F);
        assert_eq!(muxer.bitrate_bps, 128_000);
    }

    #[test]
    fn test_begin_segment_emits_pat_and_pmt() {
        let mut muxer = TsMuxer::new(0x0F, 128_000);
        let packets = muxer.begin_segment();
        assert!(!packets.is_empty());
        assert!(packets.iter().any(|p| p.pid == 0x0000));
        assert!(packets.iter().any(|p| p.pid == 0x0100));
    }

    #[test]
    fn test_mux_produces_audio_packets() {
        let mut muxer = TsMuxer::new(0x0F, 128_000);
        let data = vec![0xAA; 1000];
        let packets = muxer.mux(&data, 100);
        let has_audio = packets.iter().any(|p| p.pid == 0x0101);
        assert!(has_audio);
    }

    #[test]
    fn test_write_packets_size() {
        let mut muxer = TsMuxer::new(0x0F, 128_000);
        let packets = muxer.begin_segment();
        let buf = write_packets(&packets);
        assert_eq!(buf.len(), packets.len() * 188);
    }
}
