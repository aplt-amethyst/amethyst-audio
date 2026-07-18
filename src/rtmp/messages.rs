use super::amf0::*;

pub const MSG_TYPE_SET_CHUNK_SIZE: u8 = 1;
pub const MSG_TYPE_ABORT: u8 = 2;
pub const MSG_TYPE_ACK: u8 = 3;
pub const MSG_TYPE_USER_CONTROL: u8 = 4;
pub const MSG_TYPE_WINDOW_ACK_SIZE: u8 = 5;
pub const MSG_TYPE_SET_PEER_BW: u8 = 6;
pub const MSG_TYPE_AUDIO: u8 = 8;
pub const MSG_TYPE_VIDEO: u8 = 9;
pub const MSG_TYPE_AMF3_DATA: u8 = 15;
pub const MSG_TYPE_AMF3_SHARED: u8 = 16;
pub const MSG_TYPE_AMF3_COMMAND: u8 = 17;
pub const MSG_TYPE_AMF0_DATA: u8 = 18;
pub const MSG_TYPE_AMF0_SHARED: u8 = 19;
pub const MSG_TYPE_AMF0_COMMAND: u8 = 20;

#[derive(Debug)]
pub enum RtmpCommand {
    Connect { transaction_id: f64, app: String, tc_url: String },
    CreateStream { transaction_id: f64 },
    Publish { transaction_id: f64, stream_name: String, stream_type: String },
    ReleaseStream { transaction_id: f64, stream_name: String },
    FCPublish { transaction_id: f64, stream_name: String },
    FCUnpublish { transaction_id: f64, stream_name: String },
    DeleteStream { transaction_id: f64, stream_id: f64 },
    CloseStream,
    Unknown,
}

pub fn parse_command(payload: &[u8]) -> Option<RtmpCommand> {
    let (cmd_name_val, rest) = decode_amf0(payload)?;
    let cmd_name = match cmd_name_val {
        Amf0::String(ref s) => s.clone(),
        _ => return None,
    };

    let (tid_val, rest) = decode_amf0(rest)?;
    let tid = match tid_val {
        Amf0::Number(n) => n,
        _ => return None,
    };

    match cmd_name.as_str() {
        "connect" => {
            let (cmd_obj_val, _rest) = decode_amf0(rest)?;
            let app = match cmd_obj_val {
                Amf0::Object(ref obj) => obj.get("app").and_then(|v| match v {
                    Amf0::String(s) => Some(s.clone()),
                    _ => None,
                }).unwrap_or_default(),
                _ => String::new(),
            };
            let tc_url = String::new();
            Some(RtmpCommand::Connect { transaction_id: tid, app, tc_url })
        }
        "createStream" => {
            Some(RtmpCommand::CreateStream { transaction_id: tid })
        }
        "publish" => {
            let (name_val, rest2) = decode_amf0(rest)?;
            let name = match name_val {
                Amf0::String(ref s) => s.clone(),
                Amf0::Null => String::new(),
                _ => return None,
            };
            let (type_val, _) = decode_amf0(rest2)?;
            let stream_type = match type_val {
                Amf0::String(ref s) => s.clone(),
                _ => "live".to_string(),
            };
            Some(RtmpCommand::Publish { transaction_id: tid, stream_name: name, stream_type })
        }
        "releaseStream" => {
            let (name_val, _) = decode_amf0(rest)?;
            let name = match name_val {
                Amf0::String(ref s) => s.clone(),
                _ => String::new(),
            };
            Some(RtmpCommand::ReleaseStream { transaction_id: tid, stream_name: name })
        }
        "FCPublish" => {
            let (name_val, _) = decode_amf0(rest)?;
            let name = match name_val {
                Amf0::String(ref s) => s.clone(),
                _ => String::new(),
            };
            Some(RtmpCommand::FCPublish { transaction_id: tid, stream_name: name })
        }
        "FCUnpublish" => {
            let (name_val, _) = decode_amf0(rest)?;
            let name = match name_val {
                Amf0::String(ref s) => s.clone(),
                _ => String::new(),
            };
            Some(RtmpCommand::FCUnpublish { transaction_id: tid, stream_name: name })
        }
        "deleteStream" => {
            let (sid_val, _) = decode_amf0(rest)?;
            let sid = match sid_val {
                Amf0::Number(n) => n,
                _ => return None,
            };
            Some(RtmpCommand::DeleteStream { transaction_id: tid, stream_id: sid })
        }
        "closeStream" => {
            Some(RtmpCommand::CloseStream)
        }
        _ => Some(RtmpCommand::Unknown),
    }
}
