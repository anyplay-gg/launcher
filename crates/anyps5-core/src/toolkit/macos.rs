//! The macOS toolkit is one archive, including its Vulkan runtime and file manifest.

use super::*;
use std::collections::{BTreeMap, HashSet};

pub(super) fn validate(relinker: &Path, root: &Path) -> Result<(), ToolkitError> {
    crate::platform::require_macho(relinker)?;
    for name in [
        "libvulkan.1.dylib",
        "libMoltenVK.dylib",
        "MoltenVK_icd.json",
    ] {
        if !root.join("vulkan").join(name).is_file() {
            return Err(ToolkitError::Missing(format!("vulkan/{name}")));
        }
    }
    Ok(())
}

/// Reject unsafe entries before extraction, then verify every extracted file against the manifest.
pub(super) fn extract(
    zip_path: &Path,
    destination: &Path,
    tag: &str,
    progress: &mut dyn FnMut(DownloadStage, f64),
    cancel: &CancelToken,
) -> Result<(), ToolkitError> {
    let bad = |message: String| ToolkitError::BadArchive(message);
    let mut archive =
        zip::ZipArchive::new(fs::File::open(zip_path)?).map_err(|e| bad(e.to_string()))?;
    let prefix = format!("AnyPS5-toolkit-macos-{tag}/");
    let mut files = Vec::new();
    let mut names = HashSet::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|e| bad(e.to_string()))?;
        let safe =
            safe_entry_name(entry.name()).ok_or_else(|| bad("unsafe toolkit path".into()))?;
        let relative = safe
            .strip_prefix(&prefix)
            .ok_or_else(|| bad("unexpected toolkit root".into()))?;
        if entry.is_dir() {
            continue;
        }
        if relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
            || !names.insert(relative.to_lowercase())
        {
            return Err(bad("duplicate or empty toolkit path".into()));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o100000))
        {
            return Err(bad(
                "toolkit archives must contain only regular files".into()
            ));
        }
        total = total
            .checked_add(entry.size())
            .ok_or(ToolkitError::TooLarge)?;
        if total > 2 * 1024 * 1024 * 1024 {
            return Err(ToolkitError::TooLarge);
        }
        files.push((index, relative.to_owned(), entry.size()));
    }
    let mut hashes = BTreeMap::new();
    for (position, (index, name, size)) in files.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(ToolkitError::Cancelled);
        }
        let mut entry = archive.by_index(*index).map_err(|e| bad(e.to_string()))?;
        let path = destination.join(name);
        fs::create_dir_all(path.parent().expect("file parent"))?;
        let mut output = fs::File::create_new(&path)?;
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        let mut copied = 0u64;
        loop {
            if cancel.is_cancelled() {
                return Err(ToolkitError::Cancelled);
            }
            let n = entry.read(&mut buffer).map_err(|e| bad(e.to_string()))?;
            if n == 0 {
                break;
            }
            copied += n as u64;
            if copied > *size {
                return Err(ToolkitError::TooLarge);
            }
            output.write_all(&buffer[..n])?;
            digest.update(&buffer[..n]);
        }
        if copied != *size {
            return Err(ToolkitError::Digest);
        }
        if name != "toolkit.json" {
            hashes.insert(name.clone(), hex(&digest.finalize()));
        }
        progress(
            DownloadStage::Extracting,
            (position + 1) as f64 / files.len().max(1) as f64,
        );
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(destination.join("toolkit.json"))?)
            .map_err(|e| bad(e.to_string()))?;
    if manifest["format"].as_u64() != Some(1) {
        return Err(bad("unsupported toolkit manifest format".into()));
    }
    let declared: BTreeMap<String, String> =
        serde_json::from_value(manifest["files"].clone()).map_err(|e| bad(e.to_string()))?;
    if declared != hashes {
        return Err(ToolkitError::Digest);
    }
    validate(&destination.join("relinker"), destination)?;
    Ok(())
}
