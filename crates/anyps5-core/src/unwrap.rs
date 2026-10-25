//! Ported from utkuhalis/AnyPS5-ARM (SelfUnwrap.swift, GPL-2.0-only) and changed since; see README "Credits".
//! Turns a decrypted SELF or fSELF container into the plain ELF the relinker reads.
//!
//! Byte-exact with `SelfUnwrap.swift` of the macOS launcher (ARM `cb20fd8`), which in turn follows
//! `tools/unwrap_self.py`. The 64-bit little-endian check of the script is included. Where the
//! Swift code would trap on malformed input, this module returns [`Refused`]; it never panics.
//!
//! Interface: [`is_container`], [`unwrap_bytes`], [`unwrap_file`]. Everything else is private.

use std::fs;
use std::io::{self, Read};
use std::path::Path;

/// Why a container was not unwrapped. `Display` is the user-facing reason.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{reason}")]
pub struct Refused {
    pub reason: String,
}

impl Refused {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl From<io::Error> for Refused {
    fn from(error: io::Error) -> Self {
        Self::new(error.to_string())
    }
}

const SELF_MAGIC: u32 = 0x1D3D_154F;
const SELF_MAGIC_KERNEL: u32 = 0xEEF5_1454;
const RECORD_OFFSET: usize = 0x40;
const RECORD_SIZE: usize = 0x20;
const RECORD_HAS_SEGMENT: u64 = 0x800;
const RECORD_SEGMENT_SHIFT: u32 = 20;
const FIRST_REGION: usize = 0x400;
const MAX_EMPTY_LOAD_PADDING: u64 = 64 * 1024 * 1024;
/// Not in the original: a guard so that hostile headers cannot ask for an absurd allocation.
const MAX_OUTPUT: u64 = 1 << 32;
const PT_LOAD: u32 = 1;
const PT_NOTE: u32 = 4;
const ENTROPY_LIMIT: f64 = 7.5;
const ENTROPY_SAMPLE: u64 = 0x10000;

/// Whether the file starts with the magic of a SELF container. Unreadable or short files: false.
pub fn is_container(path: &Path) -> bool {
    let mut head = [0u8; 4];
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    if file.read_exact(&mut head).is_err() {
        return false;
    }
    let magic = u32::from_le_bytes(head);
    magic == SELF_MAGIC || magic == SELF_MAGIC_KERNEL
}

/// Unwraps `input` into `output` (parent folders are created) and returns the ELF's size.
pub fn unwrap_file(input: &Path, output: &Path) -> Result<usize, Refused> {
    if aliases(input, output) {
        return Err(Refused::new("output aliases the input container"));
    }
    let data = fs::read(input)?;
    let result = unwrap_bytes(&data)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, &result)?;
    Ok(result.len())
}

/// Same file after resolving links (an output that does not exist yet cannot alias anything).
fn aliases(input: &Path, output: &Path) -> bool {
    match (fs::canonicalize(input), fs::canonicalize(output)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn u16_at(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().expect("4 bytes"))
}

fn u64_at(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(data[at..at + 8].try_into().expect("8 bytes"))
}

#[derive(Clone)]
struct Header {
    index: usize,
    kind: u32,
    flags: u32,
    offset: u64,
    vaddr: u64,
    size: u64,
    memory: u64,
    align: u64,
    source: u64,
    placed: u64,
}

struct Record {
    flags: u64,
    segment: u64,
    size: u64,
}

struct Region {
    old: u64,
    size: u64,
    vaddr: u64,
    align: u64,
    new: u64,
}

fn entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut counts = [0usize; 256];
    for byte in data {
        counts[*byte as usize] += 1;
    }
    let length = data.len() as f64;
    let mut total = 0.0;
    for count in counts.into_iter().filter(|c| *c != 0) {
        let share = count as f64 / length;
        total -= share * share.log2();
    }
    total
}

fn read_records(data: &[u8]) -> Vec<Record> {
    let mut records = Vec::new();
    let mut offset = RECORD_OFFSET;
    let length = data.len() as u64;
    while offset + RECORD_SIZE <= FIRST_REGION {
        let flags = u64_at(data, offset);
        let segment = u64_at(data, offset + 8);
        let size = u64_at(data, offset + 16);
        let size_again = u64_at(data, offset + 24);
        if size == 0 || size != size_again {
            break;
        }
        if segment >= length || segment < 0x350 || !segment.is_multiple_of(0x10) {
            break;
        }
        if size > length {
            break;
        }
        records.push(Record {
            flags,
            segment,
            size,
        });
        offset += RECORD_SIZE;
    }
    records
}

fn align_up(value: u64, alignment: u64) -> u64 {
    if alignment <= 1 {
        return value;
    }
    (value.wrapping_add(alignment - 1)) / alignment * alignment
}

fn find_elf(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|w| w == [0x7F, b'E', b'L', b'F'])
}

struct Elf {
    header: Vec<u8>,
    headers: Vec<Header>,
}

fn parse_elf(data: &[u8]) -> Result<Elf, Refused> {
    let inner = find_elf(data).ok_or_else(|| Refused::new("no inner ELF header"))?;
    if inner + 0x40 > data.len() {
        return Err(Refused::new("inner ELF header is truncated"));
    }
    let header = data[inner..inner + 0x40].to_vec();
    if header[4] != 2 || header[5] != 1 {
        return Err(Refused::new("inner ELF is not 64-bit little-endian"));
    }
    let program_offset = u64_at(&header, 0x20);
    let entry_size = u16_at(&header, 0x36);
    let count = u16_at(&header, 0x38) as usize;
    if entry_size != 56 || count == 0 {
        return Err(Refused::new(format!(
            "program header table is {count} entries of {entry_size} bytes"
        )));
    }
    let table_fits = (inner as u64)
        .checked_add(program_offset)
        .and_then(|end| end.checked_add(count as u64 * 56))
        .is_some_and(|end| end <= data.len() as u64);
    if !table_fits {
        return Err(Refused::new(
            "ELF program header table does not fit the file",
        ));
    }
    let mut headers = Vec::with_capacity(count);
    for index in 0..count {
        let at = inner + program_offset as usize + index * 56;
        let kind = u32_at(data, at);
        let flags = u32_at(data, at + 4);
        let offset = u64_at(data, at + 8);
        let vaddr = u64_at(data, at + 16);
        let size = u64_at(data, at + 32);
        let memory = u64_at(data, at + 40);
        let align = u64_at(data, at + 48);
        if kind == PT_LOAD {
            if size > memory {
                return Err(Refused::new(format!(
                    "segment {index} file size exceeds memory size"
                )));
            }
            if align != 0 && align & (align - 1) != 0 {
                return Err(Refused::new(format!(
                    "segment {index} alignment is not a power of two"
                )));
            }
            let (sum, wrapped) = vaddr.overflowing_add(memory);
            if wrapped && sum != 0 {
                return Err(Refused::new(format!(
                    "segment {index} memory range exceeds ELF64"
                )));
            }
        }
        headers.push(Header {
            index,
            kind,
            flags,
            offset,
            vaddr,
            size,
            memory,
            align,
            source: offset,
            placed: 0,
        });
    }
    Ok(Elf { header, headers })
}

fn place_segments(data: &[u8], headers: &mut [Header], records: &[Record]) -> Result<(), Refused> {
    let length = data.len() as u64;
    let mut used = vec![false; records.len()];
    let mut placed = vec![false; headers.len()];

    for (index, record) in records.iter().enumerate() {
        let target = (record.flags >> RECORD_SEGMENT_SHIFT) as i64;
        if record.flags & RECORD_HAS_SEGMENT != 0
            && target >= 0
            && (target as usize) < headers.len()
            && !placed[target as usize]
            && headers[target as usize].size == record.size
        {
            headers[target as usize].source = record.segment;
            placed[target as usize] = true;
            used[index] = true;
        }
    }

    // First maximum, as Swift's max(by:) and Python's max() keep it.
    let mut last: Option<usize> = None;
    for header in headers.iter() {
        if placed[header.index] && header.vaddr == 0 && header.kind != PT_LOAD {
            match last {
                Some(l) if header.offset <= headers[l].offset => {}
                _ => last = Some(header.index),
            }
        }
    }
    if let Some(last) = last {
        let last_offset = headers[last].offset;
        let mut cursor = headers[last].source.saturating_add(headers[last].size);
        let mut trailing: Vec<usize> = headers
            .iter()
            .filter(|h| {
                !placed[h.index]
                    && h.vaddr == 0
                    && h.kind != PT_LOAD
                    && h.size != 0
                    && h.offset > last_offset
            })
            .map(|h| h.index)
            .collect();
        trailing.sort_by_key(|i| headers[*i].offset);
        for index in trailing {
            placed[index] = true;
            let header = &mut headers[index];
            if header.kind == PT_NOTE {
                header.source = 0;
                header.size = 0;
                continue;
            }
            header.source = cursor;
            cursor = cursor.saturating_add(header.size);
            if cursor > length {
                header.source = 0;
                header.size = 0;
            }
        }
    }

    for header in headers.iter_mut() {
        if header.kind != PT_LOAD || header.size == 0 || placed[header.index] {
            continue;
        }
        for (index, record) in records.iter().enumerate() {
            if !used[index] && record.size == header.size {
                used[index] = true;
                header.source = record.segment;
                break;
            }
        }
    }

    let loads: Vec<usize> = headers
        .iter()
        .filter(|h| h.kind == PT_LOAD)
        .map(|h| h.index)
        .collect();
    // The last PT_LOAD that holds the address; returns its (index, vaddr, size, source).
    let load_for = |headers: &[Header], vaddr: u64, size: u64| -> Option<(usize, u64, u64, u64)> {
        let mut found = None;
        for &i in &loads {
            let h = &headers[i];
            let end = h.vaddr.wrapping_add(h.memory);
            if h.memory != 0 && h.vaddr <= vaddr && (vaddr < end || (size == 0 && vaddr == end)) {
                found = Some((h.index, h.vaddr, h.size, h.source));
            }
        }
        found
    };

    // (offset in the ELF, shift to the container offset) of the last placed segment.
    let mut tail: Option<(u64, u64)> = None;
    for i in 0..headers.len() {
        if headers[i].kind == PT_LOAD {
            continue;
        }
        if placed[i] {
            let h = &headers[i];
            if h.size != 0 && tail.is_none_or(|t| h.offset > t.0) {
                tail = Some((h.offset, h.source.wrapping_sub(h.offset)));
            }
            continue;
        }
        if headers[i].vaddr != 0 {
            let (vaddr, size) = (headers[i].vaddr, headers[i].size);
            let Some((host_index, host_vaddr, host_size, host_source)) =
                load_for(headers, vaddr, size)
            else {
                if size == 0 {
                    headers[i].source = 0;
                    continue;
                }
                return Err(Refused::new(format!(
                    "segment {i} vaddr 0x{vaddr:x} is in no PT_LOAD"
                )));
            };
            let offset = vaddr - host_vaddr;
            if size != 0 && (offset > host_size || size > host_size - offset) {
                return Err(Refused::new(format!(
                    "segment {i} does not fit the file-backed range of PT_LOAD {host_index}"
                )));
            }
            let h = &mut headers[i];
            h.source = host_source.wrapping_add(offset);
            if tail.is_none_or(|t| h.offset > t.0) {
                tail = Some((h.offset, h.source.wrapping_sub(h.offset)));
            }
        } else if headers[i].size != 0 {
            let h = &mut headers[i];
            h.source = h.offset.wrapping_add(tail.map_or(0, |t| t.1));
            if h.source > length {
                h.source = length;
            }
            let keep = length - h.source;
            h.size = h.size.min(keep);
            h.memory = h.memory.min(keep);
        }
    }

    for h in headers.iter() {
        if h.kind == PT_LOAD && h.size == 0 {
            continue;
        }
        if h.source > length || h.size > length - h.source {
            return Err(Refused::new(format!(
                "segment {} does not fit the file",
                h.index
            )));
        }
    }
    Ok(())
}

fn check_decrypted(data: &[u8], headers: &[Header]) -> Result<(), Refused> {
    let loaded: Vec<&Header> = headers
        .iter()
        .filter(|h| h.kind == PT_LOAD && h.size != 0)
        .collect();
    let mut encrypted = Vec::new();
    for h in &loaded {
        let start = h.source as usize;
        let end = start + h.size.min(ENTROPY_SAMPLE) as usize;
        let value = entropy(&data[start..end]);
        if value >= ENTROPY_LIMIT {
            encrypted.push((h.index, h.source, value));
        }
    }
    if !encrypted.is_empty() && encrypted.len() == loaded.len() {
        let listed: Vec<String> = encrypted
            .iter()
            .map(|(i, s, v)| format!("segment {i} at 0x{s:x} ({v:.2} bits per byte)"))
            .collect();
        return Err(Refused::new(format!(
            "every load segment looks encrypted: {}",
            listed.join(", ")
        )));
    }
    Ok(())
}

fn region_for(regions: &[Region], offset: u64, size: u64) -> Option<usize> {
    regions.iter().position(|r| {
        r.old <= offset
            && offset
                .checked_add(size)
                .zip(r.old.checked_add(r.size))
                .is_some_and(|(end, region_end)| end <= region_end)
    })
}

/// Unwraps a decrypted SELF container held in memory into the bytes of a plain ELF.
pub fn unwrap_bytes(data: &[u8]) -> Result<Vec<u8>, Refused> {
    if data.len() < FIRST_REGION {
        return Err(Refused::new("file is smaller than the SELF header area"));
    }
    let magic = u32_at(data, 0);
    if magic != SELF_MAGIC && magic != SELF_MAGIC_KERNEL {
        return Err(Refused::new(format!(
            "not a SELF container (magic 0x{magic:x})"
        )));
    }

    let records = read_records(data);
    let Elf {
        header: elf_header,
        mut headers,
    } = parse_elf(data)?;
    place_segments(data, &mut headers, &records)?;
    check_decrypted(data, &headers)?;

    let mut regions: Vec<Region> = headers
        .iter()
        .filter(|h| h.kind == PT_LOAD && h.size != 0)
        .map(|h| Region {
            old: h.source,
            size: h.size,
            vaddr: h.vaddr,
            align: h.align,
            new: 0,
        })
        .collect();
    for h in headers.iter().filter(|h| h.kind != PT_LOAD && h.size != 0) {
        if region_for(&regions, h.source, h.size).is_none() {
            regions.push(Region {
                old: h.source,
                size: h.size,
                vaddr: h.vaddr,
                align: h.align,
                new: 0,
            });
        }
    }
    // Stable, as Python's sort is.
    regions.sort_by_key(|r| r.old);

    let program_offset = u64_at(&elf_header, 0x20);
    let mut cursor = (FIRST_REGION as u64).max(program_offset + headers.len() as u64 * 56);
    for region in regions.iter_mut() {
        let alignment = if region.align != 0 && region.align & (region.align - 1) == 0 {
            region.align
        } else {
            1
        };
        let mut placed = align_up(cursor, alignment);
        if region.vaddr % alignment != placed % alignment {
            placed = placed.wrapping_add(
                (region.vaddr % alignment)
                    .wrapping_add(alignment)
                    .wrapping_sub(placed % alignment)
                    % alignment,
            );
        }
        region.new = placed;
        cursor = placed.wrapping_add(region.size);
        if cursor > MAX_OUTPUT || placed > MAX_OUTPUT {
            return Err(Refused::new("output would be larger than 4 GiB"));
        }
    }

    let mut empty_end = cursor;
    for h in headers.iter().filter(|h| h.kind == PT_LOAD && h.size == 0) {
        empty_end = empty_end.max(h.vaddr % h.align.max(1));
    }
    if empty_end - cursor > MAX_EMPTY_LOAD_PADDING {
        return Err(Refused::new(format!(
            "empty load alignment requires more than {MAX_EMPTY_LOAD_PADDING} bytes of output padding"
        )));
    }
    cursor = empty_end;

    let mut result = vec![0u8; cursor as usize];
    for region in &regions {
        let from = region.old as usize;
        let to = region.new as usize;
        let size = region.size as usize;
        result[to..to + size].copy_from_slice(&data[from..from + size]);
    }

    for h in headers.iter_mut() {
        if h.size == 0 && (h.kind == PT_LOAD || h.source == 0) {
            h.placed = if h.kind == PT_LOAD {
                h.vaddr % h.align.max(1)
            } else {
                0
            };
            continue;
        }
        let placed = match region_for(&regions, h.source, h.size) {
            Some(r) => regions[r].new + (h.source - regions[r].old),
            None => h.source,
        };
        if placed
            .checked_add(h.size)
            .is_none_or(|end| end > result.len() as u64)
        {
            return Err(Refused::new(format!(
                "segment {} escaped the output",
                h.index
            )));
        }
        h.placed = placed;
    }

    result[0..0x40].copy_from_slice(&elf_header);
    result[0x28..0x30].copy_from_slice(&0u64.to_le_bytes());
    result[0x3C..0x3E].copy_from_slice(&0u16.to_le_bytes());
    result[0x3E..0x40].copy_from_slice(&0u16.to_le_bytes());
    for h in &headers {
        let at = program_offset as usize + h.index * 56;
        result[at..at + 4].copy_from_slice(&h.kind.to_le_bytes());
        result[at + 4..at + 8].copy_from_slice(&h.flags.to_le_bytes());
        result[at + 8..at + 16].copy_from_slice(&h.placed.to_le_bytes());
        result[at + 16..at + 24].copy_from_slice(&h.vaddr.to_le_bytes());
        result[at + 24..at + 32].copy_from_slice(&h.vaddr.to_le_bytes());
        result[at + 32..at + 40].copy_from_slice(&h.size.to_le_bytes());
        result[at + 40..at + 48].copy_from_slice(&h.memory.to_le_bytes());
        result[at + 48..at + 56].copy_from_slice(&h.align.to_le_bytes());
    }
    Ok(result)
}

/// Builders for synthetic containers, shared by the tests of this crate (appendix A of PORT.md).
#[cfg(test)]
pub(crate) mod testing {
    pub fn phdr(t: u32, fl: u32, off: u64, va: u64, fs: u64, ms: u64, al: u64) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend(t.to_le_bytes());
        v.extend(fl.to_le_bytes());
        for x in [off, va, va, fs, ms, al] {
            v.extend(x.to_le_bytes());
        }
        v
    }

    pub fn elf_header(phoff: u64, phnum: u16) -> Vec<u8> {
        let mut h = vec![0u8; 64];
        h[0..8].copy_from_slice(b"\x7fELF\x02\x01\x01\x00");
        h[16..18].copy_from_slice(&2u16.to_le_bytes());
        h[18..20].copy_from_slice(&0x3Eu16.to_le_bytes());
        h[20..24].copy_from_slice(&1u32.to_le_bytes());
        h[24..32].copy_from_slice(&0x401000u64.to_le_bytes());
        h[32..40].copy_from_slice(&phoff.to_le_bytes());
        h[40..48].copy_from_slice(&0x1234u64.to_le_bytes());
        h[48..52].copy_from_slice(&0u32.to_le_bytes());
        h[52..54].copy_from_slice(&64u16.to_le_bytes());
        h[54..56].copy_from_slice(&56u16.to_le_bytes());
        h[56..58].copy_from_slice(&phnum.to_le_bytes());
        h[58..60].copy_from_slice(&64u16.to_le_bytes());
        h[60..62].copy_from_slice(&5u16.to_le_bytes());
        h[62..64].copy_from_slice(&4u16.to_le_bytes());
        h
    }

    pub fn record(flags: u64, seg: u64, size: u64) -> Vec<u8> {
        let mut v = Vec::new();
        for x in [flags, seg, size, size] {
            v.extend(x.to_le_bytes());
        }
        v
    }

    pub fn build(
        inner_at: usize,
        phoff: u64,
        phs: &[Vec<u8>],
        records: &[Vec<u8>],
        blobs: &[(usize, Vec<u8>)],
        total: usize,
    ) -> Vec<u8> {
        let mut d = vec![0u8; total];
        d[0..4].copy_from_slice(&0x1D3D154Fu32.to_le_bytes());
        for (i, r) in records.iter().enumerate() {
            d[0x40 + 0x20 * i..0x40 + 0x20 * (i + 1)].copy_from_slice(r);
        }
        d[inner_at..inner_at + 64].copy_from_slice(&elf_header(phoff, phs.len() as u16));
        for (i, p) in phs.iter().enumerate() {
            let a = inner_at + phoff as usize + 56 * i;
            d[a..a + 56].copy_from_slice(p);
        }
        for (off, b) in blobs {
            d[*off..*off + b.len()].copy_from_slice(b);
        }
        d
    }

    pub fn pattern(n: usize, seed: usize) -> Vec<u8> {
        (0..n)
            .map(|i| ((seed * 7 + i * 13 + (i >> 3)) % 61) as u8)
            .collect()
    }

    /// V1 of appendix A: one load segment.
    pub fn v1() -> Vec<u8> {
        build(
            0x100,
            0x40,
            &[phdr(1, 5, 0x400, 0x400000, 0x100, 0x100, 0x1000)],
            &[record(0x800, 0x1000, 0x100)],
            &[(0x1000, pattern(0x100, 1))],
            0x1100,
        )
    }

    /// V2: two loads, a PT_NOTE that is cut, a GNU_STACK header.
    pub fn v2() -> Vec<u8> {
        build(
            0x100,
            0x40,
            &[
                phdr(1, 5, 0x400, 0x400000, 0x200, 0x200, 0x4000),
                phdr(1, 6, 0x4400, 0x404000, 0x80, 0x300, 0x4000),
                phdr(4, 4, 0x4500, 0, 0x10, 0x10, 8),
                phdr(0x6474e551, 6, 0, 0, 0, 0, 0x10),
            ],
            &[record(0x800, 0x1000, 0x200), record(0, 0x1400, 0x80)],
            &[(0x1000, pattern(0x200, 2)), (0x1400, pattern(0x80, 3))],
            0x1500,
        )
    }

    /// V3: V1 with a segment of 256 distinct bytes, which reads as encrypted.
    pub fn v3() -> Vec<u8> {
        build(
            0x100,
            0x40,
            &[phdr(1, 5, 0x400, 0x400000, 0x100, 0x100, 0x1000)],
            &[record(0x800, 0x1000, 0x100)],
            &[(0x1000, (0..=255u8).collect())],
            0x1100,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use sha2::{Digest, Sha256};

    fn sha(data: &[u8]) -> String {
        Sha256::digest(data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn header_at(out: &[u8], i: usize) -> [u64; 8] {
        let at = 0x40 + 56 * i;
        [
            u32_at(out, at) as u64,
            u32_at(out, at + 4) as u64,
            u64_at(out, at + 8),
            u64_at(out, at + 16),
            u64_at(out, at + 24),
            u64_at(out, at + 32),
            u64_at(out, at + 40),
            u64_at(out, at + 48),
        ]
    }

    fn refused(data: &[u8]) -> String {
        unwrap_bytes(data).unwrap_err().reason
    }

    #[test]
    fn vector_v1() {
        let input = v1();
        assert_eq!(input.len(), 4352);
        assert_eq!(
            sha(&input),
            "d53440efc25dc2df09a7e2549506e3c9cb50b354b607cf0dbbb31c8c70a0c9d0"
        );
        let out = unwrap_bytes(&input).unwrap();
        assert_eq!(out.len(), 4352);
        assert_eq!(
            sha(&out),
            "1384a1dae8b587352a342472c4b37f1cac70d76e7454b61414cb526ba94bdffd"
        );
        assert_eq!(u64_at(&out, 0x28), 0);
        assert_eq!(u16_at(&out, 0x3C), 0);
        assert_eq!(u16_at(&out, 0x3E), 0);
        assert_eq!(
            header_at(&out, 0),
            [1, 5, 0x1000, 0x400000, 0x400000, 0x100, 0x100, 0x1000]
        );
    }

    #[test]
    fn vector_v2() {
        let input = v2();
        assert_eq!(input.len(), 5376);
        assert_eq!(
            sha(&input),
            "ae7efa53d323d80176a0597aa1779fbaf0be1efb11317085ba241950cdaa6c62"
        );
        let out = unwrap_bytes(&input).unwrap();
        assert_eq!(out.len(), 32896);
        assert_eq!(
            sha(&out),
            "29ee0d7e0b97848a55b038e8a4fd5b410403ea4dcd3a30d5ff8dc92d19a1e170"
        );
        assert_eq!(
            header_at(&out, 0),
            [1, 5, 0x4000, 0x400000, 0x400000, 0x200, 0x200, 0x4000]
        );
        assert_eq!(
            header_at(&out, 1),
            [1, 6, 0x8000, 0x404000, 0x404000, 0x80, 0x300, 0x4000]
        );
        assert_eq!(header_at(&out, 2), [4, 4, 0x1500, 0, 0, 0, 0, 8]);
        assert_eq!(header_at(&out, 3), [0x6474e551, 6, 0, 0, 0, 0, 0, 0x10]);
    }

    #[test]
    fn vector_v3_is_refused_as_encrypted() {
        let input = v3();
        assert_eq!(
            sha(&input),
            "0ec58b3db5cf2bd054be594f824036fdd82bd33ff75592b1dad58d42b09f58ad"
        );
        assert_eq!(
            refused(&input),
            "every load segment looks encrypted: segment 0 at 0x1000 (8.00 bits per byte)"
        );
    }

    #[test]
    fn small_files_and_wrong_magic() {
        assert_eq!(
            refused(&[0u8; 0x3FF]),
            "file is smaller than the SELF header area"
        );
        let mut d = v1();
        d[0..4].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
        assert_eq!(refused(&d), "not a SELF container (magic 0xdeadbeef)");
        let mut kernel = v1();
        kernel[0..4].copy_from_slice(&SELF_MAGIC_KERNEL.to_le_bytes());
        assert!(unwrap_bytes(&kernel).is_ok());
    }

    #[test]
    fn inner_elf_errors() {
        let mut d = v1();
        d[0x100..0x104].copy_from_slice(b"XXXX");
        assert_eq!(refused(&d), "no inner ELF header");

        // The ELF magic in the last 40 bytes: header truncated.
        let mut d = vec![0u8; 0x400];
        d[0..4].copy_from_slice(&SELF_MAGIC.to_le_bytes());
        d[0x3E0..0x3E4].copy_from_slice(b"\x7fELF");
        assert_eq!(refused(&d), "inner ELF header is truncated");

        let mut d = v1();
        d[0x100 + 4] = 1;
        assert_eq!(refused(&d), "inner ELF is not 64-bit little-endian");

        let mut d = v1();
        d[0x100 + 0x36..0x100 + 0x38].copy_from_slice(&32u16.to_le_bytes());
        assert_eq!(refused(&d), "program header table is 1 entries of 32 bytes");

        let mut d = v1();
        d[0x100 + 0x20..0x100 + 0x28].copy_from_slice(&0x10000u64.to_le_bytes());
        assert_eq!(
            refused(&d),
            "ELF program header table does not fit the file"
        );
        d[0x100 + 0x20..0x100 + 0x28].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(
            refused(&d),
            "ELF program header table does not fit the file"
        );
    }

    #[test]
    fn segment_errors() {
        let one = |ph: Vec<u8>, recs: &[Vec<u8>], blobs: &[(usize, Vec<u8>)]| {
            build(0x100, 0x40, &[ph], recs, blobs, 0x1100)
        };
        let blob = vec![(0x1000usize, pattern(0x100, 1))];
        let rec = vec![record(0x800, 0x1000, 0x100)];

        let d = one(
            phdr(1, 5, 0x400, 0x400000, 0x200, 0x100, 0x1000),
            &rec,
            &blob,
        );
        assert_eq!(refused(&d), "segment 0 file size exceeds memory size");
        let d = one(phdr(1, 5, 0x400, 0x400000, 0x100, 0x100, 3), &rec, &blob);
        assert_eq!(refused(&d), "segment 0 alignment is not a power of two");
        let d = one(
            phdr(1, 5, 0x400, u64::MAX - 5, 0x100, 0x100, 0x1000),
            &rec,
            &blob,
        );
        assert_eq!(refused(&d), "segment 0 memory range exceeds ELF64");

        // A non-load header with a vaddr outside every load.
        let d = build(
            0x100,
            0x40,
            &[
                phdr(1, 5, 0x400, 0x400000, 0x100, 0x100, 0x1000),
                phdr(2, 4, 0x500, 0x900000, 0x10, 0x10, 8),
            ],
            &rec,
            &blob,
            0x1100,
        );
        assert_eq!(refused(&d), "segment 1 vaddr 0x900000 is in no PT_LOAD");

        // A non-load header that starts in a load but reaches beyond its file range.
        let d = build(
            0x100,
            0x40,
            &[
                phdr(1, 5, 0x400, 0x400000, 0x100, 0x200, 0x1000),
                phdr(2, 4, 0x500, 0x4000f0, 0x20, 0x20, 8),
            ],
            &rec,
            &blob,
            0x1100,
        );
        assert_eq!(
            refused(&d),
            "segment 1 does not fit the file-backed range of PT_LOAD 0"
        );

        // A load that no record covers points into the void.
        let d = one(phdr(1, 5, 0x9000, 0x400000, 0x100, 0x100, 0x1000), &[], &[]);
        assert_eq!(refused(&d), "segment 0 does not fit the file");
    }

    #[test]
    fn empty_load_padding_is_capped() {
        let d = build(
            0x100,
            0x40,
            &[
                phdr(1, 5, 0x400, 0x400000, 0x100, 0x100, 0x1000),
                phdr(1, 6, 0, 0x7fff_ffff, 0, 0x10, 1 << 31),
            ],
            &[record(0x800, 0x1000, 0x100)],
            &[(0x1000, pattern(0x100, 1))],
            0x1100,
        );
        assert_eq!(
            refused(&d),
            "empty load alignment requires more than 67108864 bytes of output padding"
        );
    }

    #[test]
    fn note_without_bytes_and_stable_first_maximum() {
        // Two recorded headers without vaddr and the same offset: the first one is "last".
        // The PT_NOTE behind them is emitted without file bytes.
        let d = build(
            0x100,
            0x40,
            &[
                phdr(1, 5, 0x400, 0x400000, 0x100, 0x100, 0x1000),
                phdr(0x61000000, 4, 0x2000, 0, 0x20, 0x20, 8),
                phdr(0x61000001, 4, 0x2000, 0, 0x20, 0x20, 8),
                phdr(4, 4, 0x2100, 0, 0x10, 0x10, 8),
            ],
            &[
                record(0x800, 0x1000, 0x100),
                record(0x800 | (1 << 20), 0x1200, 0x20),
                record(0x800 | (2 << 20), 0x1300, 0x20),
            ],
            &[
                (0x1000, pattern(0x100, 1)),
                (0x1200, pattern(0x20, 4)),
                (0x1300, pattern(0x20, 5)),
            ],
            0x1400,
        );
        let out = unwrap_bytes(&d).unwrap();
        let note = header_at(&out, 3);
        assert_eq!(
            note[5], 0,
            "a trailing PT_NOTE is emitted without file bytes"
        );
    }

    #[test]
    fn never_panics_on_mutated_input() {
        // A deterministic xorshift stands in for a property test.
        let mut state = 0x2545F4914F6CDD1Du64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for base in [v1(), v2(), v3()] {
            for _ in 0..3000 {
                let mut d = base.clone();
                for _ in 0..(next() % 6 + 1) {
                    let at = (next() % d.len() as u64) as usize;
                    d[at] = next() as u8;
                }
                // Aim some mutations at the headers.
                let at = 0x100 + (next() % 0x180) as usize;
                if at < d.len() {
                    d[at] = next() as u8;
                }
                let _ = unwrap_bytes(&d);
            }
        }
    }

    #[test]
    fn is_container_checks_both_magics() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, bytes: &[u8]| {
            let p = dir.path().join(name);
            fs::write(&p, bytes).unwrap();
            p
        };
        assert!(is_container(&write("a", &SELF_MAGIC.to_le_bytes())));
        assert!(is_container(&write("b", &SELF_MAGIC_KERNEL.to_le_bytes())));
        assert!(!is_container(&write("c", b"\x7fELF")));
        assert!(!is_container(&write("d", &[0x4F, 0x15, 0x3D])));
        assert!(!is_container(&dir.path().join("missing")));
        assert!(!is_container(dir.path()));
    }

    #[test]
    fn unwrap_file_writes_and_refuses_aliases() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.bin");
        fs::write(&input, v1()).unwrap();
        let output = dir.path().join("deep/er/out.elf");
        assert_eq!(unwrap_file(&input, &output).unwrap(), 4352);
        assert_eq!(fs::read(&output).unwrap().len(), 4352);

        let err = unwrap_file(&input, &input).unwrap_err();
        assert_eq!(err.reason, "output aliases the input container");
        // The same file through a different spelling of the path.
        let spelled = dir.path().join("deep/../in.bin");
        let err = unwrap_file(&input, &spelled).unwrap_err();
        assert_eq!(err.reason, "output aliases the input container");
        assert_eq!(fs::read(&input).unwrap(), v1());
    }
}
