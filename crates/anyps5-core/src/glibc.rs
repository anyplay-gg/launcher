//! The glibc version the engine's libraries ask for, against the one this system has (Linux).
//!
//! Every ELF that links against glibc lists the symbol versions it needs in its `.gnu.version_r`
//! section (`GLIBC_2.38`, ...). The newest of them over the relinker and the `.prx` libraries is the
//! oldest glibc the toolkit runs on. [`needed_in`] reads those sections without loading the files
//! (section headers only, every offset and length checked); [`system`] asks the running glibc.
//!
//! A shortfall is a notice, never a block: unlocking and starting still work, and the launcher says
//! plainly what is missing (docs/DESIGN.md 5.20).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::toolkit::Toolkit;

/// A glibc version, `major.minor` (a patch level never changes what a symbol needs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }

    /// `2.38`, `2.2.5` or `2.35-0ubuntu3`: the leading `major.minor`.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor_text = parts.next()?;
        let digits = minor_text
            .find(|c: char| !c.is_ascii_digit())
            .map_or(minor_text, |end| &minor_text[..end]);
        Some(Self::new(major, digits.parse().ok()?))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// The libraries need a newer glibc than the system has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortfall {
    pub needed: Version,
    pub system: Version,
}

/// `Some` when `system` is older than `needed`. Unknown on either side is no shortfall.
pub fn compare(needed: Option<Version>, system: Option<Version>) -> Option<Shortfall> {
    match (needed, system) {
        (Some(needed), Some(system)) if system < needed => Some(Shortfall { needed, system }),
        _ => None,
    }
}

/// The glibc this process runs on. `None` anywhere but Linux with glibc.
pub fn system() -> Option<Version> {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        // SAFETY: returns a pointer to a static NUL-terminated string owned by libc.
        let text = unsafe { std::ffi::CStr::from_ptr(libc::gnu_get_libc_version()) };
        Version::parse(text.to_str().ok()?)
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    None
}

/// Whether the toolkit's libraries need a newer glibc than this system has. Reads the toolkit's
/// files, so call it when the toolkit is located, not every frame. Always `None` off Linux.
pub fn check(toolkit: &Toolkit) -> Option<Shortfall> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    compare(
        needed_in_toolkit(toolkit.relinker(), toolkit.libs()),
        system(),
    )
}

/// The newest glibc version any of the relinker and the files of `libs` asks for.
pub fn needed_in_toolkit(relinker: &Path, libs: &Path) -> Option<Version> {
    // A toolkit holds a few dozen libraries; the cap only guards a folder picked by mistake.
    const MAX_FILES: usize = 4096;
    let mut newest = needed_in(relinker);
    let Ok(entries) = std::fs::read_dir(libs) else {
        return newest;
    };
    for entry in entries.flatten().take(MAX_FILES) {
        if entry.file_type().is_ok_and(|t| t.is_file()) {
            newest = newest.max(needed_in(&entry.path()));
        }
    }
    newest
}

/// The newest `GLIBC_x.y` one file asks for; `None` when it is no ELF, has no version needs, or
/// cannot be read.
pub fn needed_in(path: &Path) -> Option<Version> {
    needed_in_elf(&mut File::open(path).ok()?)
}

/// The same over any seekable source.
pub fn needed_in_elf<R: Read + Seek>(source: &mut R) -> Option<Version> {
    let elf = Elf::open(source)?;
    let mut newest = None;
    for index in 0..elf.section_count {
        let section = elf.section(source, index)?;
        if section.kind == SHT_GNU_VERNEED {
            newest = newest.max(elf.glibc_in_verneed(source, &section));
        }
    }
    newest
}

const SHT_GNU_VERNEED: u32 = 0x6fff_fffe;
/// Section headers are walked one by one; 65535 headers of 64 bytes is the most a file can have.
const MAX_SECTIONS: u32 = 65_535;
/// A `.gnu.version_r` of a real library is a few hundred bytes.
const MAX_VERNEED: u64 = 1 << 20;
const MAX_NAME: u64 = 64;

struct Section {
    kind: u32,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
}

struct Elf {
    wide: bool,
    table: u64,
    entry_size: u64,
    section_count: u32,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        b.get(at..at.checked_add(8)?)?.try_into().ok()?,
    ))
}

/// `len` bytes at `offset`, or `None` when the source is shorter.
fn read_at<R: Read + Seek>(source: &mut R, offset: u64, len: u64) -> Option<Vec<u8>> {
    let mut buf = vec![0; usize::try_from(len).ok()?];
    source.seek(SeekFrom::Start(offset)).ok()?;
    source.read_exact(&mut buf).ok()?;
    Some(buf)
}

impl Elf {
    /// Little-endian 32- and 64-bit ELF with a section table; anything else is `None`.
    fn open<R: Read + Seek>(source: &mut R) -> Option<Self> {
        let head = read_at(source, 0, 0x40).or_else(|| read_at(source, 0, 0x34))?;
        if head.get(..4)? != b"\x7fELF" || *head.get(5)? != 1 {
            return None;
        }
        let wide = match head.get(4)? {
            1 => false,
            2 => true,
            _ => return None,
        };
        let (table, entry_size, count) = if wide {
            (
                u64_at(&head, 0x28)?,
                u64::from(u16_at(&head, 0x3a)?),
                u32::from(u16_at(&head, 0x3c)?),
            )
        } else {
            (
                u64::from(u32_at(&head, 0x20)?),
                u64::from(u16_at(&head, 0x2e)?),
                u32::from(u16_at(&head, 0x30)?),
            )
        };
        if table == 0 || entry_size < if wide { 0x30 } else { 0x28 } {
            return None;
        }
        let mut elf = Self {
            wide,
            table,
            entry_size,
            section_count: count.max(1),
        };
        if count == 0 {
            // More sections than a 16-bit count holds: the size of the first header says how many.
            elf.section_count = u32::try_from(elf.section(source, 0)?.size).ok()?;
        }
        (elf.section_count <= MAX_SECTIONS).then_some(elf)
    }

    fn section<R: Read + Seek>(&self, source: &mut R, index: u32) -> Option<Section> {
        if index >= self.section_count {
            return None;
        }
        let at = self
            .table
            .checked_add(u64::from(index).checked_mul(self.entry_size)?)?;
        let b = read_at(source, at, self.entry_size.min(0x40))?;
        Some(if self.wide {
            Section {
                kind: u32_at(&b, 4)?,
                offset: u64_at(&b, 0x18)?,
                size: u64_at(&b, 0x20)?,
                link: u32_at(&b, 0x28)?,
                info: u32_at(&b, 0x2c)?,
            }
        } else {
            Section {
                kind: u32_at(&b, 4)?,
                offset: u64::from(u32_at(&b, 0x10)?),
                size: u64::from(u32_at(&b, 0x14)?),
                link: u32_at(&b, 0x18)?,
                info: u32_at(&b, 0x1c)?,
            }
        })
    }

    /// Walks the `Verneed` records: each names a library and lists the versions needed from it.
    fn glibc_in_verneed<R: Read + Seek>(
        &self,
        source: &mut R,
        verneed: &Section,
    ) -> Option<Version> {
        if verneed.size == 0 || verneed.size > MAX_VERNEED {
            return None;
        }
        let data = read_at(source, verneed.offset, verneed.size)?;
        let strings = self.section(source, verneed.link)?;
        let mut newest = None;
        let mut need = 0usize;
        for _ in 0..verneed.info.min(4096) {
            let count = u16_at(&data, need.checked_add(2)?)?;
            let aux_offset = u32_at(&data, need.checked_add(8)?)?;
            let next = u32_at(&data, need.checked_add(12)?)?;
            let mut aux = need.checked_add(aux_offset as usize)?;
            for _ in 0..count {
                let name = u32_at(&data, aux.checked_add(8)?)?;
                let next_aux = u32_at(&data, aux.checked_add(12)?)?;
                newest = newest.max(self.version_name(source, &strings, name));
                if next_aux == 0 {
                    break;
                }
                aux = aux.checked_add(next_aux as usize)?;
            }
            if next == 0 {
                break;
            }
            need = need.checked_add(next as usize)?;
        }
        newest
    }

    /// `GLIBC_2.38` at `name` in the string table; other names (and `GLIBC_PRIVATE`) are `None`.
    fn version_name<R: Read + Seek>(
        &self,
        source: &mut R,
        strings: &Section,
        name: u32,
    ) -> Option<Version> {
        let name = u64::from(name);
        if name >= strings.size {
            return None;
        }
        let len = (strings.size - name).min(MAX_NAME);
        let bytes = read_at(source, strings.offset.checked_add(name)?, len)?;
        let end = bytes.iter().position(|b| *b == 0)?;
        let text = std::str::from_utf8(&bytes[..end]).ok()?;
        Version::parse(text.strip_prefix("GLIBC_")?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A little-endian ELF with a string table and one `.gnu.version_r`, built by hand.
    /// `needs`: (library, versions).
    fn elf(wide: bool, needs: &[(&str, &[&str])]) -> Vec<u8> {
        fn add(s: &str, strtab: &mut Vec<u8>) -> u32 {
            let at = strtab.len() as u32;
            strtab.extend_from_slice(s.as_bytes());
            strtab.push(0);
            at
        }
        let mut strtab = vec![0u8];
        let mut verneed = Vec::new();
        for (i, (lib, versions)) in needs.iter().enumerate() {
            let file = add(lib, &mut strtab);
            let next = if i + 1 == needs.len() {
                0
            } else {
                16 + 16 * versions.len() as u32
            };
            verneed.extend_from_slice(&1u16.to_le_bytes());
            verneed.extend_from_slice(&(versions.len() as u16).to_le_bytes());
            verneed.extend_from_slice(&file.to_le_bytes());
            verneed.extend_from_slice(&16u32.to_le_bytes());
            verneed.extend_from_slice(&next.to_le_bytes());
            for (j, v) in versions.iter().enumerate() {
                let name = add(v, &mut strtab);
                let next_aux = if j + 1 == versions.len() { 0 } else { 16u32 };
                verneed.extend_from_slice(&0u32.to_le_bytes());
                verneed.extend_from_slice(&0u16.to_le_bytes());
                verneed.extend_from_slice(&0u16.to_le_bytes());
                verneed.extend_from_slice(&name.to_le_bytes());
                verneed.extend_from_slice(&next_aux.to_le_bytes());
            }
        }
        let header = if wide { 0x40 } else { 0x34 };
        let entry = if wide { 0x40usize } else { 0x28 };
        let strtab_at = header;
        let verneed_at = strtab_at + strtab.len();
        let table_at = (verneed_at + verneed.len()).next_multiple_of(8);
        let mut out = vec![0u8; header];
        out[..4].copy_from_slice(b"\x7fELF");
        out[4] = if wide { 2 } else { 1 };
        out[5] = 1;
        out[6] = 1;
        let (shoff, shentsize, shnum) = if wide {
            (0x28, 0x3a, 0x3c)
        } else {
            (0x20, 0x2e, 0x30)
        };
        if wide {
            out[shoff..shoff + 8].copy_from_slice(&(table_at as u64).to_le_bytes());
        } else {
            out[shoff..shoff + 4].copy_from_slice(&(table_at as u32).to_le_bytes());
        }
        out[shentsize..shentsize + 2].copy_from_slice(&(entry as u16).to_le_bytes());
        out[shnum..shnum + 2].copy_from_slice(&3u16.to_le_bytes());
        out.extend_from_slice(&strtab);
        out.extend_from_slice(&verneed);
        out.resize(table_at, 0);
        let mut section = |kind: u32, offset: usize, size: usize, link: u32, info: u32| {
            let mut s = vec![0u8; entry];
            s[4..8].copy_from_slice(&kind.to_le_bytes());
            if wide {
                s[0x18..0x20].copy_from_slice(&(offset as u64).to_le_bytes());
                s[0x20..0x28].copy_from_slice(&(size as u64).to_le_bytes());
                s[0x28..0x2c].copy_from_slice(&link.to_le_bytes());
                s[0x2c..0x30].copy_from_slice(&info.to_le_bytes());
            } else {
                s[0x10..0x14].copy_from_slice(&(offset as u32).to_le_bytes());
                s[0x14..0x18].copy_from_slice(&(size as u32).to_le_bytes());
                s[0x18..0x1c].copy_from_slice(&link.to_le_bytes());
                s[0x1c..0x20].copy_from_slice(&info.to_le_bytes());
            }
            out.extend_from_slice(&s);
        };
        section(0, 0, 0, 0, 0);
        section(3, strtab_at, strtab.len(), 0, 0);
        section(
            SHT_GNU_VERNEED,
            verneed_at,
            verneed.len(),
            1,
            needs.len() as u32,
        );
        out
    }

    fn needed(bytes: Vec<u8>) -> Option<Version> {
        needed_in_elf(&mut Cursor::new(bytes))
    }

    const SAMPLE: &[(&str, &[&str])] = &[
        ("libc.so.6", &["GLIBC_2.2.5", "GLIBC_2.38", "GLIBC_2.17"]),
        ("libm.so.6", &["GLIBC_2.29", "GLIBC_PRIVATE"]),
    ];

    #[test]
    fn the_newest_version_over_all_needs_wins_in_64_bit() {
        assert_eq!(needed(elf(true, SAMPLE)), Some(Version::new(2, 38)));
    }

    #[test]
    fn the_same_in_32_bit() {
        assert_eq!(needed(elf(false, SAMPLE)), Some(Version::new(2, 38)));
    }

    #[test]
    fn other_libraries_versions_and_private_names_do_not_count() {
        let other: &[(&str, &[&str])] = &[
            ("libstdc++.so.6", &["GLIBCXX_3.4.30", "CXXABI_1.3.13"]),
            ("libc.so.6", &["GLIBC_PRIVATE", "GLIBC_2.14"]),
        ];
        assert_eq!(needed(elf(true, other)), Some(Version::new(2, 14)));
        let none: &[(&str, &[&str])] = &[("libz.so.1", &["ZLIB_1.2.2"])];
        assert_eq!(needed(elf(true, none)), None);
    }

    #[test]
    fn not_an_elf_or_a_big_endian_one_is_nothing() {
        assert_eq!(needed(b"MZ\x90\0 a windows file".to_vec()), None);
        assert_eq!(needed(Vec::new()), None);
        let mut big = elf(true, SAMPLE);
        big[5] = 2;
        assert_eq!(needed(big), None);
    }

    #[test]
    fn a_truncated_file_is_nothing_and_does_not_panic() {
        let good = elf(true, SAMPLE);
        for cut in 0..good.len() {
            let _ = needed(good[..cut].to_vec());
        }
        assert_eq!(needed(good[..good.len() - 1].to_vec()), None);
    }

    #[test]
    fn lying_offsets_and_counts_are_nothing_and_do_not_panic() {
        let good = elf(true, SAMPLE);
        // Every 4-byte field in turn set to a huge value: never a panic.
        let mut bad = good.clone();
        for at in 0..good.len() - 4 {
            let mut fuzzed = good.clone();
            fuzzed[at..at + 4].copy_from_slice(&0xffff_fff0u32.to_le_bytes());
            let _ = needed(fuzzed);
        }
        bad[0x3c] = 0xff;
        bad[0x3d] = 0xff;
        assert_eq!(needed(bad), None);
    }

    #[test]
    fn versions_parse_and_order() {
        assert_eq!(Version::parse("2.38"), Some(Version::new(2, 38)));
        assert_eq!(Version::parse("2.2.5"), Some(Version::new(2, 2)));
        assert_eq!(Version::parse("2.35-0ubuntu3"), Some(Version::new(2, 35)));
        assert_eq!(Version::parse("PRIVATE"), None);
        assert!(Version::new(2, 9) < Version::new(2, 17));
        assert_eq!(Version::new(2, 38).to_string(), "2.38");
    }

    #[test]
    fn a_shortfall_needs_both_sides() {
        let (a, b) = (Version::new(2, 38), Version::new(2, 35));
        assert_eq!(
            compare(Some(a), Some(b)),
            Some(Shortfall {
                needed: a,
                system: b
            })
        );
        assert_eq!(compare(Some(b), Some(a)), None);
        assert_eq!(compare(Some(a), Some(a)), None);
        assert_eq!(compare(None, Some(b)), None);
        assert_eq!(compare(Some(a), None), None);
    }

    #[test]
    fn a_folder_is_judged_by_its_newest_file() {
        let dir = std::env::temp_dir().join(format!("anyps5-glibc-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("libs")).unwrap();
        let old: &[(&str, &[&str])] = &[("libc.so.6", &["GLIBC_2.17"])];
        std::fs::write(dir.join("relinker"), elf(true, old)).unwrap();
        std::fs::write(dir.join("libs/a.prx"), elf(true, SAMPLE)).unwrap();
        std::fs::write(dir.join("libs/readme.txt"), "text").unwrap();
        assert_eq!(
            needed_in_toolkit(&dir.join("relinker"), &dir.join("libs")),
            Some(Version::new(2, 38))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
