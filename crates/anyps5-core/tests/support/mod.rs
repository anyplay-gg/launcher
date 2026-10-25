//! Shared fixtures of the integration tests: the fake relinker/game binary, synthetic dumps and
//! toolkits. No game data of any kind.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Path of the `fake-relinker` binary, built on first use into the test run's target directory.
pub fn fake_binary() -> &'static Path {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let exe = std::env::current_exe().expect("test exe");
        let profile_dir = exe
            .parent()
            .and_then(Path::parent)
            .expect("profile dir")
            .to_path_buf();
        let target_dir = profile_dir.parent().expect("target dir").to_path_buf();
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
        command
            .args(["build", "-p", "fake-relinker", "--manifest-path"])
            .arg(workspace.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", &target_dir);
        if profile_dir.file_name().is_some_and(|n| n == "release") {
            command.arg("--release");
        }
        let status = command.status().expect("run cargo");
        assert!(status.success(), "building fake-relinker failed");
        profile_dir.join(format!("fake-relinker{}", std::env::consts::EXE_SUFFIX))
    })
}

pub const MINGW: &[&str] = anyps5_core::platform::Os::current().runtime_libs();

pub fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// Settings for the fake in a folder (`fake.env` beside the copied executable).
pub fn configure_fake(dir: &Path, lines: &[&str]) {
    write(&dir.join("fake.env"), lines.join("\n").as_bytes());
}

/// A toolkit folder whose relinker is the fake.
pub fn make_toolkit(root: &Path, relinker_settings: &[&str]) -> anyps5_core::toolkit::Toolkit {
    fs::create_dir_all(root.join("libs")).unwrap();
    fs::copy(
        fake_binary(),
        root.join(anyps5_core::platform::Os::current().relinker_file()),
    )
    .unwrap();
    configure_fake(root, relinker_settings);
    if cfg!(target_os = "macos") {
        write(&root.join("vulkan/libMoltenVK.dylib"), b"fixture runtime");
        write(&root.join("vulkan/libvulkan.1.dylib"), b"fixture loader");
        write(
            &root.join("vulkan/MoltenVK_icd.json"),
            br#"{"ICD":{"library_path":"libMoltenVK.dylib"}}"#,
        );
    }
    write(&root.join("libs/libSceLibc.prx"), b"PRX-LIBC");
    write(&root.join("libs/libSceFoo.prx"), b"PRX-FOO");
    for dll in MINGW {
        write(&root.join("libs").join(dll), b"DLL");
    }
    write(&root.join("version.txt"), b"0.1.3-aaaaaaaaaaaa\n");
    anyps5_core::toolkit::Toolkit::open(root).expect("fake toolkit opens")
}

/// 4352 bytes of a decrypted SELF container (V1 of PORT.md appendix A).
pub fn self_v1() -> Vec<u8> {
    fn le(v: u64, n: usize) -> Vec<u8> {
        v.to_le_bytes()[..n].to_vec()
    }
    let mut d = vec![0u8; 0x1100];
    d[0..4].copy_from_slice(&0x1D3D154Fu32.to_le_bytes());
    // One record: flags, segment, size, size.
    for (i, v) in [0x800u64, 0x1000, 0x100, 0x100].iter().enumerate() {
        d[0x40 + 8 * i..0x48 + 8 * i].copy_from_slice(&le(*v, 8));
    }
    // Inner ELF header at 0x100.
    let mut h = vec![0u8; 64];
    h[0..8].copy_from_slice(b"\x7fELF\x02\x01\x01\x00");
    h[16..18].copy_from_slice(&le(2, 2));
    h[18..20].copy_from_slice(&le(0x3E, 2));
    h[20..24].copy_from_slice(&le(1, 4));
    h[24..32].copy_from_slice(&le(0x401000, 8));
    h[32..40].copy_from_slice(&le(0x40, 8));
    h[52..54].copy_from_slice(&le(64, 2));
    h[54..56].copy_from_slice(&le(56, 2));
    h[56..58].copy_from_slice(&le(1, 2));
    d[0x100..0x140].copy_from_slice(&h);
    // One PT_LOAD at inner + phoff.
    let mut ph = Vec::new();
    ph.extend(le(1, 4));
    ph.extend(le(5, 4));
    for v in [0x400u64, 0x400000, 0x400000, 0x100, 0x100, 0x1000] {
        ph.extend(le(v, 8));
    }
    d[0x140..0x140 + 56].copy_from_slice(&ph);
    for i in 0..0x100usize {
        d[0x1000 + i] = ((7 + i * 13 + (i >> 3)) % 61) as u8;
    }
    d
}

/// The same container with 256 distinct bytes in the load segment: reads as encrypted.
pub fn self_encrypted() -> Vec<u8> {
    let mut d = self_v1();
    for i in 0..256usize {
        d[0x1000 + i] = i as u8;
    }
    d
}

pub fn elf_bytes() -> Vec<u8> {
    let mut d = b"\x7fELF\x02\x01\x01\x00".to_vec();
    d.extend([0u8; 120]);
    d
}

/// A dump folder: a SELF `eboot.bin` (or the bytes given), metadata, a module in `sce_module`
/// and one Unity plugin.
pub fn make_dump(folder: &Path, title_id: &str, eboot: &[u8]) {
    write(&folder.join("eboot.bin"), eboot);
    write(
        &folder.join("sce_sys/param.json"),
        format!(r#"{{"titleId":"{title_id}","contentVersion":"01.000.000"}}"#).as_bytes(),
    );
    write(&folder.join("sce_sys/icon0.png"), b"PNG");
    write(&folder.join("sce_module/libFoo.prx"), &elf_bytes());
    write(&folder.join("Media/Plugins/libPlugin.prx"), &elf_bytes());
    write(&folder.join("data/level1.pak"), b"LEVEL-ONE-DATA");
}

/// Collects events from a sink and waits for them.
pub struct Events<T>(pub Arc<Mutex<Vec<T>>>);

impl<T> Clone for Events<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Events<T> {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Vec::new())))
    }

    pub fn push(&self, event: T) {
        self.0.lock().unwrap().push(event);
    }

    pub fn wait_until(&self, what: &str, timeout: Duration, mut done: impl FnMut(&[T]) -> bool) {
        let start = Instant::now();
        loop {
            if done(&self.0.lock().unwrap()) {
                return;
            }
            assert!(start.elapsed() < timeout, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
