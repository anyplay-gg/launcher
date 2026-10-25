//! Native game-window detection and activation, without a timed "window shown" assumption.

use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::window;

fn number(dictionary: &CFDictionary<CFString, CFType>, key: &str) -> Option<i64> {
    dictionary
        .find(CFString::new(key))?
        .downcast::<CFNumber>()?
        .to_i64()
}

/// The owner of a visible, normal game window in this process group, including LoadExec children.
pub fn find_window(group: u32) -> Option<isize> {
    let group = i32::try_from(group).ok()?;
    let windows = window::create_window_list(window::kCGWindowListOptionOnScreenOnly, 0)?;
    let descriptions = window::create_description_from_array(windows)?;
    for entry in &descriptions {
        let Some(pid) = number(&entry, "kCGWindowOwnerPID").and_then(|n| i32::try_from(n).ok())
        else {
            continue;
        };
        // SAFETY: getpgid only reads process metadata. It returns -1 for a departed process.
        if pid != group && unsafe { libc::getpgid(pid) } != group {
            continue;
        }
        if number(&entry, "kCGWindowLayer") != Some(0) {
            continue;
        }
        let Some(value) = entry.find(CFString::new("kCGWindowBounds")) else {
            continue;
        };
        let Some(bounds) = value.downcast::<CFDictionary>() else {
            continue;
        };
        // SAFETY: the dynamic type was checked above; CoreGraphics bounds use string keys.
        let bounds: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(bounds.as_concrete_TypeRef()) };
        if number(&bounds, "Width").is_some_and(|n| n > 200)
            && number(&bounds, "Height").is_some_and(|n| n > 150)
        {
            return Some(pid as isize);
        }
    }
    None
}

/// Bring the actual window owner forward, rather than the relinker's parent process.
#[allow(deprecated)]
pub fn activate(pid: isize) {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    if let Ok(pid) = i32::try_from(pid)
        && pid > 0
        && let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
    {
        app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
    }
}

/// Rosetta is required for the x86-64 runtime on Apple silicon. This never installs it silently.
pub fn require_rosetta_for(executable: &std::path::Path) -> std::io::Result<()> {
    use std::io::Read;
    let mut header = [0u8; 8];
    std::fs::File::open(executable)?.read_exact(&mut header)?;
    // Native arm64 tools and universal executables can start without translation. The published
    // runtime and relinked games are thin x86-64 Mach-O binaries (CPU_TYPE_X86_64).
    let x86_64 = header[..4] == [0xcf, 0xfa, 0xed, 0xfe]
        && u32::from_le_bytes(header[4..8].try_into().expect("CPU type")) == 0x0100_0007;
    if cfg!(target_arch = "aarch64") && x86_64 {
        let status = std::process::Command::new("/usr/bin/arch")
            .args(["-x86_64", "/usr/bin/true"])
            .status()?;
        if !status.success() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Rosetta 2 is required to convert and run games; install Rosetta and try again",
            ));
        }
    }
    Ok(())
}
