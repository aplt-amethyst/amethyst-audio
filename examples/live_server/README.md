# Live Server Example

Start a live streaming HLS server with a sliding window playlist.

## Usage

```bash
cargo run --example live_server
```

1. Register a live audio source:
```bash
curl -X POST http://localhost:3001/api/sources \
  -H "Content-Type: application/json" \
  -d '{"id": "radio", "file_path": "/path/to/live-feed.aac"}'
```

2. Access the live playlist:
```bash
curl http://localhost:3001/streams/level/radio/playlist.m3u8
```

3. The playlist will NOT include `#EXT-X-ENDLIST` (sliding window). 
   Older segments are auto-deleted as new ones arrive.

## Expected Output

```
#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:4
#EXT-X-MEDIA-SEQUENCE:3
#EXTINF:4.000,
radio-0003.ts
#EXTINF:4.000,
radio-0004.ts
#EXTINF:4.000,
radio-0005.ts
#EXTINF:4.000,
radio-0006.ts
#EXTINF:4.000,
radio-0007.ts
```

Play in VLC: `http://localhost:3001/streams/level/radio/playlist.m3u8`
