use std::sync::Arc;

use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::service::HlsService;

use super::amf0::{encode_result_command, encode_on_status};
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
                                let source_id = stream_name.clone();
                                let mut status = "success";
                                let existing = service.list_sources().await;
                                let found = existing.iter().any(|s| s.id == source_id);

                                if !found {
                                    let pw_hash = String::new();
                                    service
                                        .register_live_source(
                                            source_id.clone(),
                                            128_000,
                                            String::new(),
                                            pw_hash,
                                            true,
                                            true,
                                        )
                                        .await;
                                    info!(source_id = %source_id, "auto-created live source from RTMP publish");
                                } else {
                                    status = "success";
                                }

                                let on_status = encode_on_status("status", "NetStream.Publish.Start", &format!("{status}"));
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
