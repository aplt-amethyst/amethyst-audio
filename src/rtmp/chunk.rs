use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub const RTMP_CHUNK_SIZE: usize = 128;

pub struct ChunkReader;

impl ChunkReader {
    pub async fn read_chunk(stream: &mut TcpStream) -> anyhow::Result<Chunk> {
        let header = stream.read_u8().await?;
        let fmt = (header >> 6) & 0x03;
        let mut cs_id = (header & 0x3F) as u32;

        if cs_id == 0 {
            cs_id = stream.read_u8().await? as u32 + 64;
        } else if cs_id == 1 {
            let b1 = stream.read_u8().await? as u32;
            let b2 = stream.read_u8().await? as u32;
            cs_id = b1 * 256 + b2 + 64;
        }

        let ts_delta: u32;

        let msg_length: u32;
        let msg_type_id: u8;

        match fmt {
            0 => {
                let ts = read_u24(stream).await?;
                msg_length = read_u24(stream).await?;
                msg_type_id = stream.read_u8().await?;
                let msg_stream_id = read_u32_le(stream).await?;
                ts_delta = if ts == 0xFFFFFF {
                    stream.read_u32().await?
                } else {
                    ts
                };
                let _ = msg_stream_id;
            }
            1 => {
                let ts = read_u24(stream).await?;
                msg_length = read_u24(stream).await?;
                msg_type_id = stream.read_u8().await?;
                ts_delta = if ts == 0xFFFFFF {
                    stream.read_u32().await?
                } else {
                    ts
                };
            }
            2 => {
                let ts = read_u24(stream).await?;
                ts_delta = if ts == 0xFFFFFF {
                    stream.read_u32().await?
                } else {
                    ts
                };
                msg_length = 0;
                msg_type_id = 0;
            }
            3 => {
                msg_length = 0;
                msg_type_id = 0;
                ts_delta = 0;
            }
            _ => anyhow::bail!("invalid chunk fmt: {fmt}"),
        }

        Ok(Chunk {
            fmt,
            cs_id,
            ts_delta,
            msg_length,
            msg_type_id,
            payload: Vec::new(),
        })
    }
}

pub struct ChunkWriter {
    pub chunk_stream_id: u32,
    pub max_chunk_size: usize,
}

impl ChunkWriter {
    pub fn new(chunk_stream_id: u32) -> Self {
        Self {
            chunk_stream_id,
            max_chunk_size: RTMP_CHUNK_SIZE,
        }
    }

    pub async fn write_message(
        &self,
        stream: &mut TcpStream,
        msg_type_id: u8,
        payload: &[u8],
        timestamp: u32,
    ) -> anyhow::Result<()> {
        let mut offset = 0;
        let total = payload.len() as u32;
        let mut first = true;

        while offset < payload.len() {
            let remaining = payload.len() - offset;
            let chunk_data_len = self.max_chunk_size.min(remaining);
            let chunk = &payload[offset..offset + chunk_data_len];
            offset += chunk_data_len;

            let ts_field = if timestamp >= 0xFFFFFF {
                0xFFFFFFu32
            } else {
                timestamp
            };

            let fmt = if first { 0u8 } else { 3u8 };

            self.write_chunk_header(stream, fmt, ts_field, total, msg_type_id, chunk.len() as u32).await?;

            stream.write_all(chunk).await?;

            if ts_field == 0xFFFFFF {
                stream.write_u32(timestamp).await?;
            }

            first = false;
        }

        Ok(())
    }

    async fn write_chunk_header(
        &self,
        stream: &mut TcpStream,
        fmt: u8,
        ts_field: u32,
        msg_length: u32,
        msg_type_id: u8,
        chunk_data_len: u32,
    ) -> anyhow::Result<()> {
        let cs_id = self.chunk_stream_id;
        let header_byte = (fmt << 6) | if cs_id < 64 { cs_id as u8 } else if cs_id < 320 {
            stream.write_all(&[0u8]).await?;
            0
        } else {
            stream.write_all(&[1u8]).await?;
            0
        };

        if cs_id < 64 {
            stream.write_u8(header_byte).await?;
        } else if cs_id < 320 {
            stream.write_u8((cs_id - 64) as u8).await?;
        } else {
            let id = (cs_id - 64) as u16;
            stream.write_u16(id).await?;
        }

        if fmt <= 2 {
            write_u24(stream, ts_field).await?;
        }
        if fmt <= 1 {
            write_u24(stream, msg_length).await?;
            stream.write_u8(msg_type_id).await?;
        }
        if fmt == 0 {
            stream.write_u32_le(1).await?;
        }
        let _ = chunk_data_len;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub fmt: u8,
    pub cs_id: u32,
    pub ts_delta: u32,
    pub msg_length: u32,
    pub msg_type_id: u8,
    pub payload: Vec<u8>,
}

async fn read_u24(stream: &mut TcpStream) -> anyhow::Result<u32> {
    let mut buf = [0u8; 3];
    stream.read_exact(&mut buf).await?;
    Ok(u32::from_be_bytes([0, buf[0], buf[1], buf[2]]))
}

async fn read_u32_le(stream: &mut TcpStream) -> anyhow::Result<u32> {
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf).await?;
    Ok(u32::from_le_bytes(buf))
}

async fn write_u24(stream: &mut TcpStream, val: u32) -> anyhow::Result<()> {
    let bytes = val.to_be_bytes();
    stream.write_all(&bytes[1..]).await?;
    Ok(())
}
