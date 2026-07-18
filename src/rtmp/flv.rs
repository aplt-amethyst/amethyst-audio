pub fn extract_aac_from_flv_tag(tag_data: &[u8]) -> Option<Vec<u8>> {
    if tag_data.is_empty() {
        return None;
    }

    let byte0 = tag_data[0];
    let sound_format = (byte0 >> 4) & 0x0F;
    let _sound_rate = (byte0 >> 2) & 0x03;
    let _sound_size = (byte0 >> 1) & 0x01;
    let _sound_type = byte0 & 0x01;

    if sound_format != 10 {
        return None;
    }

    if tag_data.len() < 2 {
        return None;
    }

    let aac_packet_type = tag_data[1];

    let aac_data = &tag_data[2..];

    if aac_packet_type == 0 {
        let mut adts_header = Vec::new();
        let profile = aac_data.first().copied()?;
        let sample_rate_idx = (profile >> 2) & 0x0F;
        let channel_config = ((profile << 2) & 0x04) | (aac_data.get(1).copied().unwrap_or(0) >> 6);
        let _sr = sample_rate_idx_to_hz(sample_rate_idx as u32);

        let adts_len = (aac_data.len() + 7) as u32;
        adts_header.extend_from_slice(&[0xFF, 0xF9]);
        let profile_byte = ((profile.wrapping_sub(1)) << 6) | (sample_rate_idx << 2) | ((channel_config >> 2) & 0x01);
        adts_header.push(profile_byte);
        let cfg_byte = ((channel_config & 0x03) << 6) | ((adts_len >> 11) as u8 & 0x03);
        adts_header.push(cfg_byte);
        adts_header.push(((adts_len >> 3) & 0xFF) as u8);
        adts_header.push(((adts_len & 0x07) << 5) as u8 | 0x1F);
        adts_header.push(0xFC);

        return Some(adts_header);
    }

    if aac_packet_type == 1 {
        return Some(aac_data.to_vec());
    }

    None
}

fn sample_rate_idx_to_hz(idx: u32) -> u32 {
    match idx {
        0 => 96000,
        1 => 88200,
        2 => 64000,
        3 => 48000,
        4 => 44100,
        5 => 32000,
        6 => 24000,
        7 => 22050,
        8 => 16000,
        9 => 12000,
        10 => 11025,
        11 => 8000,
        _ => 44100,
    }
}
