use super::TS_PACKET_SIZE;
use super::TS_SYNC_BYTE;

#[derive(Debug, Clone)]
pub struct TsPacket {
    pub pid: u16,
    pub continuity_counter: u8,
    pub payload_unit_start_indicator: bool,
    pub adaptation_field_control: AdaptationFieldControl,
    pub adaptation_field: Option<AdaptationField>,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdaptationFieldControl {
    PayloadOnly,
    AdaptationOnly,
    AdaptationAndPayload,
    Reserved,
}

impl AdaptationFieldControl {
    pub fn to_bits(self) -> u8 {
        match self {
            Self::PayloadOnly => 0b01,
            Self::AdaptationOnly => 0b10,
            Self::AdaptationAndPayload => 0b11,
            Self::Reserved => 0b00,
        }
    }

    pub fn from_bits(bits: u8) -> Self {
        match bits {
            0b01 => Self::PayloadOnly,
            0b10 => Self::AdaptationOnly,
            0b11 => Self::AdaptationAndPayload,
            _ => Self::Reserved,
        }
    }

    pub fn has_payload(self) -> bool {
        matches!(self, Self::PayloadOnly | Self::AdaptationAndPayload)
    }
}

#[derive(Debug, Clone)]
pub struct AdaptationField {
    pub discontinuity_indicator: bool,
    pub random_access_indicator: bool,
    pub pcr_flag: bool,
    pub pcr_value: Option<u64>,
    pub stuffing_bytes: Vec<u8>,
}

impl AdaptationField {
    /// Create an adaptation field containing only a PCR value (pcr_base * 300 + pcr_ext).
    pub fn with_pcr(pcr_90khz: u64) -> Self {
        let pcr_base = pcr_90khz / 300;
        let pcr_ext = (pcr_90khz % 300) as u16;

        Self {
            discontinuity_indicator: false,
            random_access_indicator: false,
            pcr_flag: true,
            pcr_value: Some(Self::encode_pcr_48bit(pcr_base, pcr_ext)),
            stuffing_bytes: Vec::new(),
        }
    }

    /// Encodes a 33-bit PCR base and 9-bit extension into a 48-bit value suitable for the adaptation field.
    fn encode_pcr_48bit(pcr_base: u64, pcr_ext: u16) -> u64 {
        let base = pcr_base & 0x1_FFFF_FFFF;
        let ext = u64::from(pcr_ext) & 0x1FF;
        (base << 15) | (ext << 6) | 0x3F
    }
}

impl TsPacket {
    pub fn new(
        pid: u16,
        continuity_counter: u8,
        payload_unit_start_indicator: bool,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            pid,
            continuity_counter: continuity_counter & 0x0F,
            payload_unit_start_indicator,
            adaptation_field_control: AdaptationFieldControl::PayloadOnly,
            adaptation_field: None,
            payload,
        }
    }

    pub fn with_adaptation(
        pid: u16,
        continuity_counter: u8,
        payload_unit_start_indicator: bool,
        adaptation_field: AdaptationField,
        payload: Vec<u8>,
    ) -> Self {
        let afc = if payload.is_empty() {
            AdaptationFieldControl::AdaptationOnly
        } else {
            AdaptationFieldControl::AdaptationAndPayload
        };

        Self {
            pid,
            continuity_counter: continuity_counter & 0x0F,
            payload_unit_start_indicator,
            adaptation_field_control: afc,
            adaptation_field: Some(adaptation_field),
            payload,
        }
    }

    /// Serialize this packet to a 188-byte buffer.
    ///
    /// # Panics
    ///
    /// Panics if the resulting packet exceeds 188 bytes.
    pub fn to_bytes(&self) -> [u8; TS_PACKET_SIZE] {
        let mut buf = [0xFF; TS_PACKET_SIZE];

        let mut offset = 0;

        buf[offset] = TS_SYNC_BYTE;
        offset += 1;

        let tei = 0u8;
        let pusiz = u8::from(self.payload_unit_start_indicator) << 6;
        let priority = 0u8;
        let pid_high = ((self.pid >> 8) & 0x1F) as u8;
        let pid_low = (self.pid & 0xFF) as u8;
        buf[offset] = tei | pusiz | priority | pid_high;
        offset += 1;
        buf[offset] = pid_low;
        offset += 1;

        let tsc = 0u8;
        let afc = self.adaptation_field_control.to_bits() << 4;
        let cc = self.continuity_counter & 0x0F;
        buf[offset] = tsc | afc | cc;
        offset += 1;

        if let Some(ref af) = self.adaptation_field {
            let af_start = offset;
            offset += 1;
            let len_offset = offset;
            offset += 1;

            let mut af_flags: u8 = 0;
            if af.discontinuity_indicator {
                af_flags |= 0x80;
            }
            if af.random_access_indicator {
                af_flags |= 0x40;
            }
            if af.pcr_flag {
                af_flags |= 0x10;
            }

            buf[af_start] = af_flags;

            if af.pcr_flag {
                if let Some(pcr) = af.pcr_value {
                    buf[offset] = (pcr >> 40) as u8;
                    buf[offset + 1] = (pcr >> 32) as u8;
                    buf[offset + 2] = (pcr >> 24) as u8;
                    buf[offset + 3] = (pcr >> 16) as u8;
                    buf[offset + 4] = (pcr >> 8) as u8;
                    buf[offset + 5] = pcr as u8;
                    offset += 6;
                }
            }

            for byte in &af.stuffing_bytes {
                buf[offset] = *byte;
                offset += 1;
            }

            let af_len = offset - len_offset - 1;
            buf[len_offset] = af_len as u8;
        }

        for (i, byte) in self.payload.iter().enumerate() {
            if offset + i < TS_PACKET_SIZE {
                buf[offset + i] = *byte;
            }
        }

        let payload_end = offset + self.payload.len();
        #[allow(clippy::cast_sign_loss)]
        if payload_end < TS_PACKET_SIZE {
            for byte in buf.iter_mut().skip(payload_end) {
                *byte = 0xFF;
            }
        }

        buf
    }

    /// Create a null packet (PID 0x1FFF) filled with 0xFF.
    pub fn null() -> Self {
        Self {
            pid: super::NULL_PID,
            continuity_counter: 0,
            payload_unit_start_indicator: false,
            adaptation_field_control: AdaptationFieldControl::PayloadOnly,
            adaptation_field: None,
            payload: vec![0xFF; TS_PACKET_SIZE - 4],
        }
    }
}
