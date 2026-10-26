//! Captures: the screenshots and clips the player took, as files in the screenshots folder
//! (docs/DESIGN.md 9.10). This module reads that folder and nothing else: it lists the files,
//! tells which game each belongs to, groups them, and reads a clip's length out of the MP4. It
//! never writes, moves or uploads a capture; deleting one is the caller's explicit act
//! ([`delete`]).
//!
//! Where the files come from, and so what a name tells:
//!
//! * a screenshot is written by the engine as `<TITLEID>-<yyyymmdd>-<hhmmss>[-<n>].png`;
//! * a clip is written by the share worker as `<game name> <yyyy-mm-dd> <hh-mm-ss>[ (<n>)].mp4`
//!   ([`crate::clips::clip_path`]).
//!
//! Any other `.png`, `.jpg`, `.jpeg` or `.mp4` in the folder counts as a capture of no known game.

use std::collections::HashMap;
use std::fs::File;
use std::hash::Hash;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

/// What a capture is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Screenshot,
    Clip,
}

/// Which captures a view lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Screenshots,
    Clips,
}

impl Filter {
    pub const ALL: [Filter; 3] = [Filter::All, Filter::Screenshots, Filter::Clips];

    pub fn allows(self, kind: Kind) -> bool {
        matches!(
            (self, kind),
            (Filter::All, _)
                | (Filter::Screenshots, Kind::Screenshot)
                | (Filter::Clips, Kind::Clip)
        )
    }
}

/// What the file name says about the game.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Owner {
    /// A screenshot: the title ID the engine put first.
    Title(String),
    /// A clip: the game's name as it went into the file name.
    Name(String),
    Unknown,
}

/// One capture file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capture {
    pub path: PathBuf,
    pub kind: Kind,
    pub owner: Owner,
    /// When the file was last written, in seconds since 1970.
    pub modified: i64,
    pub size: u64,
}

impl Capture {
    /// The file's name with its extension.
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// Why a folder gave no list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanError {
    /// There is no such folder (not made yet, renamed or on a drive that is not there).
    Missing,
    /// The folder is there and cannot be read.
    Unreadable,
}

/// Every capture in `folder` (not its subfolders), newest first. Links are not followed.
pub fn scan(folder: &Path) -> Result<Vec<Capture>, ScanError> {
    let entries = std::fs::read_dir(folder).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => ScanError::Missing,
        _ => ScanError::Unreadable,
    })?;
    let mut found: Vec<Capture> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let (kind, owner) = classify(&name)?;
            let meta = entry.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let modified = meta
                .modified()
                .ok()
                .map_or(0, |t| match t.duration_since(UNIX_EPOCH) {
                    Ok(after) => after.as_secs() as i64,
                    Err(before) => -(before.duration().as_secs() as i64),
                });
            Some(Capture {
                path: entry.path(),
                kind,
                owner,
                modified,
                size: meta.len(),
            })
        })
        .collect();
    found.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| b.path.file_name().cmp(&a.path.file_name()))
    });
    Ok(found)
}

/// What a file name is: a capture and whose, or nothing we list.
pub fn classify(file_name: &str) -> Option<(Kind, Owner)> {
    if file_name.starts_with('.') {
        return None;
    }
    let (stem, extension) = file_name.rsplit_once('.')?;
    match extension.to_ascii_lowercase().as_str() {
        "png" | "jpg" | "jpeg" => Some((
            Kind::Screenshot,
            screenshot_title(stem).map_or(Owner::Unknown, Owner::Title),
        )),
        "mp4" => Some((
            Kind::Clip,
            clip_name(stem).map_or(Owner::Unknown, Owner::Name),
        )),
        _ => None,
    }
}

/// `PPSA01234-20261025-180431` or `...-2`: the title ID.
fn screenshot_title(stem: &str) -> Option<String> {
    let bytes = stem.as_bytes();
    if bytes.len() < 25 || !stem.is_char_boundary(9) {
        return None;
    }
    let (id, rest) = stem.split_at(9);
    let id_ok = (id.starts_with("PPSA") || id.starts_with("CUSA"))
        && id[4..].bytes().all(|b| b.is_ascii_digit());
    let rest = rest.strip_prefix('-')?;
    let stamp_ok = rest.len() >= 15
        && rest.is_char_boundary(15)
        && rest.as_bytes()[..8].iter().all(u8::is_ascii_digit)
        && rest.as_bytes()[8] == b'-'
        && rest.as_bytes()[9..15].iter().all(u8::is_ascii_digit);
    if !stamp_ok {
        return None;
    }
    let tail = &rest[15..];
    let tail_ok = tail.is_empty()
        || tail.strip_prefix('-').is_some_and(|n| {
            !n.is_empty() && n.len() <= 4 && n.bytes().all(|b| b.is_ascii_digit())
        });
    (id_ok && stamp_ok && tail_ok).then(|| id.to_owned())
}

/// `<name> 2026-10-25 18-04-31` with an optional ` (2)`: the name.
fn clip_name(stem: &str) -> Option<String> {
    let mut rest = stem;
    if let Some(open) = rest.strip_suffix(')').and_then(|r| r.rfind(" (")) {
        let number = &rest[open + 2..rest.len() - 1];
        if !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) {
            rest = &rest[..open];
        }
    }
    let cut = rest.len().checked_sub(20)?;
    if !rest.is_char_boundary(cut) {
        return None;
    }
    let (name, stamp) = rest.split_at(cut);
    // " yyyy-mm-dd hh-mm-ss"
    let shape = b" 0000-00-00 00-00-00";
    let fits = stamp.len() == shape.len()
        && stamp.bytes().zip(shape).all(|(have, want)| {
            if *want == b'0' {
                have.is_ascii_digit()
            } else {
                have == *want
            }
        });
    let name = name.trim();
    (fits && !name.is_empty()).then(|| name.to_owned())
}

/// The captures that `filter` lets through, grouped by `key` in the order the groups first appear.
/// Given the newest-first list of [`scan`], the groups are ordered by their newest capture and each
/// group's members are newest first. The members are positions in `captures`.
pub fn group<K: Eq + Hash + Clone>(
    captures: &[Capture],
    filter: Filter,
    key: impl Fn(&Capture) -> K,
) -> Vec<(K, Vec<usize>)> {
    let mut order: Vec<(K, Vec<usize>)> = Vec::new();
    let mut at: HashMap<K, usize> = HashMap::new();
    for (i, capture) in captures.iter().enumerate() {
        if !filter.allows(capture.kind) {
            continue;
        }
        let k = key(capture);
        match at.get(&k) {
            Some(&slot) => order[slot].1.push(i),
            None => {
                at.insert(k.clone(), order.len());
                order.push((k, vec![i]));
            }
        }
    }
    order
}

/// Deletes one capture file. It refuses anything that is not a capture the scan would list, so a
/// wrong path can never remove another kind of file.
pub fn delete(path: &Path) -> io::Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if classify(&name).is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a capture file",
        ));
    }
    std::fs::remove_file(path)
}

// --- a clip's length ---------------------------------------------------------------------------

/// How many boxes at one level are read before giving up (a file that is not an MP4 ends soon).
const MAX_BOXES: usize = 256;

/// The length of the MP4 at `path`, from its movie header. `None` for anything that is not a
/// readable MP4 with a length.
pub fn clip_duration(path: &Path) -> Option<Duration> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    mp4_duration(&mut file, len)
}

/// [`clip_duration`] on any reader of `len` bytes.
pub fn mp4_duration<R: Read + Seek>(reader: &mut R, len: u64) -> Option<Duration> {
    let moov = find_box(reader, 0, len, *b"moov")?;
    let mvhd = find_box(reader, moov.0, moov.1, *b"mvhd")?;
    reader.seek(SeekFrom::Start(mvhd.0)).ok()?;
    let mut head = [0u8; 1];
    reader.read_exact(&mut head).ok()?;
    let (timescale, duration) = if head[0] == 1 {
        let mut b = [0u8; 3 + 8 + 8 + 4 + 8];
        reader.read_exact(&mut b).ok()?;
        (
            u32::from_be_bytes(b[19..23].try_into().ok()?),
            u64::from_be_bytes(b[23..31].try_into().ok()?),
        )
    } else {
        let mut b = [0u8; 3 + 4 + 4 + 4 + 4];
        reader.read_exact(&mut b).ok()?;
        (
            u32::from_be_bytes(b[11..15].try_into().ok()?),
            u64::from(u32::from_be_bytes(b[15..19].try_into().ok()?)),
        )
    };
    if timescale == 0 || duration == 0 {
        return None;
    }
    Some(Duration::from_secs_f64(
        duration as f64 / f64::from(timescale),
    ))
}

/// The payload `(start, end)` of the first box called `kind` between `from` and `to`.
fn find_box<R: Read + Seek>(
    reader: &mut R,
    from: u64,
    to: u64,
    kind: [u8; 4],
) -> Option<(u64, u64)> {
    let mut at = from;
    for _ in 0..MAX_BOXES {
        if at + 8 > to {
            return None;
        }
        reader.seek(SeekFrom::Start(at)).ok()?;
        let mut head = [0u8; 8];
        reader.read_exact(&mut head).ok()?;
        let size32 = u32::from_be_bytes(head[..4].try_into().ok()?);
        let (header, size) = match size32 {
            0 => (8, to - at),
            1 => {
                let mut big = [0u8; 8];
                reader.read_exact(&mut big).ok()?;
                (16, u64::from_be_bytes(big))
            }
            n => (8, u64::from(n)),
        };
        if size < header || at.checked_add(size)? > to {
            return None;
        }
        if head[4..8] == kind {
            return Some((at + header, at + size));
        }
        at += size;
    }
    None
}

// --- a clip's picture --------------------------------------------------------------------------

/// A picture in RGBA, 8 bits a channel.
#[derive(Clone, Debug)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The picture the system's shell shows for a clip (its Explorer thumbnail), at most `side` pixels
/// on the longer side. Windows only; elsewhere, and whenever the shell has none, `None`: the
/// caller shows a plain clip tile. Blocks, so call it off the interface thread.
pub fn clip_thumbnail(path: &Path, side: u32) -> Option<Picture> {
    #[cfg(windows)]
    {
        windows_thumbnail(path, side)
    }
    #[cfg(not(windows))]
    {
        let _ = (path, side);
        None
    }
}

#[cfg(windows)]
fn windows_thumbnail(path: &Path, side: u32) -> Option<Picture> {
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
        DeleteObject, GetDIBits, GetObjectW, HGDIOBJ,
    };
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_THUMBNAILONLY,
    };
    use windows::core::HSTRING;

    // SAFETY: plain calls into the shell and GDI. Every handle made here is released before the
    // function returns, the buffer is as long as the bitmap header says, and COM is set up for
    // this thread (a thread that has it already answers with a harmless "changed mode" code).
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(
            &HSTRING::from(path.to_string_lossy().replace('/', "\\")),
            None,
        )
        .inspect_err(|e| tracing::debug!("captures: no shell item: {e}"))
        .ok()?;
        let side = side.clamp(32, 1024) as i32;
        let bitmap = factory
            .GetImage(SIZE { cx: side, cy: side }, SIIGBF_THUMBNAILONLY)
            .inspect_err(|e| tracing::debug!("captures: no shell picture: {e}"))
            .ok()?;
        let object = HGDIOBJ(bitmap.0);
        let mut info = BITMAP::default();
        let described = GetObjectW(
            object,
            std::mem::size_of::<BITMAP>() as i32,
            Some(std::ptr::from_mut(&mut info).cast()),
        );
        let (width, height) = (info.bmWidth, info.bmHeight);
        let mut picture = None;
        if described > 0 && width > 0 && height > 0 && width <= 4096 && height <= 4096 {
            let dc = CreateCompatibleDC(None);
            let mut header = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    // Negative: top-down rows.
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bgra = vec![0u8; width as usize * height as usize * 4];
            let rows = GetDIBits(
                dc,
                bitmap,
                0,
                height as u32,
                Some(bgra.as_mut_ptr().cast()),
                &mut header,
                DIB_RGB_COLORS,
            );
            let _ = DeleteDC(dc);
            if rows > 0 {
                // A video frame is opaque; a shell that leaves the alpha at zero would show it clear.
                for px in bgra.as_chunks_mut::<4>().0 {
                    px.swap(0, 2);
                    px[3] = 255;
                }
                picture = Some(Picture {
                    width: width as u32,
                    height: height as u32,
                    rgba: bgra,
                });
            }
        }
        let _ = DeleteObject(object);
        picture
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn engine_screenshot_names_give_their_title() {
        for name in [
            "PPSA01234-20261025-180431.png",
            "CUSA00001-20261025-180431-2.png",
            "PPSA01234-20261025-180431.JPG",
        ] {
            let (kind, owner) = classify(name).unwrap();
            assert_eq!(kind, Kind::Screenshot, "{name}");
            assert!(
                matches!(owner, Owner::Title(ref id) if id.len() == 9),
                "{name}"
            );
        }
        assert_eq!(
            classify("PPSA01234-20261025-180431.png").unwrap().1,
            Owner::Title("PPSA01234".into())
        );
    }

    #[test]
    fn other_pictures_belong_to_no_game() {
        for name in [
            "holiday.png",
            "PPSA01234.png",
            "PPSA01234-2026.png",
            "ppsa01234-20261025-180431.png",
            "PPSA01234-20261025-180431-x.png",
        ] {
            assert_eq!(classify(name).unwrap().1, Owner::Unknown, "{name}");
        }
    }

    #[test]
    fn clip_names_give_the_game_name() {
        assert_eq!(
            classify("Hollow Fields 2026-10-25 18-04-31.mp4").unwrap(),
            (Kind::Clip, Owner::Name("Hollow Fields".into()))
        );
        assert_eq!(
            classify("Hollow Fields 2026-10-25 18-04-31 (2).mp4")
                .unwrap()
                .1,
            Owner::Name("Hollow Fields".into())
        );
        assert_eq!(
            classify("Game 2026-10-25 18-04-31.MP4").unwrap().0,
            Kind::Clip
        );
        assert_eq!(classify("holiday.mp4").unwrap().1, Owner::Unknown);
        assert_eq!(
            classify("2026-10-25 18-04-31.mp4").unwrap().1,
            Owner::Unknown
        );
    }

    #[test]
    fn everything_else_is_not_a_capture() {
        for name in [
            "notes.txt",
            "clip.mp4.part",
            ".hidden.png",
            "folder",
            "x.mkv",
            "",
        ] {
            assert!(classify(name).is_none(), "{name}");
        }
        // Multi-byte names never panic.
        assert!(classify("日本語のゲーム 2026-10-25 18-04-31.mp4").is_some());
        assert!(classify("ééééééééééééééééééééééé.png").is_some());
    }

    #[test]
    fn a_scan_lists_newest_first_and_skips_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, age: u64| {
            let path = dir.path().join(name);
            std::fs::write(&path, b"x").unwrap();
            let when = std::time::SystemTime::now() - Duration::from_secs(age);
            File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(when)
                .unwrap();
        };
        write("PPSA01234-20261025-180431.png", 300);
        write("Game 2026-10-25 18-04-31.mp4", 100);
        write("notes.txt", 10);
        std::fs::create_dir(dir.path().join("inner.png")).unwrap();
        let found = scan(dir.path()).unwrap();
        let names: Vec<_> = found.iter().map(Capture::file_name).collect();
        assert_eq!(
            names,
            [
                "Game 2026-10-25 18-04-31.mp4",
                "PPSA01234-20261025-180431.png"
            ]
        );
        assert_eq!(found[0].size, 1);
    }

    #[test]
    fn a_missing_folder_is_told_apart() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(scan(&dir.path().join("gone")), Err(ScanError::Missing));
        let file = dir.path().join("file");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(
            scan(&file),
            Err(ScanError::Missing),
            "a file is not a folder"
        );
    }

    fn capture(name: &str, modified: i64) -> Capture {
        let (kind, owner) = classify(name).unwrap();
        Capture {
            path: PathBuf::from(name),
            kind,
            owner,
            modified,
            size: 1,
        }
    }

    #[test]
    fn groups_keep_the_newest_first_and_the_filter_applies() {
        let all = [
            capture("Alpha 2026-10-25 18-04-31.mp4", 50),
            capture("PPSA00002-20261025-180431.png", 40),
            capture("PPSA00001-20261025-180431.png", 30),
            capture("PPSA00002-20261024-180431.png", 20),
            capture("Alpha 2026-10-24 18-04-31.mp4", 10),
        ];
        let key = |c: &Capture| match &c.owner {
            Owner::Name(n) if n == "Alpha" => "PPSA00001".to_owned(),
            Owner::Title(t) => t.clone(),
            _ => String::new(),
        };
        let groups = group(&all, Filter::All, key);
        assert_eq!(
            groups,
            vec![
                ("PPSA00001".to_owned(), vec![0, 2, 4]),
                ("PPSA00002".to_owned(), vec![1, 3]),
            ]
        );
        let shots = group(&all, Filter::Screenshots, key);
        assert_eq!(shots[0], ("PPSA00002".to_owned(), vec![1, 3]));
        assert_eq!(shots[1], ("PPSA00001".to_owned(), vec![2]));
        assert_eq!(group(&all, Filter::Clips, key).len(), 1);
    }

    #[test]
    fn delete_only_removes_captures() {
        let dir = tempfile::tempdir().unwrap();
        let shot = dir.path().join("PPSA01234-20261025-180431.png");
        let other = dir.path().join("keep.txt");
        std::fs::write(&shot, b"x").unwrap();
        std::fs::write(&other, b"x").unwrap();
        assert!(delete(&other).is_err());
        assert!(other.exists());
        delete(&shot).unwrap();
        assert!(!shot.exists());
    }

    fn boxed(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out
    }

    fn mvhd_v0(timescale: u32, duration: u32) -> Vec<u8> {
        let mut p = vec![0u8; 4 + 4 + 4];
        p.extend_from_slice(&timescale.to_be_bytes());
        p.extend_from_slice(&duration.to_be_bytes());
        p.extend_from_slice(&[0; 80]);
        boxed(b"mvhd", &p)
    }

    #[test]
    fn the_length_comes_from_the_movie_header() {
        let mut file = boxed(b"ftyp", b"isom0000");
        file.extend(boxed(b"free", &[0; 100]));
        file.extend(boxed(b"moov", &mvhd_v0(1000, 30_500)));
        file.extend(boxed(b"mdat", &[0; 500]));
        let len = file.len() as u64;
        assert_eq!(
            mp4_duration(&mut Cursor::new(file), len),
            Some(Duration::from_millis(30_500))
        );
    }

    #[test]
    fn a_version_one_header_and_a_moov_after_the_data_work() {
        let mut p = vec![1u8, 0, 0, 0];
        p.extend_from_slice(&[0; 16]);
        p.extend_from_slice(&600u32.to_be_bytes());
        p.extend_from_slice(&(600u64 * 90).to_be_bytes());
        p.extend_from_slice(&[0; 80]);
        let mut file = boxed(b"ftyp", b"isom0000");
        file.extend(boxed(b"mdat", &[0; 64]));
        file.extend(boxed(b"moov", &boxed(b"mvhd", &p)));
        let len = file.len() as u64;
        assert_eq!(
            mp4_duration(&mut Cursor::new(file), len),
            Some(Duration::from_secs(90))
        );
    }

    #[test]
    fn broken_files_have_no_length_and_do_not_hang() {
        for bytes in [
            Vec::new(),
            vec![0u8; 7],
            b"not an mp4 at all, just some text that goes on".to_vec(),
            // A box that claims more than the file holds.
            [&1_000_000u32.to_be_bytes()[..], b"moov"].concat(),
            // A box of size 4 (smaller than its header).
            [&4u32.to_be_bytes()[..], b"moov"].concat(),
            // Empty boxes forever.
            [&8u32.to_be_bytes()[..], b"free"].concat().repeat(1000),
        ] {
            let len = bytes.len() as u64;
            assert_eq!(mp4_duration(&mut Cursor::new(bytes), len), None);
        }
        let mut zero = boxed(b"moov", &mvhd_v0(1000, 0));
        let len = zero.len() as u64;
        assert_eq!(
            mp4_duration(&mut Cursor::new(std::mem::take(&mut zero)), len),
            None
        );
    }
}

#[cfg(test)]
mod real_file_tests {
    use super::*;

    /// With `ANYPLAY_TEST_CLIP=<an .mp4>` and `ANYPLAY_TEST_SECONDS=<its length>`: the movie header
    /// of a real encoder's file, and on Windows the shell's picture of it.
    #[test]
    #[ignore = "needs a real mp4 (ANYPLAY_TEST_CLIP)"]
    fn a_real_clip_has_a_length_and_on_windows_a_picture() {
        let path = PathBuf::from(std::env::var("ANYPLAY_TEST_CLIP").expect("ANYPLAY_TEST_CLIP"));
        let want: f64 = std::env::var("ANYPLAY_TEST_SECONDS")
            .expect("ANYPLAY_TEST_SECONDS")
            .parse()
            .unwrap();
        let length = clip_duration(&path).expect("a length");
        assert!((length.as_secs_f64() - want).abs() < 0.2, "{length:?}");
        let started = std::time::Instant::now();
        let picture = clip_thumbnail(&path, 512);
        eprintln!(
            "thumbnail in {:?}: {:?}",
            started.elapsed(),
            picture.as_ref().map(|p| (p.width, p.height))
        );
        if cfg!(windows) {
            let picture = picture.expect("the shell has a picture of an H.264 file");
            assert!(
                picture.width.max(picture.height) <= 512,
                "{}x{}",
                picture.width,
                picture.height
            );
            assert_eq!(
                picture.rgba.len(),
                (picture.width * picture.height * 4) as usize
            );
            let lit = picture
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[0] > 40 || p[1] > 40 || p[2] > 40)
                .count();
            assert!(lit > picture.rgba.len() / 16, "the picture is not black");
            // Keep it for a look.
            if let Ok(out) = std::env::var("ANYPLAY_TEST_THUMB_OUT") {
                let mut ppm = format!(
                    "P6
{} {}
255
",
                    picture.width, picture.height
                )
                .into_bytes();
                for px in picture.rgba.as_chunks::<4>().0 {
                    ppm.extend_from_slice(&px[..3]);
                }
                std::fs::write(out, ppm).unwrap();
            }
        }
    }
}
