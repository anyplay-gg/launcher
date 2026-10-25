//! A toolkit for one game (docs/DESIGN.md 3.8): the release a game stays on while every other game
//! follows the global choice, which of the installed releases may go, and what happens when the
//! release a game stays on is no longer on this PC.
//!
//! A game's pin is a release tag in its settings. It names a folder below `Toolkit\` and nothing
//! else: a tag that is not dotted numbers is never joined to a path. Installing and checking a
//! release is the job of the parent module; this one only looks for the folders it made.

use super::*;

/// Which toolkit a game is unlocked and started with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameToolkit {
    /// The game follows the global choice; its version, `None` when nothing is installed.
    Global(Option<String>),
    /// The game stays on an installed release.
    Pinned { tag: String, version: String },
    /// The game stays on a release that is not (or no longer) usable on this PC. Nothing else is
    /// used in its place: the player decides.
    Missing { tag: String },
}

impl GameToolkit {
    /// The version (`0.1.2-4f2a9c`) the game would be unlocked with; `None` when there is none.
    pub fn version(&self) -> Option<&str> {
        match self {
            GameToolkit::Global(version) => version.as_deref(),
            GameToolkit::Pinned { version, .. } => Some(version),
            GameToolkit::Missing { .. } => None,
        }
    }

    pub fn is_missing(&self) -> bool {
        matches!(self, GameToolkit::Missing { .. })
    }

    /// The release the game stays on, present or not; `None` when it follows the global choice.
    pub fn pinned_tag(&self) -> Option<&str> {
        match self {
            GameToolkit::Global(_) => None,
            GameToolkit::Pinned { tag, .. } | GameToolkit::Missing { tag } => Some(tag),
        }
    }
}

/// `v0.1.2` and `0.1.2` are the release `0.1.2`; anything that is not a release tag is nothing.
pub fn pin_tag(pin: &str) -> Option<String> {
    let plain = pin.trim().trim_start_matches('v');
    parse_tag(plain)?;
    Some(plain.to_owned())
}

/// Whether the folder of an installed release has what the toolkit needs on `os`. The same files
/// [`Toolkit::open`] asks for, without hashing them: this runs for every pinned game on every frame.
fn usable(os: Os, root: &Path) -> bool {
    root.join(os.relinker_file()).is_file()
        && has_prx(&root.join("libs"))
        && os
            .runtime_libs()
            .iter()
            .all(|dll| root.join("libs").join(dll).is_file())
        && (os != Os::Macos
            || [
                "libvulkan.1.dylib",
                "libMoltenVK.dylib",
                "MoltenVK_icd.json",
            ]
            .iter()
            .all(|name| root.join("vulkan").join(name).is_file()))
}

/// The version of the installed release `pin`, read from its folder; `None` when it is not there
/// or incomplete.
pub fn installed_version(paths: &AppPaths, pin: &str) -> Option<String> {
    installed_version_for(Os::current(), paths, pin)
}

pub(super) fn installed_version_for(os: Os, paths: &AppPaths, pin: &str) -> Option<String> {
    let tag = pin_tag(pin)?;
    let root = paths.toolkit_dir().join(&tag);
    if !usable(os, &root) {
        return None;
    }
    let version = fs::read_to_string(root.join("version.txt"))
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());
    Some(version.unwrap_or(tag))
}

/// The toolkit of a game: the release it stays on if it has a pin, else the global one
/// (`global`, the version in use). A pin never falls back to the global toolkit.
pub fn game_toolkit(paths: &AppPaths, global: Option<&str>, pin: Option<&str>) -> GameToolkit {
    let Some(pin) = pin else {
        return GameToolkit::Global(global.map(str::to_owned));
    };
    match (pin_tag(pin), installed_version(paths, pin)) {
        (Some(tag), Some(version)) => GameToolkit::Pinned { tag, version },
        (tag, _) => GameToolkit::Missing {
            tag: tag.unwrap_or_else(|| pin.trim().trim_start_matches('v').to_owned()),
        },
    }
}

/// The opened toolkit to unlock a game with: the pinned release, else `global`. `Ok(None)` when the
/// game follows the global choice and there is none. A pin that cannot be opened is an error, never
/// the global toolkit.
pub fn open_for_game(
    paths: &AppPaths,
    global: Option<&Toolkit>,
    pin: Option<&str>,
) -> Result<Option<Toolkit>, ToolkitError> {
    let Some(pin) = pin else {
        return Ok(global.cloned());
    };
    let tag = pin_tag(pin).ok_or_else(|| ToolkitError::PinMissing(pin.to_owned()))?;
    Toolkit::open(&paths.toolkit_dir().join(&tag))
        .map(Some)
        .map_err(|_| ToolkitError::PinMissing(tag))
}

/// The tags of the usable releases on this PC, newest first.
pub fn installed_tags(paths: &AppPaths) -> Vec<String> {
    installed_releases(paths)
        .into_iter()
        .filter(|(_, dir)| usable(Os::current(), dir))
        .filter_map(|(_, dir)| Some(dir.file_name()?.to_string_lossy().into_owned()))
        .collect()
}

/// An installed release and the room it takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledRelease {
    pub tag: String,
    pub bytes: u64,
}

/// The installed releases with their sizes, newest first. Walks the folders: call from a worker.
pub fn installed_with_sizes(paths: &AppPaths, cancel: &CancelToken) -> Vec<InstalledRelease> {
    installed_releases(paths)
        .into_iter()
        .filter_map(|(_, dir)| {
            let tag = dir.file_name()?.to_string_lossy().into_owned();
            Some(InstalledRelease {
                bytes: crate::fsx::tree_size(&dir, cancel),
                tag,
            })
        })
        .collect()
}

/// Deletes an installed release. Refused for a tag in `in_use` (the toolkit in use, the global
/// pin, the pin of a game).
pub fn remove_release(paths: &AppPaths, tag: &str, in_use: &[String]) -> Result<(), ToolkitError> {
    let tag = pin_tag(tag).ok_or_else(|| ToolkitError::PinMissing(tag.to_owned()))?;
    if in_use
        .iter()
        .any(|used| pin_tag(used).as_deref() == Some(tag.as_str()))
    {
        return Err(ToolkitError::InUse(tag));
    }
    crate::fsx::remove_tree_retry(&paths.toolkit_dir().join(&tag))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Os; 3] = [Os::Windows, Os::Linux, Os::Macos];

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    /// A release folder as the installer lays it out on `os`.
    fn install(os: Os, paths: &AppPaths, tag: &str) -> PathBuf {
        let root = paths.toolkit_dir().join(tag);
        let relinker: &[u8] = if os == Os::Macos {
            b"\xcf\xfa\xed\xfeRELINKER"
        } else {
            b"RELINKER"
        };
        write(&root.join(os.relinker_file()), relinker);
        write(&root.join("libs/a.prx"), b"AAA");
        for dll in os.runtime_libs() {
            write(&root.join("libs").join(dll), b"D");
        }
        if os == Os::Macos {
            for name in ["libvulkan.1.dylib", "libMoltenVK.dylib"] {
                write(&root.join("vulkan").join(name), b"runtime fixture");
            }
            write(&root.join("vulkan/MoltenVK_icd.json"), br#"{"ICD":{}}"#);
        }
        write(
            &root.join("version.txt"),
            format!("{tag}-abc123\n").as_bytes(),
        );
        root
    }

    fn paths() -> (tempfile::TempDir, AppPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        (dir, paths)
    }

    #[test]
    fn only_a_release_tag_names_a_folder() {
        assert_eq!(pin_tag("v0.1.2").as_deref(), Some("0.1.2"));
        assert_eq!(pin_tag("0.1.2").as_deref(), Some("0.1.2"));
        assert_eq!(pin_tag(" v0.2 ").as_deref(), Some("0.2"));
        for bad in [
            "",
            "v",
            "latest",
            "..",
            "../0.1.2",
            "0.1.2/../x",
            r"0.1.2\x",
            r"C:\x",
        ] {
            assert_eq!(pin_tag(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn every_layout_resolves_a_pinned_release() {
        for os in ALL {
            let (_dir, paths) = paths();
            let root = install(os, &paths, "0.1.2");
            assert_eq!(
                installed_version_for(os, &paths, "v0.1.2").as_deref(),
                Some("0.1.2-abc123"),
                "{os:?}"
            );
            assert!(Toolkit::open_for(os, &root).is_ok(), "{os:?} opens");
            assert_eq!(installed_version_for(os, &paths, "v0.1.3"), None, "{os:?}");
        }
    }

    #[test]
    fn an_incomplete_release_is_not_usable() {
        for os in ALL {
            let (_dir, paths) = paths();
            let root = install(os, &paths, "0.1.2");
            fs::remove_file(root.join("libs/a.prx")).unwrap();
            assert_eq!(
                installed_version_for(os, &paths, "0.1.2"),
                None,
                "{os:?} no libs"
            );
            let root = install(os, &paths, "0.1.2");
            fs::remove_file(root.join(os.relinker_file())).unwrap();
            assert_eq!(
                installed_version_for(os, &paths, "0.1.2"),
                None,
                "{os:?} no relinker"
            );
        }
        let (_dir, paths) = paths();
        let root = install(Os::Windows, &paths, "0.1.2");
        fs::remove_file(root.join("libs/libgcc_s_seh-1.dll")).unwrap();
        assert_eq!(installed_version_for(Os::Windows, &paths, "0.1.2"), None);
        let root = install(Os::Macos, &paths, "0.1.4");
        fs::remove_file(root.join("vulkan/libMoltenVK.dylib")).unwrap();
        assert_eq!(installed_version_for(Os::Macos, &paths, "0.1.4"), None);
    }

    #[test]
    fn a_game_follows_the_global_toolkit_unless_it_has_a_pin() {
        let (_dir, paths) = paths();
        install(Os::current(), &paths, "0.1.2");
        assert_eq!(
            game_toolkit(&paths, Some("0.1.3-xyz"), None),
            GameToolkit::Global(Some("0.1.3-xyz".into()))
        );
        assert_eq!(game_toolkit(&paths, None, None), GameToolkit::Global(None));
        let pinned = game_toolkit(&paths, Some("0.1.3-xyz"), Some("v0.1.2"));
        assert_eq!(
            pinned,
            GameToolkit::Pinned {
                tag: "0.1.2".into(),
                version: "0.1.2-abc123".into()
            }
        );
        assert_eq!(pinned.version(), Some("0.1.2-abc123"));
        assert_eq!(pinned.pinned_tag(), Some("0.1.2"));
        assert_eq!(
            game_toolkit(&paths, Some("0.1.3-xyz"), None).pinned_tag(),
            None
        );
    }

    #[test]
    fn a_missing_pin_never_falls_back_to_the_global_toolkit() {
        let (_dir, paths) = paths();
        install(Os::current(), &paths, "0.1.3");
        let missing = game_toolkit(&paths, Some("0.1.3-abc123"), Some("v0.1.2"));
        assert_eq!(
            missing,
            GameToolkit::Missing {
                tag: "0.1.2".into()
            }
        );
        assert!(missing.is_missing());
        assert_eq!(missing.version(), None);
        // A pin that is not even a tag is missing too, and is never joined to a path.
        let odd = game_toolkit(&paths, Some("0.1.3-abc123"), Some("../0.1.3"));
        assert!(odd.is_missing());
        assert!(matches!(
            open_for_game(&paths, None, Some("../0.1.3")),
            Err(ToolkitError::PinMissing(_))
        ));
    }

    #[test]
    fn opening_for_a_game_uses_its_own_release() {
        let (_dir, paths) = paths();
        let os = Os::current();
        install(os, &paths, "0.1.2");
        let global_root = install(os, &paths, "0.1.3");
        let global = Toolkit::open(&global_root).unwrap();
        let followed = open_for_game(&paths, Some(&global), None).unwrap().unwrap();
        assert_eq!(followed.tag(), Some("0.1.3"));
        assert!(open_for_game(&paths, None, None).unwrap().is_none());
        let pinned = open_for_game(&paths, Some(&global), Some("v0.1.2"))
            .unwrap()
            .unwrap();
        assert_eq!(pinned.tag(), Some("0.1.2"));
        assert_eq!(pinned.version(), "0.1.2-abc123");
        assert!(matches!(
            open_for_game(&paths, Some(&global), Some("v0.1.1")),
            Err(ToolkitError::PinMissing(tag)) if tag == "0.1.1"
        ));
    }

    #[test]
    fn pruned_releases_spare_the_ones_games_stay_on() {
        let (_dir, paths) = paths();
        for tag in ["0.1.0", "0.1.1", "0.1.2", "0.1.3"] {
            install(Os::current(), &paths, tag);
        }
        prune_old_releases(&paths, "0.1.3", &["v0.1.0".to_owned()]);
        assert_eq!(installed_tags(&paths), ["0.1.3", "0.1.2", "0.1.0"]);
        // The tag a pin names may carry a `v`; a pin that is not a tag spares nothing.
        prune_old_releases(&paths, "0.1.3", &["nonsense".to_owned()]);
        assert_eq!(installed_tags(&paths), ["0.1.3", "0.1.2"]);
    }

    #[test]
    fn an_unused_release_can_be_removed_and_a_used_one_cannot() {
        let (_dir, paths) = paths();
        for tag in ["0.1.2", "0.1.3"] {
            install(Os::current(), &paths, tag);
        }
        let sizes = installed_with_sizes(&paths, &CancelToken::new());
        assert_eq!(sizes.len(), 2);
        assert_eq!(sizes[0].tag, "0.1.3");
        assert!(sizes.iter().all(|s| s.bytes > 0));
        assert!(matches!(
            remove_release(&paths, "0.1.2", &["v0.1.2".to_owned()]),
            Err(ToolkitError::InUse(_))
        ));
        assert!(paths.toolkit_dir().join("0.1.2").is_dir());
        remove_release(&paths, "v0.1.2", &["0.1.3".to_owned()]).unwrap();
        assert_eq!(installed_tags(&paths), ["0.1.3"]);
        assert!(matches!(
            remove_release(&paths, "../0.1.3", &[]),
            Err(ToolkitError::PinMissing(_))
        ));
        assert_eq!(installed_tags(&paths), ["0.1.3"]);
    }
}
