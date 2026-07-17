# amethyst-audio API Documentation

## Architecture

### Layers (CPS compliant)

```
Interface:   axum HTTP (src/server/)
Control:     HLS orchestration (src/service/)
Model:       Playlist generator (src/playlist.rs)
Abstraction: Codec parsers (src/codec/)
Core:        MPEG-TS muxer (src/ts/)
```

### Core: TS Muxer (`src/ts/`)

The pure MPEG-TS muxer generates 188-byte transport stream packets.

| Module | Specification |
|--------|-------------|
| `packet.rs` | ISO 13818-1 2.4.3.2 — TS packet header (sync 0x47, PID 13-bit, continuity_counter) |
| `pat.rs` | ISO 13818-1 2.4.4.3 — Program Association Table (PID 0x0000) |
| `pmt.rs` | ISO 13818-1 2.4.4.8 — Program Map Table (PID 0x0100, stream_type) |
| `pes.rs` | ISO 13818-1 2.4.3.6 — PES packet (PTS 33-bit 90kHz, start code 0x000001) |
| `muxer.rs` | TS segment assembly with PAT/PMT reinsertion every 500ms |

**Constants:**
- `TS_PACKET_SIZE` = 188 bytes
- `TS_SYNC_BYTE` = 0x47
- `PAT_PID` = 0x0000
- `PMT_PID` = 0x0100
- `AUDIO_PID` = 0x0101
- `PCR_CLOCK_HZ` = 90,000
- `STREAM_TYPE_AAC` = 0x0F
- `STREAM_TYPE_MP3` = 0x03

### Codec: Parsers (`src/codec/`)

| Module | Input | Output |
|--------|-------|--------|
| `ts/adts_parser.rs` | ADTS frames (0xFFF sync) | Raw AAC |
| `ts/mp3_parser.rs` | MPEG audio frames (0xFFE sync) | Raw MP3 |
| `codec/wav.rs` | WAV file (via `hound`) | PCM i16 + metadata |
| `codec/flac.rs` | FLAC file (via `claxon`) | PCM i16 + metadata |
| `codec/aac.rs` | PCM i16 s16le | AAC ADTS (via ffmpeg subprocess) |

### Playlist: .m3u8 Generation (`src/playlist.rs`)

Per RFC 8216:
- `#EXTM3U` — header
- `#EXT-X-VERSION:3` — version
- `#EXT-X-TARGETDURATION:<N>` — segment target duration
- `#EXT-X-MEDIA-SEQUENCE:<N>` — live playlist sequence
- `#EXTINF:<duration>,` — per-segment duration
- `#EXT-X-ENDLIST` — VOD only (not present in live)

### Service: HLS Lifecycle (`src/service/`)

- **VOD**: full file → segment on register → static .m3u8 with ENDLIST
- **Live**: register + ingest raw AAC ADTS → accumulate → auto-segment → sliding window .m3u8 without ENDLIST

### Live Ingest Flow

1. Register: `POST /api/sources {"id":"stream","live":true,"bitrate":128000}`
2. Push: `POST /streams/level/stream/ingest` (raw AAC ADTS body)
3. Play: `GET /streams/level/stream/playlist.m3u8`

Server strips ADTS headers → raw AAC → TS muxer → 188-byte packets → .ts segments.

ffmpeg pipe example:
```bash
ffmpeg -i input.wav -c:a aac -b:a 128k -f adts pipe:1 | \
  curl -X POST --data-binary @- http://localhost:3000/streams/level/live/ingest
```

### Server: HTTP API (`src/server/`)

| Method | Path | Content-Type |
|--------|------|-------------|
| `GET` | `/health` | `application/json` |
| `GET` | `/streams/level/{id}/playlist.m3u8` | `application/vnd.apple.mpegurl` |
| `GET` | `/streams/level/{id}/{seg}` | `video/mp2t` |
| `POST` | `/streams/level/{id}/ingest` | `application/octet-stream`, `application/json` (response) |
| `GET` | `/api/sources` | `application/json` |
| `POST` | `/api/sources` | `application/json` |
