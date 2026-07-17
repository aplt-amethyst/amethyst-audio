pub mod adts_parser;
pub mod mp3_parser;
pub mod muxer;
pub mod packet;
pub mod pat;
pub mod pes;
pub mod pmt;

pub const TS_PACKET_SIZE: usize = 188;
pub const TS_SYNC_BYTE: u8 = 0x47;
pub const PAT_PID: u16 = 0x0000;
pub const PMT_PID: u16 = 0x0100;
pub const AUDIO_PID: u16 = 0x0101;
pub const PAT_TABLE_ID: u8 = 0x00;
pub const PMT_TABLE_ID: u8 = 0x02;
pub const PAT_INTERVAL_MS: u64 = 500;
pub const PMT_INTERVAL_MS: u64 = 500;
pub const SI_TABLE_ID_PAT: u8 = 0x00;
pub const SI_TABLE_ID_PMT: u8 = 0x02;

pub const STREAM_TYPE_AAC: u8 = 0x0F;
pub const STREAM_TYPE_MP3: u8 = 0x03;

pub const PCR_CLOCK_HZ: u64 = 90_000;

pub const NULL_PID: u16 = 0x1FFF;
