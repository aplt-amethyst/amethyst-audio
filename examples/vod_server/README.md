# VOD Server Example

Start an on-demand HLS server that segments audio files on registration.

## Usage

```bash
cargo run --example vod_server
```

1. Register an audio source:
```bash
curl -X POST http://localhost:3000/api/sources \
  -H "Content-Type: application/json" \
  -d '{"id": "my-song", "file_path": "/path/to/audio.aac"}'
```

2. Access the HLS playlist:
```bash
curl http://localhost:3000/streams/level/my-song/playlist.m3u8
```

3. The playlist will include `#EXT-X-ENDLIST` (fixed-length media).

## Supported Formats

- `.aac` — AAC ADTS (direct remux, no transcoding)
- `.mp3` — MPEG audio (direct remux, no transcoding)  
- `.wav` — PCM WAV (transcodes via ffmpeg to AAC)
- `.flac` — FLAC (transcodes via ffmpeg to AAC)

## Expected Output

The .m3u8 playlist will look like:
```
#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:10
#EXTINF:10.000,
my-song-0000.ts
#EXTINF:10.000,
my-song-0001.ts
#EXTINF:5.500,
my-song-0002.ts
#EXT-X-ENDLIST
```

TS segments are ISO 13818-1 MPEG-2 Transport Stream files.
