use std::collections::BTreeMap;

pub type Amf0Value = Amf0;

#[derive(Debug, Clone, PartialEq)]
pub enum Amf0 {
    Number(f64),
    Boolean(bool),
    String(String),
    Object(BTreeMap<String, Amf0>),
    Null,
}

impl Amf0 {
    pub fn marker(&self) -> u8 {
        match self {
            Amf0::Number(_) => 0x00,
            Amf0::Boolean(_) => 0x01,
            Amf0::String(_) => 0x02,
            Amf0::Object(_) => 0x03,
            Amf0::Null => 0x05,
        }
    }
}

pub fn encode_amf0(value: &Amf0) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_amf0_to(&mut buf, value);
    buf
}

pub fn encode_amf0_to(buf: &mut Vec<u8>, value: &Amf0) {
    buf.push(value.marker());
    match value {
        Amf0::Number(n) => buf.extend_from_slice(&n.to_be_bytes()),
        Amf0::Boolean(b) => buf.push(if *b { 1 } else { 0 }),
        Amf0::String(s) => {
            let len = s.len() as u16;
            buf.extend_from_slice(&len.to_be_bytes());
            buf.extend_from_slice(s.as_bytes());
        }
        Amf0::Object(obj) => {
            let mut keys: Vec<&String> = obj.keys().collect();
            keys.sort();
            for key in keys {
                let key_bytes = key.as_bytes();
                let klen = key_bytes.len() as u16;
                buf.extend_from_slice(&klen.to_be_bytes());
                buf.extend_from_slice(key_bytes);
                encode_amf0_to(buf, obj.get(key).unwrap());
            }
            buf.extend_from_slice(&[0x00, 0x00, 0x09]);
        }
        Amf0::Null => {}
    }
}

pub fn decode_amf0<'a>(data: &'a [u8]) -> Option<(Amf0, &'a [u8])> {
    if data.is_empty() {
        return None;
    }
    let marker = data[0];
    let rest = &data[1..];
    match marker {
        0x00 => decode_number(rest),
        0x01 => decode_bool(rest),
        0x02 => decode_string(rest),
        0x03 => decode_object(rest),
        0x05 => Some((Amf0::Null, rest)),
        _ => None,
    }
}

fn decode_number(data: &[u8]) -> Option<(Amf0, &[u8])> {
    if data.len() < 8 {
        return None;
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&data[..8]);
    let n = f64::from_be_bytes(bytes);
    Some((Amf0::Number(n), &data[8..]))
}

fn decode_bool(data: &[u8]) -> Option<(Amf0, &[u8])> {
    if data.is_empty() {
        return None;
    }
    Some((Amf0::Boolean(data[0] != 0), &data[1..]))
}

fn decode_string(data: &[u8]) -> Option<(Amf0, &[u8])> {
    if data.len() < 2 {
        return None;
    }
    let len = u16::from_be_bytes([data[0], data[1]]) as usize;
    if data.len() < 2 + len {
        return None;
    }
    let s = String::from_utf8(data[2..2 + len].to_vec()).ok()?;
    Some((Amf0::String(s), &data[2 + len..]))
}

fn decode_object(data: &[u8]) -> Option<(Amf0, &[u8])> {
    let mut obj = BTreeMap::new();
    let mut cursor = data;
    loop {
        if cursor.len() < 3 {
            return None;
        }
        if cursor[0] == 0x00 && cursor[1] == 0x00 && cursor[2] == 0x09 {
            return Some((Amf0::Object(obj), &cursor[3..]));
        }
        let (key, rest) = decode_string(cursor)?;
        let key_str = match key {
            Amf0::String(s) => s,
            _ => return None,
        };
        let (val, rest) = decode_amf0(rest)?;
        obj.insert(key_str, val);
        cursor = rest;
    }
}

pub fn encode_connect_command(app: &str, tc_url: &str) -> Vec<u8> {
    let mut obj = BTreeMap::new();
    obj.insert("app".to_string(), Amf0::String(app.to_string()));
    obj.insert("type".to_string(), Amf0::String("nonprivate".to_string()));
    obj.insert("tcUrl".to_string(), Amf0::String(tc_url.to_string()));
    obj.insert("flashVer".to_string(), Amf0::String("FMLE/3.0 (compatible; FMSc/1.0)".to_string()));

    let command = Amf0::String("connect".to_string());
    let transaction_id = Amf0::Number(1.0);
    let cmd_obj = Amf0::Object(obj);

    let mut buf = Vec::new();
    encode_amf0_to(&mut buf, &command);
    encode_amf0_to(&mut buf, &transaction_id);
    encode_amf0_to(&mut buf, &cmd_obj);
    buf
}

pub fn encode_result_command(transaction_id: f64) -> Vec<u8> {
    let mut info = BTreeMap::new();
    info.insert("code".to_string(), Amf0::String("NetConnection.Connect.Success".to_string()));
    info.insert("level".to_string(), Amf0::String("status".to_string()));

    let mut buf = Vec::new();
    encode_amf0_to(&mut buf, &Amf0::String("_result".to_string()));
    encode_amf0_to(&mut buf, &Amf0::Number(transaction_id));
    encode_amf0_to(&mut buf, &Amf0::Object(info));
    encode_amf0_to(&mut buf, &Amf0::Object(BTreeMap::new()));
    buf
}

pub fn encode_on_status(level: &str, code: &str, description: &str) -> Vec<u8> {
    let mut info = BTreeMap::new();
    info.insert("level".to_string(), Amf0::String(level.to_string()));
    info.insert("code".to_string(), Amf0::String(code.to_string()));
    info.insert("description".to_string(), Amf0::String(description.to_string()));

    let mut buf = Vec::new();
    encode_amf0_to(&mut buf, &Amf0::String("onStatus".to_string()));
    encode_amf0_to(&mut buf, &Amf0::Number(0.0));
    encode_amf0_to(&mut buf, &Amf0::Null);
    encode_amf0_to(&mut buf, &Amf0::Object(info));
    buf
}
