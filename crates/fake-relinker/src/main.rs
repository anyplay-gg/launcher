//! Test helper, never shipped. One binary that plays two parts, chosen by the name it is started
//! under (tests copy it to `relinker.exe` and to `eboot.exe`):
//!
//! * `relinker*`: takes the relinker's command line, prints progress in the real format with `\r`
//!   rewrites, writes the output executable and converted modules. `FAKE_RELINKER` selects a
//!   behaviour: `ok` (default), `fail` (exit 2), `nothing` (exit 0, no output), `sleep` (hangs).
//! * `eboot*`: behaves like a game process: prints to stdout and stderr, optionally opens a
//!   window, spawns a child, and exits with a chosen code. See `game()` for the variables.
//!
//! The relinker output file contains the arguments it was started with, so tests can check them.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

fn main() {
    let exe = env::current_exe().unwrap_or_default();
    let stem = exe
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if env::args().nth(1).as_deref() == Some("--fake-sleeper") {
        thread::sleep(Duration::from_millis(env_u64("FAKE_CHILD_MS", 60_000)));
        return;
    }
    if stem.starts_with("eboot") || setting("FAKE_GAME").is_some() {
        game();
    } else {
        relinker();
    }
}

/// A setting: the sidecar file `fake.env` beside the executable (KEY=VALUE per line) wins over
/// the process environment, so tests can configure a copy without touching global state.
fn setting(name: &str) -> Option<String> {
    let sidecar = env::current_exe().ok()?.with_file_name("fake.env");
    if let Ok(text) = fs::read_to_string(sidecar) {
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=')
                && key.trim() == name
            {
                return Some(value.trim().to_string());
            }
        }
    }
    env::var(name).ok()
}

fn env_u64(name: &str, default: u64) -> u64 {
    setting(name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn pause() {
    let ms = env_u64("FAKE_DELAY_MS", 0);
    if ms > 0 {
        thread::sleep(Duration::from_millis(ms));
    }
}

fn progress(out: &mut impl Write, phase: &str, file: &str, percent: u32, extra: &str) {
    let _ = write!(out, "{phase:<22} | {file:<12} | {percent:>3}%{extra}\r");
    let _ = out.flush();
    pause();
}

fn relinker() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: relinker --windows [options] <input.elf> <output.exe>");
        std::process::exit(1);
    }
    let mut module_dirs = Vec::new();
    let mut positional = Vec::new();
    let mut windows = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--windows" => windows = true,
            "--macos" | "--to-rosetta" => {}
            "--module-dir"
            | "--patches"
            | "--rpath"
            | "--windows-fixed-base"
            | "--linux-fixed-base" => {
                let value = iter.next().cloned().unwrap_or_default();
                if arg == "--module-dir" {
                    module_dirs.push(value);
                }
            }
            a if a.starts_with("--") => {}
            _ => positional.push(arg.clone()),
        }
    }
    // The Linux relinker has no format flag; the Windows one needs `--windows`.
    if (cfg!(windows) && !windows) || positional.len() != 2 {
        eprintln!("bad arguments: {args:?}");
        std::process::exit(1);
    }
    let (input, output) = (PathBuf::from(&positional[0]), PathBuf::from(&positional[1]));
    let magic = fs::read(&input)
        .map(|b| b.starts_with(b"\x7fELF"))
        .unwrap_or(false);
    if !magic {
        eprintln!("FAIL: {} is not an ELF file", input.display());
        std::process::exit(2);
    }
    let mut err = std::io::stderr();
    let mode = setting("FAKE_RELINKER").unwrap_or_default();

    progress(&mut err, "Executable", "eboot.bin", 100, "");
    for percent in [0, 42, 100] {
        progress(&mut err, "Instruction scan", "eboot.bin", percent, "");
    }
    if mode == "sleep" {
        thread::sleep(Duration::from_millis(env_u64("FAKE_SLEEP_MS", 60_000)));
    }
    if mode == "fail" {
        let _ = writeln!(err);
        eprintln!("FAIL: conversion failed: unsupported instruction at 0x1234");
        std::process::exit(2);
    }
    for percent in [50, 100] {
        progress(&mut err, "Instruction conversion", "eboot.bin", percent, "");
    }
    progress(&mut err, "Syscall scan", "eboot.bin", 100, "");

    // Converted modules keep their relative path under app0/.
    let out_dir = output.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut modules = Vec::new();
    for dir in &module_dirs {
        collect_modules(Path::new(dir), &mut modules);
    }
    modules.sort();
    modules.dedup();
    let total = modules.len();
    for (index, module) in modules.iter().enumerate() {
        let name = module
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        progress(
            &mut err,
            "PRX conversion",
            &name,
            100,
            &format!(" ({}/{total})", index + 1),
        );
        let target = out_dir
            .join("app0")
            .join(module.parent().unwrap_or(Path::new("")))
            .join(format!("{name}.guest.prx"));
        let _ = fs::create_dir_all(target.parent().unwrap());
        let _ = fs::write(&target, b"FAKE-GUEST-MODULE");
    }
    progress(&mut err, "Output image", "eboot.bin", 100, "");
    if mode == "nothing" {
        let _ = writeln!(err);
        return;
    }
    progress(&mut err, "Write output", "eboot.bin", 100, "");
    let _ = writeln!(err);
    let icon = input
        .parent()
        .unwrap_or(Path::new(""))
        .join("sce_sys/icon0.png")
        .is_file();
    let _ = fs::create_dir_all(&out_dir);
    let mut body = String::from("FAKE-PE\n");
    body.push_str(&format!(
        "icon={icon}\ncwd={}\n",
        env::current_dir().unwrap_or_default().display()
    ));
    for arg in &args {
        body.push_str(&format!("arg={arg}\n"));
    }
    if fs::write(&output, body).is_err() {
        eprintln!("FAIL: cannot write {}", output.display());
        std::process::exit(2);
    }
    println!("done");
}

/// Relative paths (to the current directory) of .prx/.sprx files in a module folder.
fn collect_modules(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if dir == Path::new(".") {
                // `--module-dir .` means the parent itself; only look at the usual module folders
                // and at files directly in it.
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if ["sce_module", "sce_modules", "prx"].contains(&name.as_str()) {
                    collect_modules(&path, out);
                }
            }
            continue;
        }
        let name = path.to_string_lossy().to_lowercase();
        if name.ends_with(".prx") || name.ends_with(".sprx") {
            out.push(path.strip_prefix("./").unwrap_or(&path).to_path_buf());
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The game part.
//
//   FAKE_GAME_LINES   number of stdout/stderr line pairs to print (default 3)
//   FAKE_GAME_EXIT    exit code (default 0)
//   FAKE_GAME_SLEEP_MS  stay alive this long before exiting (default 0)
//   FAKE_GAME_WINDOW  "WxH": open a visible window of that size and keep it for the sleep time
//   FAKE_GAME_CHILD   "1": start a child process that outlives this one for FAKE_CHILD_MS
//   FAKE_GAME_BIG_KB  print this many KiB of filler lines first
//   FAKE_GAME_DUMP_ENV  comma separated variable names to print as NAME=value

fn game() {
    let lines = env_u64("FAKE_GAME_LINES", 3);
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    let _ = writeln!(
        out,
        "game cwd={}",
        env::current_dir().unwrap_or_default().display()
    );
    if let Some(names) = setting("FAKE_GAME_DUMP_ENV") {
        for name in names.split(',').filter(|n| !n.is_empty()) {
            let _ = writeln!(
                out,
                "{name}={}",
                env::var(name).unwrap_or_else(|_| "<unset>".into())
            );
        }
    }
    let kb = env_u64("FAKE_GAME_BIG_KB", 0);
    for i in 0..kb {
        let _ = writeln!(out, "filler {i:06} {}", "x".repeat(1000));
    }
    for i in 1..=lines {
        let _ = writeln!(out, "out {i}");
        let _ = out.flush();
        let _ = writeln!(err, "err {i}");
        let _ = err.flush();
    }
    if setting("FAKE_GAME_CHILD").as_deref() == Some("1") {
        let _ = std::process::Command::new(env::current_exe().unwrap_or_default())
            .arg("--fake-sleeper")
            .stdin(std::process::Stdio::null())
            .spawn();
    }
    let sleep = env_u64("FAKE_GAME_SLEEP_MS", 0);
    match setting("FAKE_GAME_WINDOW") {
        Some(size) => window(&size, sleep),
        None => thread::sleep(Duration::from_millis(sleep)),
    }
    let _ = out.flush();
    std::process::exit(env_u64("FAKE_GAME_EXIT", 0) as i32);
}

#[cfg(windows)]
fn window(size: &str, sleep_ms: u64) {
    use std::ffi::c_void;
    use std::ptr::null_mut;
    #[link(name = "user32")]
    unsafe extern "system" {
        fn CreateWindowExA(
            ex: u32,
            class: *const u8,
            name: *const u8,
            style: u32,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            parent: *mut c_void,
            menu: *mut c_void,
            instance: *mut c_void,
            param: *mut c_void,
        ) -> *mut c_void;
    }
    let (w, h) = size.split_once('x').unwrap_or(("640", "480"));
    let (w, h) = (w.parse().unwrap_or(640), h.parse().unwrap_or(480));
    const WS_POPUP: u32 = 0x8000_0000;
    const WS_VISIBLE: u32 = 0x1000_0000;
    // SAFETY: creates a window of the predefined STATIC class.
    let hwnd = unsafe {
        CreateWindowExA(
            0,
            c"STATIC".as_ptr().cast(),
            c"fake game".as_ptr().cast(),
            WS_POPUP | WS_VISIBLE,
            20,
            20,
            w,
            h,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
        )
    };
    if hwnd.is_null() {
        println!("NOWINDOW");
    } else {
        println!("WINDOW {w}x{h}");
    }
    let _ = std::io::stdout().flush();
    thread::sleep(Duration::from_millis(sleep_ms));
}

#[cfg(not(windows))]
fn window(_size: &str, sleep_ms: u64) {
    thread::sleep(Duration::from_millis(sleep_ms));
}
