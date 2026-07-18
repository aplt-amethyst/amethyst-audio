use std::sync::Arc;

use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::auth::AuthState;
use crate::auth::models::UserRole;
use crate::service::HlsService;

use super::amf0::{encode_on_status, encode_result_command};
use super::chunk::{ChunkReader, ChunkWriter, RTMP_CHUNK_SIZE};
use super::flv::extract_aac_from_flv_tag;
use super::handshake::run_server_handshake;
use super::messages::*;

#[allow(dead_code)]
enum SessionState {
    Handshake,
    Connected,
    Ready { stream_id: u32 },
    Publishing { stream_id: u32, source_id: String },
}

pub async fn run_session(
    mut stream: TcpStream,
    service: Arc<HlsService>,
    auth: Option<Arc<AuthState>>,
) -> anyhow::Result<()> {
    run_server_handshake(&mut stream).await?;
    debug!("RTMP handshake complete");

    let mut state = SessionState::Handshake;
    let control_writer = ChunkWriter::new(2);
    let _audio_writer = ChunkWriter::new(4);

    let mut chunk_size_sent = false;
    let mut window_ack_sent = false;
    let mut peer_bw_sent = false;

    let partial_chunks: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let aac_sequence_header: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let mut _audio_ts: u32 = 0;

    loop {
        let chunk = ChunkReader::read_chunk(&mut stream).await?;

        match chunk.msg_type_id {
            MSG_TYPE_SET_CHUNK_SIZE => {
                if chunk.payload.len() >= 4 {
                    let _ = u32::from_be_bytes([chunk.payload[0], chunk.payload[1], chunk.payload[2], chunk.payload[3]]);
                }
            }
            MSG_TYPE_WINDOW_ACK_SIZE => {}
            MSG_TYPE_USER_CONTROL => {}
            MSG_TYPE_AMF0_COMMAND | MSG_TYPE_AMF3_COMMAND => {
                if let Some(cmd) = parse_command(&chunk.payload) {
                    match cmd {
                        RtmpCommand::Connect { transaction_id, app, tc_url: _ } => {
                            if !window_ack_sent {
                                control_writer.write_message(&mut stream, MSG_TYPE_WINDOW_ACK_SIZE, &5000000u32.to_be_bytes(), 0).await?;
                                window_ack_sent = true;
                            }
                            if !peer_bw_sent {
                                control_writer.write_message(&mut stream, MSG_TYPE_SET_PEER_BW, &[0x00, 0x4C, 0x4B, 0x40, 0x02], 0).await?;
                                peer_bw_sent = true;
                            }
                            if !chunk_size_sent {
                                control_writer.write_message(&mut stream, MSG_TYPE_SET_CHUNK_SIZE, &(RTMP_CHUNK_SIZE as u32).to_be_bytes(), 0).await?;
                                chunk_size_sent = true;
                            }

                            let result = encode_result_command(transaction_id);
                            control_writer.write_message(&mut stream, MSG_TYPE_AMF0_COMMAND, &result, 0).await?;
                            state = SessionState::Connected;
                            info!(app = %app, "RTMP connect accepted");
                        }
                        RtmpCommand::CreateStream { transaction_id } => {
                            let stream_id: u32 = 1;
                            let mut buf = Vec::new();
                            super::amf0::encode_amf0_to(&mut buf, &super::amf0::Amf0::String("_result".to_string()));
                            super::amf0::encode_amf0_to(&mut buf, &super::amf0::Amf0::Number(transaction_id));
                            super::amf0::encode_amf0_to(&mut buf, &super::amf0::Amf0::Null);
                            super::amf0::encode_amf0_to(&mut buf, &super::amf0::Amf0::Number(stream_id as f64));
                            control_writer.write_message(&mut stream, MSG_TYPE_AMF0_COMMAND, &buf, 0).await?;
                            state = SessionState::Ready { stream_id };
                        }
                        RtmpCommand::Publish { transaction_id, stream_name, stream_type: _ } => {
                            if let SessionState::Ready { stream_id } = state {
                                let (source_id, auth_param) = parse_stream_key(&stream_name);

                                let auth_result = verify_rtmp_auth(
                                    &service, &auth, &source_id, auth_param.as_deref(),
                                ).await;

                                let is_public_ingest = service
                                    .get_source(&source_id)
                                    .await
                                    .map(|s| s.is_public_ingest)
                                    .unwrap_or(false);

                                if !is_public_ingest && !auth_result {
                                    warn!(source_id = %source_id, "RTMP publish rejected: unauthorized");
                                    let on_status = encode_on_status(
                                        "error",
                                        "NetStream.Publish.BadName",
                                        "unauthorized stream key",
                                    );
                                    control_writer.write_message(&mut stream, MSG_TYPE_AMF0_COMMAND, &on_status, 0).await?;
                                    anyhow::bail!("RTMP publish rejected: unauthorized");
                                }

                                let existing = service.list_sources().await;
                                let found = existing.iter().any(|s| s.id == source_id);

                                if !found {
                                    service
                                        .register_live_source(
                                            source_id.clone(),
                                            128_000,
                                            String::new(),
                                            String::new(),
                                            true,
                                            true,
                                        )
                                        .await;
                                    info!(source_id = %source_id, "auto-created live source from RTMP publish");
                                }

                                let on_status = encode_on_status("status", "NetStream.Publish.Start", "success");
                                control_writer.write_message(&mut stream, MSG_TYPE_AMF0_COMMAND, &on_status, 0).await?;
                                state = SessionState::Publishing { stream_id, source_id };
                                let _ = transaction_id;
                            }
                        }
                        RtmpCommand::FCPublish { .. }
                        | RtmpCommand::ReleaseStream { .. } => {
                            let on_status = encode_on_status("status", "NetStream.Publish.Start", "FCPublish accepted");
                            control_writer.write_message(&mut stream, MSG_TYPE_AMF0_COMMAND, &on_status, 0).await?;
                        }
                        _ => {}
                    }
                }
            }
            MSG_TYPE_AUDIO => {
                if let SessionState::Publishing { ref source_id, .. } = state {
                    let mut buffer = partial_chunks.lock().await;
                    buffer.extend_from_slice(&chunk.payload);

                    let audio_data = if let Some(aac) = extract_aac_from_flv_tag(&buffer) {
                        buffer.clear();
                        Some(aac)
                    } else if buffer.len() > 65536 {
                        let fallback = buffer.clone();
                        buffer.clear();
                        warn!(source_id = %source_id, "FLV buffer overflow, discarding");
                        Some(fallback)
                    } else {
                        None
                    };

                    if let Some(aac_data) = audio_data {
                        let mut seq = aac_sequence_header.lock().await;
                        if seq.is_none() {
                            *seq = Some(aac_data.clone());
                            continue;
                        }

                        _audio_ts += 23;
                        if let Err(e) = service.ingest_chunk(source_id, &aac_data).await {
                            warn!(source_id = %source_id, error = %e, "RTMP ingest failed");
                        }
                    }
                }
            }
            MSG_TYPE_ABORT | MSG_TYPE_ACK => {}
            _ => {}
        }
    }
}

fn parse_stream_key(raw: &str) -> (String, Option<String>) {
    if let Some(pos) = raw.find('?') {
        let source_id = raw[..pos].to_string();
        let query = &raw[pos + 1..];
        let mut pwd = None;
        for part in query.split('&') {
            if let Some(v) = part.strip_prefix("pwd=") {
                pwd = Some(format!("pwd={v}"));
            } else if let Some(v) = part.strip_prefix("token=") {
                pwd = Some(format!("token={v}"));
            }
        }
        (source_id, pwd)
    } else {
        (raw.to_string(), None)
    }
}

async fn verify_rtmp_auth(
    service: &HlsService,
    auth: &Option<Arc<AuthState>>,
    source_id: &str,
    auth_param: Option<&str>,
) -> bool {
    let auth_state = match auth {
        Some(a) => a,
        None => return true,
    };

    let Some(source) = service.get_source(source_id).await else {
        return false;
    };

    let Some(param) = auth_param else {
        return source.is_public_ingest;
    };

    if let Some(pwd) = param.strip_prefix("pwd=") {
        if source.push_password_hash.is_empty() {
            return false;
        }
        return auth_state
            .verify_password(pwd, &source.push_password_hash)
            .unwrap_or(false);
    }

    if let Some(token) = param.strip_prefix("token=") {
        return match auth_state.verify_jwt(token) {
            Ok(claims) => {
                let users = auth_state.users.read().await;
                if let Some(user) = users.get(&claims.sub) {
                    if user.role == UserRole::Admin {
                        return true;
                    }
                    return source.owner_id == user.id;
                }
                false
            }
            Err(_) => false,
        };
    }

    false
}
