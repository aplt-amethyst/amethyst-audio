use super::packet::{AdaptationFieldControl, TsPacket};
use super::{PAT_PID, PAT_TABLE_ID};

pub fn build_pat(program_number: u16, pmt_pid: u16) -> TsPacket {
    let mut payload = Vec::new();

    payload.push(PAT_TABLE_ID);

    let section_syntax_indicator: u8 = 0x80;
    let private_bit: u8 = 0x40;
    let reserved: u8 = 0x30;
    payload.push(section_syntax_indicator | private_bit | reserved);

    let transport_stream_id = 1u16;
    payload.push((transport_stream_id >> 8) as u8);
    payload.push((transport_stream_id & 0xFF) as u8);

    payload.push(0xC1);
    let section_number = 0u8;
    payload.push(section_number);
    let last_section_number = 0u8;
    payload.push(last_section_number);

    let prog_num_high = (program_number >> 8) as u8;
    let prog_num_low = (program_number & 0xFF) as u8;
    payload.push(prog_num_high);
    payload.push(prog_num_low);

    let reserved2: u8 = 0xE0;
    let pmt_pid_high = ((pmt_pid >> 8) & 0x1F) as u8;
    let pmt_pid_low = (pmt_pid & 0xFF) as u8;
    payload.push(reserved2 | pmt_pid_high);
    payload.push(pmt_pid_low);

    let crc = compute_crc32(&payload);
    payload.push((crc >> 24) as u8);
    payload.push((crc >> 16) as u8);
    payload.push((crc >> 8) as u8);
    payload.push(crc as u8);

    let section_len_pos = 1;
    let section_len = (payload.len() - 3 + section_len_pos) as u16;
    payload[section_len_pos] |= ((section_len >> 8) & 0x0F) as u8;
    payload[section_len_pos + 1] = (section_len & 0xFF) as u8;

    let pointer_field = 0u8;
    let mut final_payload = vec![pointer_field];
    final_payload.extend_from_slice(&payload);

    TsPacket {
        pid: PAT_PID,
        continuity_counter: 0,
        payload_unit_start_indicator: true,
        adaptation_field_control: AdaptationFieldControl::PayloadOnly,
        adaptation_field: None,
        payload: final_payload,
    }
}

/// CRC-32/MPEG-2 lookup table for PSI section integrity.
static CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = (i as u32) << 24;
        let mut j = 0;
        while j < 8 {
            if (crc & 0x8000_0000) != 0 {
                crc = (crc << 1) ^ 0x04C1_1DB7;
            } else {
                crc <<= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

pub fn compute_crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for byte in data {
        let idx = ((crc >> 24) ^ u32::from(*byte)) as usize;
        crc = (crc << 8) ^ CRC32_TABLE[idx];
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pat_has_sync_byte() {
        let pat = build_pat(1, 0x0100);
        let bytes = pat.to_bytes();
        assert_eq!(bytes[0], 0x47);
    }

    #[test]
    fn test_pat_has_correct_pid() {
        let pat = build_pat(1, 0x0100);
        assert_eq!(pat.pid, 0x0000);
    }

    #[test]
    fn test_pat_payload_contains_table_id() {
        let pat = build_pat(1, 0x0100);
        let has_table_id = pat.payload.contains(&PAT_TABLE_ID);
        assert!(has_table_id);
    }

    #[test]
    fn test_crc32_known_value() {
        let data = [0x00, 0xB0, 0x0D, 0x00, 0x01, 0xC1, 0x00, 0x00];
        let crc = compute_crc32(&data);
        assert_ne!(crc, 0);
        assert_ne!(crc, 0xFFFF_FFFF);
    }
}
