use super::packet::{AdaptationFieldControl, TsPacket};
use super::pat::compute_crc32;
use super::{PMT_PID, PMT_TABLE_ID, STREAM_TYPE_AAC};

const AAC_DESC_TAG: u8 = 0x2C;

fn build_aac_descriptor() -> Vec<u8> {
    let audio_object_type: u16 = 2;
    let sample_rate_idx: u16 = 4;
    let channel_config: u16 = 2;
    let asc: u16 = (audio_object_type << 11) | (sample_rate_idx << 7) | (channel_config << 3);
    vec![AAC_DESC_TAG, 0x02, (asc >> 8) as u8, asc as u8]
}

pub fn build_pmt(
    program_number: u16,
    pcr_pid: u16,
    stream_type: u8,
    elementary_pid: u16,
) -> TsPacket {
    let mut payload = Vec::new();

    payload.push(PMT_TABLE_ID);

    let section_syntax: u8 = 0x80;
    let reserved: u8 = 0x30;
    payload.push(section_syntax | reserved);

    let prog_num_high = (program_number >> 8) as u8;
    let prog_num_low = (program_number & 0xFF) as u8;
    payload.push(prog_num_high);
    payload.push(prog_num_low);

    payload.push(0xC1);
    let section_number = 0u8;
    payload.push(section_number);
    let last_section_number = 0u8;
    payload.push(last_section_number);

    let reserved2: u8 = 0xE0;
    let pcr_pid_high = ((pcr_pid >> 8) & 0x1F) as u8;
    let pcr_pid_low = (pcr_pid & 0xFF) as u8;
    payload.push(reserved2 | pcr_pid_high);
    payload.push(pcr_pid_low);

    let reserved3: u8 = 0xF0;
    let program_info_length = 0u16;
    payload.push(reserved3 | ((program_info_length >> 8) & 0x0F) as u8);
    payload.push((program_info_length & 0xFF) as u8);

    payload.push(stream_type);

    let reserved4: u8 = 0xE0;
    let elem_pid_high = ((elementary_pid >> 8) & 0x1F) as u8;
    let elem_pid_low = (elementary_pid & 0xFF) as u8;
    payload.push(reserved4 | elem_pid_high);
    payload.push(elem_pid_low);

    let es_info: Vec<u8> = if stream_type == STREAM_TYPE_AAC {
        build_aac_descriptor()
    } else {
        Vec::new()
    };
    let es_info_length = es_info.len() as u16;
    let reserved5: u8 = 0xF0;
    payload.push(reserved5 | ((es_info_length >> 8) & 0x0F) as u8);
    payload.push((es_info_length & 0xFF) as u8);
    payload.extend_from_slice(&es_info);

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
        pid: PMT_PID,
        continuity_counter: 0,
        payload_unit_start_indicator: true,
        adaptation_field_control: AdaptationFieldControl::PayloadOnly,
        adaptation_field: None,
        payload: final_payload,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pmt_has_sync_byte() {
        let pmt = build_pmt(1, 0x0101, 0x0F, 0x0101);
        let bytes = pmt.to_bytes();
        assert_eq!(bytes[0], 0x47);
    }

    #[test]
    fn test_pmt_has_correct_pid() {
        let pmt = build_pmt(1, 0x0101, 0x0F, 0x0101);
        assert_eq!(pmt.pid, PMT_PID);
    }

    #[test]
    fn test_pmt_contains_stream_type() {
        let pmt = build_pmt(1, 0x0101, 0x0F, 0x0101);
        let has_stream_type = pmt.payload.iter().any(|b| *b == 0x0F);
        assert!(has_stream_type);
    }

    #[test]
    fn test_pmt_payload_unit_start() {
        let pmt = build_pmt(1, 0x0101, 0x0F, 0x0101);
        assert!(pmt.payload_unit_start_indicator);
    }

    #[test]
    fn test_pmt_aac_descriptor_present() {
        let pmt = build_pmt(1, 0x0101, 0x0F, 0x0101);
        let bytes = pmt.to_bytes();
        assert!(
            bytes.windows(2).any(|w| w == [0x2C, 0x02]),
            "AAC descriptor tag missing"
        );
    }
}
