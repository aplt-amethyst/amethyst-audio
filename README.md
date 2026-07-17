# amethyst-audio

HLS stream server with pure Rust MPEG-TS muxer. Serves audio over HTTP Live Streaming (.m3u8) with MPEG-2 Transport Stream (.ts) segments compliant with ISO/IEC 13818-1.

## Features

- **Pure MPEG-TS muxer** — zero external dependency for transport stream generation (188-byte packets, PAT, PMT, PES, PCR)
- **VOD & Live** — on-demand playback from static audio files or live streaming with a sliding window
- **Multi-format** — direct remux for AAC (ADTS) and MP3; WAV/FLAC via ffmpeg transcoding
- **HLS compliant** — .m3u8 playlists per RFC 8216 with EXTINF, EXT-X-TARGETDURATION, EXT-X-ENDLIST
- **Observable** — JSON structured logging, `/health` endpoint, graceful SIGTERM shutdown
- **CPS compliant** — follows CNT Programming Standards v0.1.0-4b

## Quick Start

```bash
make build
make run
```

Serve a local AAC file:

```
http://localhost:3000/streams/level/my-audio/playlist.m3u8
```

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Health check |
| `GET` | `/streams/level/{id}/playlist.m3u8` | HLS playlist |
| `GET` | `/streams/level/{id}/{segment}.ts` | TS segment |
| `POST` | `/api/sources` | Register audio source |

## License

AGPL-3.0 — see [LICENSE](LICENSE)
