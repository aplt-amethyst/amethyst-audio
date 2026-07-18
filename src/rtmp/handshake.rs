use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const HANDSHAKE_SIZE: usize = 1536;
const RTMP_VERSION: u8 = 3;

pub async fn run_server_handshake(stream: &mut TcpStream) -> anyhow::Result<()> {
    let mut c0 = [0u8; 1];
    stream.read_exact(&mut c0).await?;

    stream.write_all(&[RTMP_VERSION]).await?;

    let mut c1 = [0u8; HANDSHAKE_SIZE];
    stream.read_exact(&mut c1).await?;

    let s1 = generate_random_1536();
    stream.write_all(&s1).await?;

    let mut c2 = [0u8; HANDSHAKE_SIZE];
    stream.read_exact(&mut c2).await?;

    let s2 = if c2 == s1 {
        c1
    } else {
        s1
    };
    stream.write_all(&s2).await?;

    Ok(())
}

fn generate_random_1536() -> [u8; HANDSHAKE_SIZE] {
    let mut data = [0u8; HANDSHAKE_SIZE];
    for i in 0..8 {
        let ts = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u32)
            .to_be_bytes();
        data[i] = ts[i % 4];
    }
    for i in 8..HANDSHAKE_SIZE {
        data[i] = (i % 256) as u8;
    }
    data
}
