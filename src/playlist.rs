use std::fmt::Write;

#[derive(Debug, Clone)]
pub struct PlaylistSegment {
    pub duration_sec: f64,
    pub filename: String,
}

#[derive(Debug, Clone)]
pub struct Playlist {
    pub segments: Vec<PlaylistSegment>,
    pub target_duration_sec: u64,
    pub media_sequence: u64,
    pub is_live: bool,
}

impl Playlist {
    pub fn new(target_duration_sec: u64, is_live: bool) -> Self {
        Self {
            segments: Vec::new(),
            target_duration_sec,
            media_sequence: 0,
            is_live,
        }
    }

    /// Generate the .m3u8 playlist content.
    pub fn to_m3u8(&self) -> String {
        let mut buf = String::new();

        buf.push_str("#EXTM3U\n");
        buf.push_str("#EXT-X-VERSION:3\n");
        let _ = writeln!(buf, "#EXT-X-TARGETDURATION:{}", self.target_duration_sec);
        if self.is_live && !self.segments.is_empty() {
            let _ = writeln!(buf, "#EXT-X-MEDIA-SEQUENCE:{}", self.media_sequence);
        }

        for seg in &self.segments {
            let _ = writeln!(buf, "#EXTINF:{:.3},", seg.duration_sec);
            buf.push_str(&seg.filename);
            buf.push('\n');
        }

        if !self.is_live {
            buf.push_str("#EXT-X-ENDLIST\n");
        }

        buf
    }

    pub fn add_segment(&mut self, filename: String, duration_sec: f64) {
        self.segments.push(PlaylistSegment {
            duration_sec,
            filename,
        });
    }

    /// In live mode, remove old segments beyond the sliding window.
    pub fn trim_to_window(&mut self, max_segments: usize) -> usize {
        let removed = if self.segments.len() > max_segments {
            let excess = self.segments.len() - max_segments;
            self.segments.drain(0..excess).count()
        } else {
            0
        };
        self.media_sequence += removed as u64;
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vod_playlist_has_endlist() {
        let mut pl = Playlist::new(10, false);
        pl.add_segment("seg-0.ts".to_string(), 10.0);
        let m3u8 = pl.to_m3u8();
        assert!(m3u8.contains("#EXT-X-ENDLIST"));
    }

    #[test]
    fn test_live_playlist_no_endlist() {
        let mut pl = Playlist::new(10, true);
        pl.add_segment("seg-0.ts".to_string(), 10.0);
        let m3u8 = pl.to_m3u8();
        assert!(!m3u8.contains("#EXT-X-ENDLIST"));
    }

    #[test]
    fn test_playlist_contains_extinf() {
        let mut pl = Playlist::new(10, false);
        pl.add_segment("seg-0.ts".to_string(), 5.5);
        let m3u8 = pl.to_m3u8();
        assert!(m3u8.contains("#EXTINF:5.500,"));
    }

    #[test]
    fn test_trim_to_window() {
        let mut pl = Playlist::new(10, true);
        for i in 0..7 {
            pl.add_segment(format!("seg-{}.ts", i), 10.0);
        }
        let removed = pl.trim_to_window(5);
        assert_eq!(removed, 2);
        assert_eq!(pl.segments.len(), 5);
        assert_eq!(pl.media_sequence, 2);
    }
}
