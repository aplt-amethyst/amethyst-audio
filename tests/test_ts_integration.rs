use amethyst_audio::ts::muxer::{write_packets, TsMuxer};
use amethyst_audio::ts::{STREAM_TYPE_AAC, STREAM_TYPE_MP3, TS_PACKET_SIZE};
use std::process::Command;

#[test]
fn test_ts_segment_is_valid_mpeg_ts() {
    let mut muxer = TsMuxer::new(STREAM_TYPE_AAC, 128_000);
    let mut all_bytes = Vec::new();

    let begin_packets = muxer.begin_segment();
    all_bytes.extend(write_packets(&begin_packets));

    let fake_audio = vec![0x21u8; 928];
    for i in 0..10 {
        let pkts = muxer.mux(&fake_audio, i * 100);
        all_bytes.extend(write_packets(&pkts));
    }

    assert_eq!(all_bytes.len() % TS_PACKET_SIZE, 0);
    assert!(all_bytes.len() > TS_PACKET_SIZE * 3);

    let first_byte = all_bytes[0];
    assert_eq!(first_byte, 0x47);

    let temp_dir = tempfile::tempdir().unwrap();
    let ts_path = temp_dir.path().join("test_segment.ts");
    std::fs::write(&ts_path, &all_bytes).unwrap();

    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=format_name",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            ts_path.to_str().unwrap(),
        ])
        .output()
        .expect("ffprobe not found or failed to run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("mpegts"),
        "ffprobe did not detect mpegts format. Got: {stdout}"
    );
}

#[test]
fn test_ts_segment_has_pat_and_pmt() {
    let mut muxer = TsMuxer::new(STREAM_TYPE_AAC, 128_000);
    let begin_packets = muxer.begin_segment();

    let has_pat = begin_packets.iter().any(|p| p.pid == 0x0000);
    let has_pmt = begin_packets.iter().any(|p| p.pid == 0x0100);
    assert!(has_pat, "segment must contain PAT");
    assert!(has_pmt, "segment must contain PMT");

    let fake_audio = vec![0x21u8; 928];
    let data_packets = muxer.mux(&fake_audio, 100);

    let has_audio = data_packets.iter().any(|p| p.pid == 0x0101);
    assert!(has_audio, "segment must contain audio packets (PID 0x0101)");
}

#[test]
fn test_mp3_ts_segment_is_valid() {
    let mut muxer = TsMuxer::new(STREAM_TYPE_MP3, 128_000);
    let mut all_bytes = Vec::new();

    let begin_packets = muxer.begin_segment();
    all_bytes.extend(write_packets(&begin_packets));

    let fake_mp3_frames = build_fake_mp3_frames(10);
    for (i, frame) in fake_mp3_frames.iter().enumerate() {
        let pkts = muxer.mux(frame, i as u64 * 100);
        all_bytes.extend(write_packets(&pkts));
    }

    assert_eq!(all_bytes.len() % TS_PACKET_SIZE, 0);

    let temp_dir = tempfile::tempdir().unwrap();
    let ts_path = temp_dir.path().join("test_mp3_segment.ts");
    std::fs::write(&ts_path, &all_bytes).unwrap();

    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=format_name",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            ts_path.to_str().unwrap(),
        ])
        .output()
        .expect("ffprobe not found");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("mpegts"),
        "ffprobe did not detect mpegts format for MP3. Got: {stdout}"
    );
}

/// Build fake MP3 frames that look valid enough to pass the sync check.
/// Each frame: 0xFF 0xFB 0x90 0x00 + padding to 417 bytes (matching 128kbps CBR frame size).
fn build_fake_mp3_frames(count: usize) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    for _ in 0..count {
        let mut frame = vec![0xFF, 0xFB, 0x90, 0x00];
        frame.resize(417, 0x00);
        frames.push(frame);
    }
    frames
}
