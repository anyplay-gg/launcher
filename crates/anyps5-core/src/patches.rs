//! Per-title patch catalogue: the data model, its loader, and the planning that turns a patch set
//! into the relinker's patch file (PORT.md 6.9; GamePatches.swift for the semantics).
//!
//! Patches are byte edits in the ELF's virtual address space, checked against the staged
//! `eboot.bin` before the relinker is asked to apply them. Version 1 of the launcher ships an empty
//! catalogue: [`PatchSet::load`] then finds nothing and the interface shows no resolution choice.

use std::{
    fmt::{self, Write as _},
    fs,
    path::Path,
};

use serde::Deserialize;

/// One byte edit at an address.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Edit {
    pub address: String,
    pub original: String,
    pub replacement: String,
}

/// A named group of edits that applies or is skipped as a whole.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Patch {
    pub name: String,
    #[serde(default)]
    pub edits: Vec<Edit>,
}

/// One place in the code that holds the output width or height as an immediate.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Field {
    /// `"width"` or `"height"`.
    pub value: String,
    pub address: String,
    /// The instruction's bytes (hex) with the original immediate.
    pub instruction: String,
    /// Where the 4-byte immediate starts inside the instruction.
    pub offset: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Resolution {
    /// `WIDTHxHEIGHT` of the unpatched title.
    pub default: String,
    pub choices: Vec<String>,
    #[serde(default)]
    pub fields: Vec<Field>,
}

/// The patches of one title.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct PatchSet {
    /// Must be `"elf"`: addresses are ELF virtual addresses. Files for other address spaces (the
    /// macOS launcher's Mach-O files) are ignored.
    #[serde(default)]
    pub space: String,
    #[serde(default)]
    pub patches: Vec<Patch>,
    #[serde(default)]
    pub resolution: Option<Resolution>,
}

/// The planner's result: the relinker entries and the state of each patch by name.
pub type Plan = (Vec<PatchEntry>, Vec<(String, PatchState)>);

/// What the planner decided for one patch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchState {
    Applied,
    AlreadyApplied,
    /// The bytes at `address` are not the expected ones.
    Skipped {
        address: String,
        holds: String,
        expected: String,
    },
}

impl fmt::Display for PatchState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatchState::Applied => f.write_str("applied"),
            PatchState::AlreadyApplied => f.write_str("already applied"),
            PatchState::Skipped {
                address,
                holds,
                expected,
            } => {
                write!(f, "skipped: {address} holds {holds}, expected {expected}")
            }
        }
    }
}

/// One `[[patch]]` of the relinker's patch file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchEntry {
    pub name: String,
    pub va: u64,
    pub expected: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PatchError {
    #[error("not a 64-bit little-endian ELF executable")]
    NotElf,
    #[error("not a valid 64-bit little-endian Mach-O executable")]
    NotMacho,
    #[error("0x{0:x} is not in a file-backed segment")]
    NotInSegment(u64),
    #[error("{name}: original and replacement at {address} differ in length")]
    LengthMismatch { name: String, address: String },
    #[error("{0:?} is not an address")]
    BadAddress(String),
    #[error("{0:?} is not a hexadecimal byte string")]
    BadHex(String),
    #[error("not a WIDTHxHEIGHT resolution: {0}")]
    BadResolution(String),
}

impl PatchSet {
    /// The ARM toolkit's catalogue uses Mach-O addresses; legacy files omit `space`.
    pub fn load_macho(title_id: &str, dir: &Path) -> Option<PatchSet> {
        let bytes = fs::read(dir.join(format!("{title_id}.json"))).ok()?;
        let set: PatchSet = serde_json::from_slice(&bytes).ok()?;
        (set.space.is_empty() || set.space == "macho").then_some(set)
    }

    /// Apply checked edits only to the newly copied executable, never to the original dump.
    pub fn apply_macho(
        &self,
        path: &Path,
        resolution: Option<&str>,
    ) -> Result<Vec<(String, PatchState)>, Box<dyn std::error::Error + Send + Sync>> {
        let mut data = fs::read(path)?;
        let segments = macho_segments(&data)?;
        let (entries, states) = self.plan_segments(&data, resolution, &segments)?;
        for entry in entries {
            let offset = file_offset(&segments, entry.va, entry.value.len())?;
            data[offset..offset + entry.value.len()].copy_from_slice(&entry.value);
        }
        fs::write(path, data)?;
        Ok(states)
    }
    /// The title's patch file `<dir>\<title id>.json`. `None` when there is none, when it does not
    /// parse, or when it is not an `"elf"`-space file.
    pub fn load(title_id: &str, dir: &Path) -> Option<PatchSet> {
        let bytes = fs::read(dir.join(format!("{title_id}.json"))).ok()?;
        let set: PatchSet = serde_json::from_slice(&bytes).ok()?;
        (set.space == "elf").then_some(set)
    }

    /// The resolutions this title offers; empty when it has no resolution patch.
    pub fn resolution_choices(&self) -> &[String] {
        self.resolution
            .as_ref()
            .map_or(&[], |r| r.choices.as_slice())
    }

    /// The resolution that applies for the player's choice: the choice when it is offered, else
    /// the title's default.
    pub fn effective_resolution(&self, choice: Option<&str>) -> Option<String> {
        let resolution = self.resolution.as_ref()?;
        Some(
            choice
                .filter(|c| resolution.choices.iter().any(|x| x == c))
                .unwrap_or(&resolution.default)
                .to_string(),
        )
    }

    /// Checks every patch against the ELF image and returns the entries for the relinker plus the
    /// state of each patch (resolution fields count as the patch `resolution`).
    ///
    /// A patch whose bytes already equal the replacement is "already applied", one whose bytes
    /// are neither is skipped with the bytes found; broken data (length mismatch, address outside
    /// the file) is an error.
    pub fn plan(&self, elf: &[u8], resolution: Option<&str>) -> Result<Plan, PatchError> {
        let segments = elf_segments(elf)?;
        self.plan_segments(elf, resolution, &segments)
    }

    fn plan_segments(
        &self,
        elf: &[u8],
        resolution: Option<&str>,
        segments: &[Segment],
    ) -> Result<Plan, PatchError> {
        let mut entries = Vec::new();
        let mut states = Vec::new();
        for patch in &self.patches {
            let mut planned = Vec::new();
            let mut state = None;
            for (index, edit) in patch.edits.iter().enumerate() {
                let original = hex_bytes(&edit.original)?;
                let replacement = hex_bytes(&edit.replacement)?;
                if original.len() != replacement.len() {
                    return Err(PatchError::LengthMismatch {
                        name: patch.name.clone(),
                        address: edit.address.clone(),
                    });
                }
                let va = parse_address(&edit.address)?;
                let offset = file_offset(segments, va, original.len())?;
                let current = &elf[offset..offset + original.len()];
                if current == original {
                    planned.push(PatchEntry {
                        name: format!("{}_{index}", slug(&patch.name)),
                        va,
                        expected: original,
                        value: replacement,
                    });
                } else if current != replacement {
                    state = Some(PatchState::Skipped {
                        address: edit.address.clone(),
                        holds: to_hex(current),
                        expected: edit.original.clone(),
                    });
                    break;
                }
            }
            let state = state.unwrap_or(if planned.is_empty() {
                PatchState::AlreadyApplied
            } else {
                PatchState::Applied
            });
            if state == PatchState::Applied {
                entries.extend(planned);
            }
            states.push((patch.name.clone(), state));
        }
        if let Some(wanted) = self.effective_resolution(resolution)
            && let Some(set) = &self.resolution
        {
            let (width, height) =
                parse_resolution(&wanted).ok_or(PatchError::BadResolution(wanted.clone()))?;
            let mut planned = Vec::new();
            let mut state = PatchState::Applied;
            for (index, field) in set.fields.iter().enumerate() {
                let instruction = hex_bytes(&field.instruction)?;
                let va = parse_address(&field.address)?;
                let offset = file_offset(segments, va, instruction.len())?;
                let current = &elf[offset..offset + instruction.len()];
                if current != instruction {
                    state = PatchState::Skipped {
                        address: field.address.clone(),
                        holds: to_hex(current),
                        expected: field.instruction.clone(),
                    };
                    break;
                }
                if field
                    .offset
                    .checked_add(4)
                    .is_none_or(|end| end > instruction.len())
                {
                    return Err(PatchError::LengthMismatch {
                        name: "resolution".into(),
                        address: field.address.clone(),
                    });
                }
                let number = if field.value == "height" {
                    height
                } else {
                    width
                };
                let mut value = instruction.clone();
                value[field.offset..field.offset + 4].copy_from_slice(&number.to_le_bytes());
                planned.push(PatchEntry {
                    name: format!("resolution_{}_{index}", slug(&field.value)),
                    va,
                    expected: instruction,
                    value,
                });
            }
            if state == PatchState::Applied {
                entries.extend(planned);
            }
            states.push(("resolution".to_string(), state));
        }
        Ok((entries, states))
    }
}

/// The text of the relinker's patch file for the planned entries.
pub fn relinker_toml(entries: &[PatchEntry]) -> String {
    let mut out = String::from("format = 1\n");
    for entry in entries {
        let _ = write!(
            out,
            "\n[[patch]]\nname = \"{}\"\ntarget = \"eboot.bin\"\nva = 0x{:X}\nexpected = \"{}\"\nvalue = \"{}\"\n",
            entry.name,
            entry.va,
            to_hex(&entry.expected),
            to_hex(&entry.value)
        );
    }
    out
}

/// The resolutions the title with this ID offers, or none.
pub fn resolution_choices(title_id: &str, dir: &Path) -> Vec<String> {
    resolution_choices_for(crate::platform::Os::current(), title_id, dir)
}

fn resolution_choices_for(os: crate::platform::Os, title_id: &str, dir: &Path) -> Vec<String> {
    let set = if os == crate::platform::Os::Macos {
        PatchSet::load_macho(title_id, dir)
    } else {
        PatchSet::load(title_id, dir)
    };
    set.map(|s| s.resolution_choices().to_vec())
        .unwrap_or_default()
}

/// `WIDTHxHEIGHT` (either case of `x`), both positive.
pub fn parse_resolution(text: &str) -> Option<(u32, u32)> {
    let lower = text.to_lowercase();
    let mut parts = lower.split('x');
    let (width, height) = (parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let (width, height) = (width.parse::<u32>().ok()?, height.parse::<u32>().ok()?);
    (width > 0 && height > 0).then_some((width, height))
}

struct Segment {
    vaddr: u64,
    offset: u64,
    filesz: u64,
}

/// The file-backed `PT_LOAD` segments of a 64-bit little-endian ELF.
fn elf_segments(data: &[u8]) -> Result<Vec<Segment>, PatchError> {
    if data.len() < 0x40 || data[..4] != [0x7F, b'E', b'L', b'F'] || data[4] != 2 || data[5] != 1 {
        return Err(PatchError::NotElf);
    }
    let u16_at = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
    let u64_at = |o: usize| u64::from_le_bytes(data[o..o + 8].try_into().unwrap());
    let table = usize::try_from(u64_at(0x20)).map_err(|_| PatchError::NotElf)?;
    let size = usize::from(u16_at(0x36));
    let count = usize::from(u16_at(0x38));
    let mut segments = Vec::new();
    for i in 0..count {
        let at = table.checked_add(i * size).ok_or(PatchError::NotElf)?;
        if size < 0x38 || at.checked_add(0x38).is_none_or(|end| end > data.len()) {
            return Err(PatchError::NotElf);
        }
        let filesz = u64_at(at + 32);
        if u32_at(at) == 1 && filesz != 0 {
            segments.push(Segment {
                vaddr: u64_at(at + 16),
                offset: u64_at(at + 8),
                filesz,
            });
        }
    }
    Ok(segments)
}

fn macho_segments(data: &[u8]) -> Result<Vec<Segment>, PatchError> {
    if data.len() < 32 || data[..4] != [0xcf, 0xfa, 0xed, 0xfe] {
        return Err(PatchError::NotMacho);
    }
    let u32_at = |at| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
    let u64_at = |at| u64::from_le_bytes(data[at..at + 8].try_into().unwrap());
    let end = 32usize
        .checked_add(u32_at(20) as usize)
        .filter(|end| *end <= data.len())
        .ok_or(PatchError::NotMacho)?;
    let mut at = 32usize;
    let mut segments = Vec::new();
    for _ in 0..u32_at(16) {
        if at.checked_add(8).is_none_or(|end_at| end_at > end) {
            return Err(PatchError::NotMacho);
        }
        let size = u32_at(at + 4) as usize;
        let next = at
            .checked_add(size)
            .filter(|next| size >= 8 && *next <= end)
            .ok_or(PatchError::NotMacho)?;
        if u32_at(at) == 0x19 {
            if size < 72 {
                return Err(PatchError::NotMacho);
            }
            let segment = Segment {
                vaddr: u64_at(at + 24),
                offset: u64_at(at + 40),
                filesz: u64_at(at + 48),
            };
            if segment.vaddr.checked_add(segment.filesz).is_none() {
                return Err(PatchError::NotMacho);
            }
            if segment
                .offset
                .checked_add(segment.filesz)
                .is_none_or(|end| end > data.len() as u64)
            {
                return Err(PatchError::NotMacho);
            }
            segments.push(segment);
        }
        at = next;
    }
    Ok(segments)
}

fn file_offset(segments: &[Segment], address: u64, length: usize) -> Result<usize, PatchError> {
    let end = address
        .checked_add(length as u64)
        .ok_or(PatchError::NotInSegment(address))?;
    segments
        .iter()
        .find(|s| s.vaddr <= address && end <= s.vaddr + s.filesz)
        .and_then(|s| usize::try_from(s.offset + (address - s.vaddr)).ok())
        .ok_or(PatchError::NotInSegment(address))
}

fn parse_address(text: &str) -> Result<u64, PatchError> {
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    u64::from_str_radix(digits, 16).map_err(|_| PatchError::BadAddress(text.to_string()))
}

fn hex_bytes(text: &str) -> Result<Vec<u8>, PatchError> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return Err(PatchError::BadHex(text.to_string()));
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&text[i..i + 2], 16)
                .map_err(|_| PatchError::BadHex(text.to_string()))
        })
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// A patch name reduced to the relinker's `[a-z0-9_]`.
fn slug(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 64-bit ELF with one PT_LOAD: file offset 0x100, vaddr 0x400000, `filesz` bytes of `body`.
    fn elf(body: &[u8]) -> Vec<u8> {
        let mut data = vec![0u8; 0x100];
        data[..4].copy_from_slice(&[0x7F, b'E', b'L', b'F']);
        data[4] = 2;
        data[5] = 1;
        data[0x20..0x28].copy_from_slice(&0x40u64.to_le_bytes()); // e_phoff
        data[0x36..0x38].copy_from_slice(&0x38u16.to_le_bytes());
        data[0x38..0x3A].copy_from_slice(&2u16.to_le_bytes()); // two program headers
        // phdr 0: PT_NOTE, ignored
        data[0x40..0x44].copy_from_slice(&4u32.to_le_bytes());
        // phdr 1: PT_LOAD
        let p = 0x40 + 0x38;
        data[p..p + 4].copy_from_slice(&1u32.to_le_bytes());
        data[p + 8..p + 16].copy_from_slice(&0x100u64.to_le_bytes());
        data[p + 16..p + 24].copy_from_slice(&0x40_0000u64.to_le_bytes());
        data[p + 32..p + 40].copy_from_slice(&(body.len() as u64).to_le_bytes());
        data.extend_from_slice(body);
        data
    }

    const FILE: &str = r#"{
        "space": "elf",
        "patches": [
            {"name": "Keep Window-Size", "edits": [
                {"address": "0x400010", "original": "0f84d2000000", "replacement": "90e9d2000000"}]}
        ],
        "resolution": {"default": "1920x1080", "choices": ["1280x720", "1920x1080", "2560x1440"], "fields": [
            {"value": "width", "address": "0x400020", "instruction": "b800000000c3", "offset": 1},
            {"value": "height", "address": "0x400030", "instruction": "ba00000000c3", "offset": 1}]}
    }"#;

    fn body() -> Vec<u8> {
        let mut body = vec![0xCC; 0x40];
        body[0x10..0x16].copy_from_slice(&[0x0f, 0x84, 0xd2, 0, 0, 0]);
        body[0x20..0x26].copy_from_slice(&[0xb8, 0, 0, 0, 0, 0xc3]);
        body[0x30..0x36].copy_from_slice(&[0xba, 0, 0, 0, 0, 0xc3]);
        body
    }

    fn set() -> PatchSet {
        serde_json::from_str(FILE).unwrap()
    }

    #[test]
    fn macho_edits_are_checked_mapped_and_idempotent_without_accepting_elf() {
        let mut image = vec![0u8; 0x120];
        image[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
        image[16..20].copy_from_slice(&1u32.to_le_bytes());
        image[20..24].copy_from_slice(&72u32.to_le_bytes());
        image[32..36].copy_from_slice(&0x19u32.to_le_bytes());
        image[36..40].copy_from_slice(&72u32.to_le_bytes());
        image[56..64].copy_from_slice(&0x1000u64.to_le_bytes());
        image[72..80].copy_from_slice(&0x100u64.to_le_bytes());
        image[80..88].copy_from_slice(&32u64.to_le_bytes());
        image[0x104..0x106].copy_from_slice(&[0x11, 0x22]);
        let patches: PatchSet = serde_json::from_str(r#"{"patches":[{"name":"example","edits":[{"address":"0x1004","original":"1122","replacement":"3344"}]}]}"#).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("eboot");
        fs::write(&executable, &image).unwrap();
        assert_eq!(
            patches.apply_macho(&executable, None).unwrap()[0].1,
            PatchState::Applied
        );
        let modified = fs::read(&executable).unwrap();
        assert_eq!(&modified[0x104..0x106], &[0x33, 0x44]);
        assert_eq!(&modified[..0x104], &image[..0x104]);
        assert_eq!(&modified[0x106..], &image[0x106..]);
        assert_eq!(
            patches.apply_macho(&executable, None).unwrap()[0].1,
            PatchState::AlreadyApplied
        );
        assert!(matches!(
            patches.plan(&image, None),
            Err(PatchError::NotElf)
        ));
        image[56..64].copy_from_slice(&u64::MAX.to_le_bytes());
        fs::write(&executable, &image).unwrap();
        assert!(patches.apply_macho(&executable, None).is_err());
        assert_eq!(
            fs::read(&executable).unwrap(),
            image,
            "invalid input is never rewritten"
        );
    }

    #[test]
    fn load_reads_elf_space_files_only() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("PPSA1.json"), FILE).unwrap();
        fs::write(
            dir.path().join("PPSA2.json"),
            FILE.replace("\"elf\"", "\"macho\""),
        )
        .unwrap();
        fs::write(
            dir.path().join("PPSA3.json"),
            FILE.replace("\"space\": \"elf\",", ""),
        )
        .unwrap();
        fs::write(dir.path().join("PPSA4.json"), "{nope").unwrap();
        assert_eq!(
            PatchSet::load("PPSA1", dir.path()).unwrap().patches.len(),
            1
        );
        for id in ["PPSA2", "PPSA3", "PPSA4", "PPSA5"] {
            assert!(PatchSet::load(id, dir.path()).is_none(), "{id}");
        }
        assert_eq!(
            resolution_choices_for(crate::platform::Os::Linux, "PPSA1", dir.path()),
            ["1280x720", "1920x1080", "2560x1440"]
        );
        assert!(resolution_choices_for(crate::platform::Os::Linux, "PPSA2", dir.path()).is_empty());
    }

    #[test]
    fn the_empty_catalogue_offers_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(PatchSet::load("PPSA02100", dir.path()).is_none());
        assert!(resolution_choices("PPSA02100", dir.path()).is_empty());
        let empty: PatchSet = serde_json::from_str(r#"{"space":"elf"}"#).unwrap();
        assert!(empty.resolution_choices().is_empty());
        let (entries, states) = empty.plan(&elf(&body()), None).unwrap();
        assert!(entries.is_empty() && states.is_empty());
    }

    #[test]
    fn plan_applies_and_sets_the_resolution() {
        let (entries, states) = set().plan(&elf(&body()), Some("2560x1440")).unwrap();
        assert_eq!(
            states[0],
            ("Keep Window-Size".to_string(), PatchState::Applied)
        );
        assert_eq!(states[1], ("resolution".to_string(), PatchState::Applied));
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "keep_window_size_0");
        assert_eq!(entries[0].va, 0x400010);
        assert_eq!(entries[0].value, [0x90, 0xe9, 0xd2, 0, 0, 0]);
        assert_eq!(
            entries[1].value,
            [0xb8, 0x00, 0x0A, 0, 0, 0xc3],
            "2560 little-endian"
        );
        assert_eq!(
            entries[2].value,
            [0xba, 0xa0, 0x05, 0, 0, 0xc3],
            "1440 little-endian"
        );
    }

    #[test]
    fn unoffered_choice_falls_back_to_the_default() {
        let (entries, _) = set().plan(&elf(&body()), Some("999x999")).unwrap();
        assert_eq!(&entries[1].value[1..3], &1920u16.to_le_bytes());
        assert_eq!(
            set().effective_resolution(None).as_deref(),
            Some("1920x1080")
        );
        assert_eq!(
            set().effective_resolution(Some("1280x720")).as_deref(),
            Some("1280x720")
        );
    }

    #[test]
    fn plan_reports_already_applied_and_skipped() {
        let mut applied = body();
        applied[0x10..0x16].copy_from_slice(&[0x90, 0xe9, 0xd2, 0, 0, 0]);
        let (entries, states) = set().plan(&elf(&applied), None).unwrap();
        assert_eq!(states[0].1, PatchState::AlreadyApplied);
        assert_eq!(entries.len(), 2, "only the resolution fields");

        let mut other = body();
        other[0x10] = 0xAA;
        let (_, states) = set().plan(&elf(&other), None).unwrap();
        assert_eq!(
            states[0].1.to_string(),
            "skipped: 0x400010 holds aa84d2000000, expected 0f84d2000000"
        );
        assert_eq!(PatchState::Applied.to_string(), "applied");
        assert_eq!(PatchState::AlreadyApplied.to_string(), "already applied");

        let mut changed = body();
        changed[0x20] = 0x00;
        let (entries, states) = set().plan(&elf(&changed), None).unwrap();
        assert!(matches!(states[1].1, PatchState::Skipped { .. }));
        assert_eq!(entries.len(), 1, "no half-applied resolution");
    }

    #[test]
    fn plan_errors() {
        let mut outside = set();
        outside.patches[0].edits[0].address = "0x500000".into();
        assert_eq!(
            outside.plan(&elf(&body()), None).unwrap_err().to_string(),
            "0x500000 is not in a file-backed segment"
        );
        let mut short = set();
        short.patches[0].edits[0].replacement = "90".into();
        assert_eq!(
            short.plan(&elf(&body()), None).unwrap_err().to_string(),
            "Keep Window-Size: original and replacement at 0x400010 differ in length"
        );
        assert_eq!(
            set().plan(
                b"not an elf at all, really not an elf at all, no elf here..............",
                None
            ),
            Err(PatchError::NotElf)
        );
        let mut edge = set();
        edge.patches[0].edits[0].address = "0x40003d".into();
        assert!(
            matches!(
                edge.plan(&elf(&body()), None),
                Err(PatchError::NotInSegment(0x40003d))
            ),
            "straddles the segment end"
        );
        let mut bad = set();
        bad.patches[0].edits[0].original = "zz".into();
        assert!(matches!(
            bad.plan(&elf(&body()), None),
            Err(PatchError::BadHex(_))
        ));
        bad = set();
        bad.patches[0].edits[0].address = "nowhere".into();
        assert!(matches!(
            bad.plan(&elf(&body()), None),
            Err(PatchError::BadAddress(_))
        ));
    }

    #[test]
    fn parse_resolution_cases() {
        assert_eq!(parse_resolution("1920x1080"), Some((1920, 1080)));
        assert_eq!(parse_resolution("1920X1080"), Some((1920, 1080)));
        assert_eq!(parse_resolution("0x5"), None);
        assert_eq!(parse_resolution("1920"), None);
        assert_eq!(parse_resolution("ax2"), None);
        assert_eq!(parse_resolution("1x2x3"), None);
        assert_eq!(parse_resolution(""), None);
    }

    #[test]
    fn toml_golden() {
        let entries = vec![
            PatchEntry {
                name: "keep_window_size_0".into(),
                va: 0x40F675E,
                expected: vec![0x0f, 0x84, 0xd2, 0, 0, 0],
                value: vec![0x90, 0xe9, 0xd2, 0, 0, 0],
            },
            PatchEntry {
                name: "resolution_width_0".into(),
                va: 0x400020,
                expected: vec![0xb8, 0, 0, 0, 0, 0xc3],
                value: vec![0xb8, 0, 0x0A, 0, 0, 0xc3],
            },
        ];
        let expected = "format = 1

[[patch]]
name = \"keep_window_size_0\"
target = \"eboot.bin\"
va = 0x40F675E
expected = \"0f84d2000000\"
value = \"90e9d2000000\"

[[patch]]
name = \"resolution_width_0\"
target = \"eboot.bin\"
va = 0x400020
expected = \"b800000000c3\"
value = \"b8000a0000c3\"
";
        assert_eq!(relinker_toml(&entries), expected);
        assert_eq!(relinker_toml(&[]), "format = 1\n");
    }

    #[test]
    fn plan_to_toml_end_to_end() {
        let (entries, _) = set().plan(&elf(&body()), Some("1280x720")).unwrap();
        let toml = relinker_toml(&entries);
        assert_eq!(toml.matches("[[patch]]").count(), 3);
        assert!(toml.contains("value = \"b800050000c3\""), "1280 = 0x500");
        assert!(toml.contains("value = \"bad0020000c3\""), "720 = 0x2d0");
    }
}
