//! A game's own theme song, `sce_sys/snd0.at9` in its dump (docs/DESIGN.md section 7.3): an
//! ATRAC9 stream in a RIFF WAVE file. [`load`] reads and checks it; a [`ThemePlayer`] decodes it
//! while it plays, one superframe at a time, so a three-minute song costs its compressed size in
//! memory and not tens of megabytes of PCM. The file is only read; nothing is written near the
//! dump.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use anyplay_atrac9::Decoder;

/// `WAVE_FORMAT_EXTENSIBLE`, the format tag of every ATRAC9 file.
const EXTENSIBLE: u16 = 0xFFFE;
/// The ATRAC9 sub-format GUID as it is stored (little-endian fields).
const ATRAC9_GUID: [u8; 16] = [
    0xD2, 0x42, 0xE1, 0x47, 0xBA, 0x36, 0x8D, 0x4D, 0x88, 0xFC, 0x61, 0x65, 0x4F, 0x8C, 0x83, 0x6C,
];
/// Larger files are not theme songs.
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
/// Superframes [`load`] decodes to check the stream before it is handed to a player.
const PROBE_SUPERFRAMES: usize = 8;

/// A checked theme song: the compressed stream and where it starts, ends and loops. Frame
/// positions count from the first decoded frame, encoder delay included, as the file's own
/// `smpl` loop points do.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeSong {
    pub rate: u32,
    channels: usize,
    config: [u8; 4],
    superframe_size: usize,
    /// Frames one superframe decodes to.
    superframe_frames: usize,
    /// The part that repeats: first frame, and the frame after the last.
    pub loop_frames: (usize, usize),
    /// Where playback begins: after the encoder delay.
    pub start: usize,
    data: Vec<u8>,
}

impl ThemeSong {
    fn superframes(&self) -> usize {
        self.data.len() / self.superframe_size
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
    #[error("cannot read the theme song: {0}")]
    Read(#[from] std::io::Error),
    #[error("the theme song is larger than {} MB", MAX_FILE_BYTES / 1024 / 1024)]
    TooLarge,
    #[error("not an ATRAC9 file: {0}")]
    Format(&'static str),
    #[error("the ATRAC9 stream does not decode: {0}")]
    Decode(String),
}

/// Reads a game's `snd0.at9` and checks that it decodes.
pub fn load(path: &Path) -> Result<ThemeSong, ThemeError> {
    if fs::metadata(path)?.len() > MAX_FILE_BYTES {
        return Err(ThemeError::TooLarge);
    }
    from_bytes(fs::read(path)?)
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The chunks a theme song needs, as byte ranges of the file.
#[derive(Debug, Default)]
struct Chunks {
    fmt: Option<(usize, usize)>,
    fact: Option<(usize, usize)>,
    smpl: Option<(usize, usize)>,
    data: Option<(usize, usize)>,
}

fn chunks(bytes: &[u8]) -> Result<Chunks, ThemeError> {
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(ThemeError::Format("no RIFF WAVE header"));
    }
    let mut found = Chunks::default();
    let mut at = 12;
    while let (Some(id), Some(size)) = (bytes.get(at..at + 4), u32_at(bytes, at + 4)) {
        let start = at + 8;
        let end = start.saturating_add(size as usize).min(bytes.len());
        let range = Some((start, end));
        match id {
            b"fmt " => found.fmt = range,
            b"fact" => found.fact = range,
            b"smpl" => found.smpl = range,
            b"data" => found.data = range,
            _ => {}
        }
        // Chunks are padded to an even length.
        at = end.saturating_add(size as usize & 1);
    }
    Ok(found)
}

/// Checks a whole `.at9` file held in memory and keeps its stream.
pub fn from_bytes(mut bytes: Vec<u8>) -> Result<ThemeSong, ThemeError> {
    let found = chunks(&bytes)?;
    let (fmt_start, fmt_end) = found.fmt.ok_or(ThemeError::Format("no fmt chunk"))?;
    let (data_start, data_end) = found.data.ok_or(ThemeError::Format("no data chunk"))?;
    let fmt = &bytes[fmt_start..fmt_end];
    if u16_at(fmt, 0) != Some(EXTENSIBLE) || fmt.get(24..40) != Some(&ATRAC9_GUID[..]) {
        return Err(ThemeError::Format("the fmt chunk is not ATRAC9"));
    }
    let channels = usize::from(u16_at(fmt, 2).ok_or(ThemeError::Format("short fmt chunk"))?);
    let rate = u32_at(fmt, 4).ok_or(ThemeError::Format("short fmt chunk"))?;
    let config: [u8; 4] = fmt
        .get(44..48)
        .and_then(|c| c.try_into().ok())
        .ok_or(ThemeError::Format("no ATRAC9 configuration"))?;
    if channels == 0 || rate == 0 {
        return Err(ThemeError::Format("no channels or no sample rate"));
    }
    let info = Decoder::new(config)
        .map_err(|e| ThemeError::Decode(e.to_string()))?
        .info();
    if info.channels != channels {
        return Err(ThemeError::Format(
            "channel count differs from the configuration",
        ));
    }
    let superframe_frames = info.frame_samples * info.frames_in_superframe;
    if info.superframe_size == 0 || superframe_frames == 0 {
        return Err(ThemeError::Format("empty superframes"));
    }
    let decoded = (data_end - data_start) / info.superframe_size * superframe_frames;
    // `fact`: the frame count without the delay, then (for ATRAC9) two words of which the second
    // is the encoder delay; the file's loop points count the delay in.
    let fact = found.fact.map(|(s, e)| &bytes[s..e]);
    let delay = fact
        .and_then(|f| u32_at(f, 8))
        .filter(|&d| (d as usize) < decoded)
        .unwrap_or(0) as usize;
    let end = fact
        .and_then(|f| u32_at(f, 0))
        .map(|total| (delay + total as usize).min(decoded))
        .filter(|&end| end > delay)
        .unwrap_or(decoded);
    // `smpl`: the first loop's start and last frame.
    let looped = found.smpl.and_then(|(s, e)| {
        let smpl = &bytes[s..e];
        (u32_at(smpl, 28)? > 0).then_some(())?;
        let first = u32_at(smpl, 36 + 8)? as usize;
        let last = u32_at(smpl, 36 + 12)? as usize;
        Some((first, (last + 1).min(end)))
    });
    let loop_frames = choose_loop(looped, (delay, end), rate);
    if delay >= end {
        return Err(ThemeError::Format("no audio after the encoder delay"));
    }
    bytes.truncate(data_end);
    bytes.drain(..data_start);
    let song = ThemeSong {
        rate,
        channels,
        config,
        superframe_size: info.superframe_size,
        superframe_frames,
        loop_frames,
        start: delay,
        data: bytes,
    };
    probe(&song)?;
    Ok(song)
}

/// The file's own loop, unless it is missing or under a second (it would stutter); then the
/// whole song.
fn choose_loop(looped: Option<(usize, usize)>, song: (usize, usize), rate: u32) -> (usize, usize) {
    match looped {
        Some((first, after)) if after.saturating_sub(first) >= rate as usize => (first, after),
        _ => song,
    }
}

/// Decodes the first superframes once, so a broken stream is found before it plays.
fn probe(song: &ThemeSong) -> Result<(), ThemeError> {
    let mut decoder = Decoder::new(song.config).map_err(|e| ThemeError::Decode(e.to_string()))?;
    let mut pcm = vec![0i16; song.channels * song.superframe_frames];
    let count = song.superframes().min(PROBE_SUPERFRAMES);
    if count == 0 {
        return Err(ThemeError::Format("no audio in the data chunk"));
    }
    for superframe in song.data.chunks_exact(song.superframe_size).take(count) {
        decoder
            .decode_superframe(superframe, &mut pcm)
            .map_err(|e| ThemeError::Decode(e.to_string()))?;
    }
    Ok(())
}

/// Plays a [`ThemeSong`] forever: from its start to the loop's end, then the loop again.
pub struct ThemePlayer {
    song: Arc<ThemeSong>,
    decoder: Option<Decoder>,
    /// One decoded superframe, interleaved in the file's channels.
    pcm: Vec<i16>,
    /// The frame position of `pcm`'s first frame.
    pcm_first: usize,
    /// The next frame to hand out.
    position: usize,
}

impl ThemePlayer {
    pub fn new(song: Arc<ThemeSong>) -> Self {
        let pcm = vec![0i16; song.channels * song.superframe_frames];
        let start = song.start;
        let mut player = Self {
            song,
            decoder: None,
            pcm,
            pcm_first: usize::MAX,
            position: 0,
        };
        player.seek(start);
        player
    }

    pub fn rate(&self) -> u32 {
        self.song.rate
    }

    /// Jumps to `frame`. The decoder carries state from one superframe to the next, so it starts
    /// a superframe early and that one is thrown away. The decoder is kept across jumps: a loop
    /// runs on the audio thread, which should not allocate.
    fn seek(&mut self, frame: usize) {
        let wanted = frame / self.song.superframe_frames;
        if self.decoder.is_none() {
            self.decoder = Decoder::new(self.song.config).ok();
        }
        if wanted > 0 {
            self.decode(wanted - 1);
        }
        self.decode(wanted);
        self.position = frame;
    }

    /// Decodes superframe `index` into `pcm`; a broken or missing one plays as silence.
    fn decode(&mut self, index: usize) {
        let size = self.song.superframe_size;
        let bytes = self.song.data.get(index * size..(index + 1) * size);
        let ok = match (&mut self.decoder, bytes) {
            (Some(decoder), Some(bytes)) => decoder.decode_superframe(bytes, &mut self.pcm).is_ok(),
            _ => false,
        };
        if !ok {
            self.pcm.fill(0);
        }
        self.pcm_first = index * self.song.superframe_frames;
    }

    /// The next frame as left and right, from -1 to 1.
    pub fn next_frame(&mut self) -> (f32, f32) {
        if self.position >= self.song.loop_frames.1 {
            self.seek(self.song.loop_frames.0);
        }
        let frames = self.song.superframe_frames;
        if self.position < self.pcm_first || self.position >= self.pcm_first + frames {
            self.decode(self.position / frames);
        }
        let channels = self.song.channels;
        let at = (self.position - self.pcm_first) * channels;
        let left = self.pcm[at];
        let right = if channels > 1 { self.pcm[at + 1] } else { left };
        self.position += 1;
        (
            f32::from(left) / f32::from(i16::MAX),
            f32::from(right) / f32::from(i16::MAX),
        )
    }
}

/// Songs for the tests of other crates, which cannot build one by hand.
#[doc(hidden)]
pub mod tests_support {
    use super::ThemeSong;

    /// Two seconds of a stream that plays as silence, looping from the start.
    pub fn silent_song() -> ThemeSong {
        ThemeSong {
            rate: 48_000,
            channels: 2,
            config: [0xFE, 0x74, 0x05, 0xF0],
            superframe_size: 192,
            superframe_frames: 1024,
            loop_frames: (0, 96_000),
            start: 0,
            data: vec![0; 94 * 192],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A RIFF WAVE file with an ATRAC9 fmt chunk and the given extra chunks.
    fn riff(config: [u8; 4], channels: u16, chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&EXTENSIBLE.to_le_bytes());
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&48_000u32.to_le_bytes());
        fmt.extend_from_slice(&0u32.to_le_bytes()); // bytes per second
        fmt.extend_from_slice(&192u16.to_le_bytes()); // block align
        fmt.extend_from_slice(&0u16.to_le_bytes()); // bits per sample
        fmt.extend_from_slice(&34u16.to_le_bytes()); // extension size
        fmt.extend_from_slice(&256u16.to_le_bytes()); // samples per block
        fmt.extend_from_slice(&3u32.to_le_bytes()); // channel mask
        fmt.extend_from_slice(&ATRAC9_GUID);
        fmt.extend_from_slice(&2u32.to_le_bytes()); // version
        fmt.extend_from_slice(&config);
        fmt.extend_from_slice(&0u32.to_le_bytes());
        let mut body = b"WAVE".to_vec();
        for (id, chunk) in std::iter::once(&(b"fmt ", fmt)).chain(chunks.iter()) {
            body.extend_from_slice(*id);
            body.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
            body.extend_from_slice(chunk);
            if chunk.len() % 2 == 1 {
                body.push(0);
            }
        }
        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend(body);
        file
    }

    fn words(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    /// The configuration of a real 48 kHz stereo theme song: 192-byte superframes of 1024 frames.
    const CONFIG: [u8; 4] = [0xFE, 0x74, 0x05, 0xF0];

    #[test]
    fn the_chunks_are_found_and_odd_sizes_padded() {
        let file = riff(
            CONFIG,
            2,
            &[
                (b"LIST", vec![1, 2, 3]),
                (b"fact", words(&[96_000, 256, 256])),
                (b"data", vec![0; 384]),
            ],
        );
        let found = chunks(&file).unwrap();
        let (s, e) = found.data.unwrap();
        assert_eq!(e - s, 384);
        let (s, e) = found.fact.unwrap();
        assert_eq!(&file[s..e], &words(&[96_000, 256, 256])[..]);
    }

    #[test]
    fn files_that_are_not_atrac9_are_refused() {
        assert!(matches!(
            from_bytes(b"RIFX0000WAVE".to_vec()),
            Err(ThemeError::Format(_))
        ));
        let mut file = riff(CONFIG, 2, &[(b"data", vec![0; 384])]);
        let guid = file.windows(16).position(|w| w == ATRAC9_GUID).unwrap();
        file[guid] ^= 0xFF;
        assert!(matches!(from_bytes(file), Err(ThemeError::Format(_))));
        let no_data = riff(CONFIG, 2, &[]);
        assert!(matches!(from_bytes(no_data), Err(ThemeError::Format(_))));
        let wrong_channels = riff(CONFIG, 1, &[(b"data", vec![0; 384])]);
        assert!(matches!(
            from_bytes(wrong_channels),
            Err(ThemeError::Format(_))
        ));
    }

    #[test]
    fn truncated_or_noisy_files_never_panic() {
        let noise: Vec<u8> = (0..192 * 12).map(|i| (i * 7 % 251) as u8).collect();
        let file = riff(
            CONFIG,
            2,
            &[(b"fact", words(&[4_000, 256, 256])), (b"data", noise)],
        );
        for cut in 0..file.len() {
            if let Ok(song) = from_bytes(file[..cut].to_vec()) {
                let mut player = ThemePlayer::new(Arc::new(song));
                for _ in 0..5_000 {
                    player.next_frame();
                }
            }
        }
    }

    #[test]
    fn a_loop_under_a_second_gives_way_to_the_whole_song() {
        let song = (256, 100_256);
        assert_eq!(
            choose_loop(Some((1_000, 61_000)), song, 48_000),
            (1_000, 61_000)
        );
        assert_eq!(choose_loop(Some((1_000, 2_000)), song, 48_000), song);
        assert_eq!(choose_loop(Some((5_000, 5_000)), song, 48_000), song);
        assert_eq!(choose_loop(None, song, 48_000), song);
    }

    /// A song built by hand around a known stream, to check positions without real audio.
    fn song(superframes: usize, loop_frames: (usize, usize), start: usize) -> ThemeSong {
        ThemeSong {
            rate: 48_000,
            channels: 2,
            config: CONFIG,
            superframe_size: 192,
            superframe_frames: 1024,
            loop_frames,
            start,
            data: vec![0; superframes * 192],
        }
    }

    #[test]
    fn the_player_starts_after_the_delay_and_wraps_to_the_loop_start() {
        let mut player = ThemePlayer::new(Arc::new(song(4, (1_000, 3_000), 256)));
        assert_eq!(player.position, 256);
        for _ in 256..3_000 {
            player.next_frame();
        }
        assert_eq!(player.position, 3_000);
        player.next_frame();
        assert_eq!(player.position, 1_001);
        assert_eq!(player.pcm_first, 0);
    }

    #[test]
    fn a_loop_past_the_stream_plays_silence_rather_than_panicking() {
        let mut player = ThemePlayer::new(Arc::new(song(2, (0, 5_000), 0)));
        for _ in 0..12_000 {
            let (l, r) = player.next_frame();
            assert!(l.abs() <= 1.0 && r.abs() <= 1.0);
        }
    }

    /// Decodes and plays the theme songs of real dumps without changing them:
    /// `ANYPLAY_THEME_DIR=<folder of games> cargo test theme_songs_of_real -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs game dumps on this machine"]
    fn theme_songs_of_real_dumps_decode() {
        let root = std::env::var("ANYPLAY_THEME_DIR").expect("ANYPLAY_THEME_DIR");
        let mut found = 0;
        for game in fs::read_dir(root).unwrap().flatten() {
            let path = game.path().join("sce_sys/snd0.at9");
            if !path.is_file() {
                continue;
            }
            found += 1;
            let song = Arc::new(load(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())));
            let mut player = ThemePlayer::new(song.clone());
            let seconds = 10;
            let started = std::time::Instant::now();
            let mut peak = 0.0f32;
            for _ in 0..song.rate as usize * seconds {
                let (l, r) = player.next_frame();
                peak = peak.max(l.abs()).max(r.abs());
            }
            println!(
                "{}: {} Hz, loop {:?}, start {}, peak {peak:.2}, {seconds} s decoded in {:?}",
                game.file_name().to_string_lossy(),
                song.rate,
                song.loop_frames,
                song.start,
                started.elapsed()
            );
            assert!(peak > 0.01, "silent");
            // The wrap point decodes too.
            player.seek(song.loop_frames.1 - 10);
            for _ in 0..2_000 {
                player.next_frame();
            }
        }
        assert!(found > 0, "no snd0.at9 found");
    }
}
