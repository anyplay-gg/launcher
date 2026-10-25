# Port specification: the launcher's logic

Source of truth for the core port of the macOS launcher to Windows. It specifies what the Rust crates `anyps5-core` (no UI) and `anyps5-launcher` (egui app) must do, derived from the code, not from memory.

- Terms are defined in [CONTEXT.md](../CONTEXT.md). This file uses them exactly.
- Design and look are not covered here; see `docs/DESIGN.md`.
- Status: specification, written 2026-10-22. Nothing here has been run against a real game: the development machine must not start AnyPS5 binaries. Everything that depends on observing the engine on Windows is listed in section 11 as an assumption to verify.

## 0. Sources

| Name | What | Revision |
|---|---|---|
| ARM | `utkuhalis/AnyPS5-ARM`, `tools/macos/` (launcher, `build_launcher_app.sh`, `install.sh`, `game_patches.py`), `tools/unwrap_self.py`, `docs/user/GUIDE.md` | `cb20fd8` |
| UP | `boykopovar/AnyPS5` `origin/main` (`docs/user/USAGE.md`, `INPUT_MAPPING.md`, `docs/dev/ENVIRONMENT.md`, `core/relinker`, `core/libs/prx/libc/src/specifics/windows/CrashReport.cpp`, `core/libs/prx/libkernel/AppMetadata`), release `v0.1.3` | `a6dcea9279` |
| PR3786 | Tkinter launcher PR on UP (`tools/launcher/anyps5_launcher.py`), the only existing Windows conversion code | open |
| PR7499 | UP, per-title input profiles `anyps5-input.<TITLEID>.ini` | open, not in `origin/main` |

Swift names are given as `File.swift:Type.member` so each rule can be checked against the source.

## 1. Source map

| Swift (ARM) | Rust | Notes |
|---|---|---|
| `GameLibrary.swift` (`Game`, `GameLibrary`, `Elf`) | `anyps5-core::library` | `scan`, `inspect`, `folder_size`; adds param.sfo fallback |
| `SelfUnwrap.swift` | `anyps5-core::unwrap` | byte-exact with `tools/unwrap_self.py`; vectors in appendix A |
| `Installer.swift` (`Toolkit`, `InstallRecord`, `Installer`, `OutputCollector`, `RelinkProgress`) | `toolkit`, `unlock`, `process` | renamed: Installer -> Unlocker, InstallRecord -> UnlockRecord |
| `GameSettings.swift`, `Controls.swift` (`InputTuning`, `PadAction`, `Controls`) | `settings`, `controls` | JSON field names kept, so settings files are exchangeable with the macOS launcher |
| `GamePatches.swift`, `game_patches.py`, `patches/*.json` | `patches` | rewritten for PE: section 6.9 |
| `Trophies.swift` | `trophies` | unchanged logic |
| `Diagnosis.swift` (`Diagnosis`, `DiagnosticReport`, `SharedSettings`) | `diagnosis`, `report`, `settings::shared` | Windows log formats: section 3.5 |
| `anyps5-tool/main.swift` | `crates/anyps5-tool` (optional bin) | `unwrap`, `scan`, `unlock`; used by CI and for manual checks |
| `AppModel.swift` | `anyps5-launcher::model` | state machine: section 8 |
| `AppLocation.swift` (App Translocation) | none | macOS only |
| `install.sh`, `build_launcher_app.sh` | NSIS installer, portable zip (CI) | the toolkit is downloaded, not bundled: section 6.3 |
| `Input.swift`, `Sounds.swift`, `Media.swift`, views | `anyps5-launcher` (gilrs, rodio, egui) | not part of this file |
| `Rosetta` enum in `AppModel.swift` | `hostcpu` (feature check) | section 6.12 |

The scaffold in the repository already names the modules `diagnosis, launch, library, patches, settings, toolkit, trophies, unlock, unwrap`. This file adds `controls, report, process, paths, fsx, error, hostcpu, win` (section 5).

## 2. What the Swift code reads and writes

Every file format the launcher touches. "W" marks what changes on Windows.

| File | Direction | Format and fields used | W |
|---|---|---|---|
| `<dump>/sce_sys/param.json` | read | JSON object. `titleId` (string, default: folder name), `contentVersion` (string, default `"1.0"`), `defaultLanguage` (string, top level), `localizedParameters` (object; `defaultLanguage` string inside it, then one object per language tag with `titleName`). | add param.sfo |
| `<dump>/sce_sys/param.sfo` | read | not read by ARM. Engine fallback; see 6.1 | new |
| `<dump>/sce_sys/icon0.png` | read | shown as tile icon; embedded into the unlocked copy's icon (macOS: `AppIcon.icns` via `sips`/`iconutil`) | W |
| `<dump>/sce_sys/pic0.png` | read | `Game.background`, drifting artwork behind the home screen | |
| `<dump>/sce_sys/pic1.png` | not read | ARM never uses it | |
| `<dump>/sce_sys/snd0.at9` | existence only | `Game.music`; ARM stores the URL and never plays it (AT9 is not decodable by the system player) | |
| `<dump>/eboot.bin`, modules | read | 4-byte magic checks; SELF unwrap; copy of ELF | |
| `<dump>/**.prx, .sprx` | read | `modules(in:)`, section 6.5 | |
| `<dump>/sce_sys/trophy2/trophyNN.ucp` | read | big-endian archive, section 6.8 | |
| `<store>/Games/<name>.app/Contents/Resources/anyps5-install.json` | write/read | `{source, titleID, toolkit, date}` sorted keys, ISO 8601 date | W: path |
| `<unlocked copy>/_trophy/<user>_<label>.txt`, `.stats.txt` | read | written by the engine, section 6.8 | |
| toolkit `version.txt`, `patches/<TITLEID>.json` | read | one line; patch JSON | W: section 6.3, 6.9 |
| `Preferences` in `UserDefaults` key `AnyPS5.preferences.v1` | read/write | JSON of `Preferences`, section 7 | W: file |
| `~/Library/Application Support/AnyPS5/Controls/<game.id>.ini` | write | `anyps5-input.ini` text, section 6.7 | W: path |
| `~/Library/Logs/AnyPS5/<TITLEID>.log` | append (by the game's shell script), read | text log; start marker | W: written by the launcher |
| `~/Library/Logs/AnyPS5/Reports/<TITLEID>-yyyyMMdd-HHmmss.txt` | write | report text | W: path |
| `~/Library/Logs/DiagnosticReports/eboot*.ips` | read | macOS crash report: JSON header line, JSON body | W: none exists |
| `~/Library/Caches/org.anyps5.<titleid>/shader_cache` | delete | engine's shader cache | W: path |
| `~/Pictures/AnyPS5` | create/open | screenshots | W: known folder |
| exported settings `.json` | read/write | `SharedSettings`, section 6.6 | |

## 3. How the same pipeline works on Windows (upstream facts)

### 3.1 Toolkit as released

Release `v0.1.3` (2026-10-20) of `boykopovar/AnyPS5` has these assets (checked with the GitHub API):

- `relinker-v0.1.3.exe` (5.4 MB): the relinker for Windows. `relinker-v0.1.3` is the Linux one.
- `prx-windows-v0.1.3.zip` (96.6 MB, 357 entries): the folder `libs/` with 352 `.prx` files and five DLLs: `libgcc_s_seh-1.dll`, `libstdc++-6.dll`, `libwinpthread-1.dll`, `mimalloc.dll`, `mimalloc-redirect.dll`. `.tar.gz` twin.
- `USAGE.md`, `INPUT_MAPPING.md`, `release-notes.md`, `web-v0.1.3.zip` (online converter, not used).

Tags look like `v0.1.3`, one every one to three weeks. A self-built toolkit has `build/core/relinker/relinker.exe` and `build/core/libs/libs/*.prx` (+ the MinGW DLLs from the compiler's `mingw64/bin`); PR3786 finds them by `rglob`. The three MinGW DLLs are required beside the libraries: without them `LoadLibrary` fails with error 126 (UP USAGE.md); the release zip already contains them.

### 3.2 Relinker for Windows (UP USAGE.md "Relinker usage")

```
relinker --windows [--module-dir <dir>]... [--to-intel] [--patches <toml> | --no-patches] <input.elf> <output.exe>
```

- `--windows` writes a PE. `.exe` in the name alone does not select it.
- Input is a clean ELF. Its modules must be ELF too: SELF/fSELF modules are skipped with a warning, hence the unwrap step. The module parent defaults to the input's folder; `sce_module/`, `sce_modules/` or `prx/` must exist there unless `--module-dir` is given; both `sce_module` and `sce_modules` present is an error. `--module-dir .` is allowed and means the parent itself.
- Without arguments it prints usage to stderr, exit 1. Exit codes: 0 ok, 1 bad arguments, 2 conversion failed (message on stderr).
- Output next to `<output>`: converted modules as `app0/sce_module/<name>.prx.guest.prx` (relative path of each module kept). Console subsystem by default; `--windows-gui` selects the GUI subsystem. Default `--rpath` is `$ORIGIN/libs` (libraries are searched in `libs/` beside the executable).
- Progress goes to stderr as a rewritten line with `\r`: `"%-22s | <file, padded> | %3u%%"` plus ` (<done>/<total>)` for module conversion. Stage labels: `Executable`, `PRX conversion`, `Syscall scan`, `Instruction scan`, `Instruction conversion`, `Import analysis`, `Output image`, `Write output`. Messages: `Converting <name>`, `Import analysis`. Same as the macOS build, so `RelinkProgress` ports unchanged (6.6).
- The icon: `WindowsPePatcher` takes `input.Icon`; it comes from the input's `sce_sys/icon0.png` (`Conversion.cpp:132`). To get the title's icon on the executable, stage `sce_sys/icon0.png` beside the staged `eboot.bin` (assumption A5).
- `--to-intel` converts AMD-only instructions for Intel hosts. On an AMD host it is not needed. A title that generates AMD-only instructions at run time needs an AMD host.
- Patches: `--patches <file>` applies byte patches from a TOML subset before conversion, in the ELF's virtual address space (6.9). There is no `--set`.

### 3.3 Layout of a runnable title on Windows

UP USAGE.md "Runtime layout", PR3786 and the engine sources agree:

```text
<run dir>/                  = working directory of the process
    app.exe                 relinked executable (any name)
    libs/*.prx, *.dll       system libraries and the MinGW/mimalloc DLLs
    app0/                   the title's files as the console mounts /app0
        eboot.bin           the ORIGINAL executable must be present: titles probe /app0/eboot.bin
        sce_sys/param.json  (or param.sfo) must be present for titleId and name
        sce_module/*.guest.prx   converted modules
    data/                   created at start when app0/sce_sys/param.json|sfo exists
    download0/              created when param downloadDataSize != 0
    temp0/                  /temp0 mount, created by the engine (libSceAppContent)
    _sd/                    save data (libSceSaveData: SAVE_DIR "_sd"; "_sd_mem")
    _trophy/<user>_<label>.txt  and .stats.txt
    anyps5-screenshots/, anyps5-fonts/, anyps5.log   defaults; overridden by env
```

`app0/` must not hold symbolic links (a directory listing reports the link's own type); hard links and copies are fine. Original PS5 modules must never be put in `libs/` (error 193). The engine reads `current_path()` for `data/`, `download0/`: the working directory must be the run dir. A Windows executable started without a stdout/stderr handle writes `anyps5.log` beside itself.

The launcher makes the run dir equal to the unlocked copy folder (section 4.2).

### 3.4 Environment variables a launcher can use

From `docs/dev/ENVIRONMENT.md` and the sources on `origin/main`. "Used by ARM" = set by `GameSettings.environment`.

| Variable | Meaning (UP) | ARM sets | Windows launcher |
|---|---|---|---|
| `ANYPS5_INPUT_CONFIG` | path of the input ini, any characters | yes | yes |
| `ANYPS5_PRESENT_MODE` | `fifo` (default), `fifo_latest_ready`, `mailbox`, `immediate`; anything else or unsupported mode fails the start | `fifo` / `immediate` | same two values |
| `ANYPS5_LANGUAGE` | console language number, default 1 | yes | yes |
| `ANYPS5_OVERLAY` | `1` shows the performance overlay; `0`/unset hides; other values stop the game | `1` when on | same |
| `ANYPS5_OVERLAY_POS` | `tl` `tr` `bl` `br` | yes | same |
| `ANYPS5_SCREENSHOTS` | directory for F12 PNGs, default `anyps5-screenshots` beside the executable | yes | yes |
| `ANYPS5_SYSTEM_FONTS` | directory with console system fonts, default `anyps5-fonts` beside the executable | yes | yes |
| `ANYPS5_SHADER_CACHE_DIR` | shader cache directory, default beside the executable | via launch script | set by the launcher |
| `ANYPS5_NO_SHADER_CACHE` | non-empty and not `0` disables the cache | when off | same |
| `ANYPS5_GPU` | restrict device selection to names containing this text (case-insensitive) | no | optional setting `gpu` (new; useful with an iGPU and a dGPU) |
| `APS5_FORCE_SDR` | no HDR advertised | no | optional, not exposed in v1 |
| `APS5_CRASH_STACK_BYTES`, `APS5_CRASH_INDIRECT_MEMORY`, `APS5_HANG_DUMP_SECS`, `APS5_LOG_TIMESTAMPS` | crash report depth, hang watchdog, timestamps | no | "Extended crash report" toggle in Advanced (v2); sets `APS5_CRASH_INDIRECT_MEMORY=1`, `APS5_CRASH_STACK_BYTES=4096` |
| `ANYPS5_FULLSCREEN`, `ANYPS5_WINDOW_SIZE`, `APS5_RESOLUTION` | **do not exist upstream.** They are ARM fork additions | yes | not passed. See D9, D10 |
| `VK_DRIVER_FILES`, `MVK_CONFIG_SYNCHRONOUS_QUEUE_SUBMITS` | MoltenVK | yes | none |

The launcher always starts the game with the full environment of its own process plus these. A variable the player set globally is overridden only by the launcher's own settings.

Window and input keys in the engine (not environment): F11 = `ToggleFullscreen`, F10 = overlay, F12 = screenshot, F7 / middle mouse = `ToggleMouse`, X = `Shake`.

### 3.5 What the engine prints (for diagnosis)

All on stderr (merged with stdout by the launcher, section 6.10). Exact strings from the sources:

| Situation | Text | Source |
|---|---|---|
| module did not load | `Failed to load module: <name>` then `GetLastError: <n> - <system message>`, then possibly the hint line `hint: the module or one of its dependent DLLs was not found; a self-built libs/ also needs libgcc_s_seh-1.dll, ...` | `WindowsEntryStubBuilder.cpp:243-246`; with `--windows-diagnostics` the stub also prints `Loading PRX: <name> -> OK` / ` -> FAILED` |
| ELF import not found | `FAIL: unresolved ELF import <name>...` | `WindowsEntryStubBuilder.cpp` error table |
| startup reached the game | `Transferring control to ELF entry point` (diagnostics builds only) | same |
| function not implemented | `NotImplemented_nid_no_patch(name)` throws `std::runtime_error("<name> not implemented")`; if uncaught: `terminate called after throwing an instance of 'std::runtime_error'` newline `  what():  <name> not implemented` | `General.cpp:828`, `exception/Runtime.cpp:74-78` |
| not implemented, other forms | `<operation> <path> in app0.zar not implemented` style messages through `NotImplemented_nid_no_patch`; `... writing to <file> is not implemented` | `AppArchive.cpp:142`, `FileAndHeap.cpp:357` |
| unsupported host CPU | exception text `The host CPU (<model>) lacks <list>. The PS5's Zen 2 CPU has these extensions and titles run their instructions natively, so this CPU cannot run them.` | `HostCpu.cpp:179` |
| GPU failure | `[gpu] driver failure: <what()>`; Vulkan errors throw `AGC graphics: <operation>: Vulkan result <n>` (`VK_ERROR_OUT_OF_HOST_MEMORY` = -1, `VK_ERROR_OUT_OF_DEVICE_MEMORY` = -2, `VK_ERROR_DEVICE_LOST` = -4) | `Lifecycle.cpp:47`, `Context.hpp:71-74` |
| unhandled exception | blank line, `FATAL: unhandled exception 0x%08lx on thread %lu '<thread name>'`, `  rip <addr> <module>+0x<off>`, for access violations `  read of 0x...` / `  write of` / `  execute of`, optional `  guest abort() (int 0x45)` / `  guest assert/break (int 0x41)`, register dump, stack candidates | `CrashReport.cpp:371-390` |
| abort | `FATAL: abort() (SIGABRT) on thread <n>` + 48-frame backtrace | `CrashReport.cpp:638` |
| CRT invalid parameter | `FATAL: UCRT invalid parameter on thread <n>` | same |
| undeliverable C++ exception | `FATAL: C++ exception <type> raised on thread <n> cannot be delivered: <addr> has no unwind data` | `CrashReport.cpp:325` |
| hang watchdog | `FATAL: timed dump after <n> s (APS5_HANG_DUMP_SECS); aborting` | `CrashReport.cpp:613` |
| trophy unlocked | `[trophy] <TITLEID> unlocked trophy <n> for user <id>` | USAGE.md |
| screenshot saved | path printed to stderr | USAGE.md |
| driver crash | log ends with `FATAL: unhandled exception 0xc0000005` and `nvgpucomp64.dll` in `rip` (NVIDIA driver 610.47) | USAGE.md |

Process exit: a Windows crash ends the process with the exception code as exit status (`0xC0000005` access violation, `0xC000001D` illegal instruction, `0xC0000409` fast fail after abort); `abort()` without the handler exits with 3. There is no Windows Error Reporting file by default. The `.ips` crash report of ARM has no equivalent: the crash block is in the log itself.

### 3.6 Comparison with PR3786

PR3786 is the only prior art for the Windows side. What is taken, what is not:

| PR3786 | This port |
|---|---|
| per game folder `<cache>/<title id>-<sha256(path)[:8]>`, files `app.exe`, `libs/`, `app0/`, `state.json`, `convert.log`, `src/` | same idea. Name from the ARM formula (FNV-1a), section 6.4. State is the unlock record. |
| `os.link` with `shutil.copy2` fallback, note when copies were made | same, plus ReFS block cloning first (6.5) |
| `app0` = hard-link tree of the game folder **skipping** `sce_module`, `sce_modules`, `prx` at top level; converted modules come from the relinker's own `app0/` | **differs.** ARM keeps the dump's original modules in `app0` and lays the converted ones over them (the engine needs `/app0/eboot.bin` and tolerates the originals). The port follows ARM. See A6. |
| rejects SELF | the port unwraps decrypted SELF (ARM behaviour) and rejects encrypted ones |
| `--module-dir` for every folder (depth <= 3) with ELF/SELF `.prx` outside the standard dirs | ARM's `modules(in:)`/`moduleDirectories` rule (6.5), which also handles `.sprx` |
| copies MinGW DLLs from the compiler dir into `libs/` and the run dir | the release zip already has them in `libs/`; the launcher validates they are present and does not copy them to the run dir (`$ORIGIN/libs` finds them: the PR puts them in both because the loader searches the executable directory for DLL dependencies of the executable itself, assumption A4) |
| fingerprint of sizes and mtimes of inputs + relinker + extra args | the toolkit version in the unlock record (ARM). No input fingerprint; the player unlocks again. |
| `download0/` not created | same |
| env from `env.txt` | `GameSettings` (6.6) |
| output merged into one pipe, written to `last_run.log` | same, log per title ID with start markers (6.10) |

## 4. macOS to Windows differences

Every place where behaviour must differ. Keep this table in sync; the code comments cite the ids.

| Id | macOS (ARM) | Windows (this port) |
|---|---|---|
| D1 | Executable is a Mach-O, built with `relinker --macos --to-rosetta`; Rosetta translates x86-64 | PE built with `relinker --windows`; native x86-64. `--to-intel` when the host CPU vendor is `GenuineIntel` (setting `cpuConversion`: `auto` default, `none`, `intel`). No Rosetta check; instead a host CPU feature check (6.12) |
| D2 | Graphics through MoltenVK: `libvulkan.1.dylib`, `libMoltenVK.dylib`, `MoltenVK_icd.json`, `VK_DRIVER_FILES`, `MVK_CONFIG_SYNCHRONOUS_QUEUE_SUBMITS` | the system's Vulkan loader and driver. Nothing is copied or set. `ANYPS5_GPU` optional |
| D3 | `.app` bundle: `Contents/{MacOS/{eboot,launch,app0->link,libs/}, Resources/{game/app0,vulkan,resolution.txt,AppIcon.icns,anyps5-install.json}, Info.plist}` | plain folder, section 4.2 |
| D4 | game files are APFS clones (`copyfile` `COPYFILE_CLONE`), no extra space | NTFS hard links (fallback: copy). ReFS / Dev Drive block cloning is tried first. The overlay of converted modules must replace, never write through a link (6.5) |
| D5 | start by `NSWorkspace.openApplication` of the bundle; the `launch` shell script sets env, `cd`s to `Resources/game`, `exec`s `eboot` with output appended to the log | `CreateProcess` of `eboot.exe` with the environment, working directory = unlocked copy, stdout+stderr merged into one pipe read by a launcher thread that appends to the log (6.10). Child processes (`sceSystemServiceLoadExec`) stay in a job object |
| D6 | first window: `CGWindowListCopyWindowInfo`, owner pid, layer 0, width > 200 and height > 150 | `EnumWindows` over the job's processes: visible, not owned, not `WS_EX_TOOLWINDOW`, client or window rect > 200 x 150 (6.11) |
| D7 | exit: `NSWorkspace.didTerminateApplicationNotification` and `DispatchSource.makeProcessSource(.exit)` | job object: process exit of the main process, `ActiveProcesses == 0` of the job. For a title that hands over with LoadExec the end is the end of the job |
| D8 | bring game to front: `NSApp.yieldActivation`, `app.activate()` | `SetForegroundWindow` after attaching input is not reliable; the launcher calls `AllowSetForegroundWindow(pid)` right after spawn and `SetForegroundWindow(hwnd)` when the window shows; the game window is also topmost-flashed if that fails (`FlashWindowEx`) |
| D9 | `ANYPS5_FULLSCREEN`, `ANYPS5_WINDOW_SIZE` (percent of the display, 30 to 100) | not in UP. Window placement is done by the launcher after the first window shows: `SetWindowPos` to `windowSize` percent of the work area of the monitor under the window, centred, once. Fullscreen: if `fullscreen` is on, one F11 key press is sent to the window (`SendInput` after `SetForegroundWindow`, press+release, scancode 0x57) 1 s after the window shows. Both are assumptions A7, A8 and may be replaced by an upstream environment variable |
| D10 | resolution patch writes immediates in the Mach-O before each start (`launch` script `dd`) | patches are relinker TOML (ELF virtual addresses) applied at unlock time (6.9). Changing the resolution of a title with a resolution patch asks the player to unlock again. Titles without a Windows patch file show no resolution choice |
| D11 | crash information: `~/Library/Logs/DiagnosticReports/eboot*.ips` (JSON), matched to the game by bundle identifier | the log's `FATAL:` block and the process exit status (3.5). `crashReport(since:bundleIdentifier:)` is replaced by `ExitInfo` |
| D12 | per-user data: `UserDefaults`, `~/Library/{Application Support,Logs,Caches}/AnyPS5`, `~/Pictures/AnyPS5` | `%APPDATA%\AnyPS5\preferences.json`, `%APPDATA%\AnyPS5\Controls\`, `%LOCALAPPDATA%\AnyPS5\{Logs,Cache,Toolkit}`, `Pictures\AnyPS5` (known folder). Section 9 |
| D13 | Gatekeeper: quarantine flag removal (`xattr -dr com.apple.quarantine`), App Translocation, move to /Applications | none. Files the launcher writes carry no Mark of the Web. Not needed: `AppLocation`, translocation prompt |
| D14 | icon: `sips` + `iconutil` to `AppIcon.icns` | the relinker embeds `icon0.png` into the PE when it finds `sce_sys/icon0.png` beside the staged executable (A5). The launcher also converts `icon0.png` to `.ico` in-process only if A5 fails (v2) |
| D15 | keyboard capture by Mac virtual key code (`Controls.keyName(forKeyCode:)`) | Windows scan code (layout independent, left/right distinct) mapped to the SDL scancode name (appendix B) |
| D16 | `Controls.unbound = "KEY:F24"` ("no Mac keyboard has F24") | keep `KEY:F24`, never offer F24 for capture (the table has no F24), see 6.7 |
| D17 | `GameLibrary.size` = allocated size | logical size (`metadata().len()`); hard links are counted once per tree, no dedupe across trees |
| D18 | file watching: `DispatchSource` on the root (O_EVTONLY) | `notify` (`ReadDirectoryChangesW`), non-recursive on the root (6.2) |
| D19 | `Rosetta` check and installer | removed. Replaced by CPU feature check and a link to the toolkit settings |
| D20 | toolkit inside the app bundle (`Contents/Resources/toolkit`), `ANYPS5_TOOLKIT` override | toolkit downloaded from GitHub releases or picked as a folder; `ANYPS5_TOOLKIT` still overrides (6.3) |
| D21 | `Process.terminate()` (SIGTERM) on cancel | `TerminateProcess` on the relinker's job object |
| D22 | atomic swap `removeItem(bundle)` + `moveItem(partial, bundle)` | same sequence with retries on sharing violations (Defender / indexer): 8 tries, 150 ms apart |
| D23 | paths are case-sensitive strings | `GameId` is the lower-cased, `/`-separated absolute path (6.4) |
| D24 | `reveal in Finder` (`activateFileViewerSelecting`) | `explorer.exe /select,<path>` (spawned without a shell) |
| D25 | per-title crash report matching by bundle id | not needed; the log file is per title ID and has start markers |

### 4.1 Things that stay identical

Scan rules, `inspect`, SELF unwrap, module discovery, relink progress parsing, queue semantics, cancel semantics, trophies, input ini text, tuning clamps and formats, settings JSON, shared settings JSON, diagnosis priority order and the shapes of its messages, report layout, and the game state machine.

### 4.2 Unlocked copy layout

```text
<games folder>\.AnyPS5\
    Games\
        <TITLEID>-<hex>\               the unlocked copy; also the game's working directory
            eboot.exe                  relinked executable (patched, section 6.9)
            libs\                      hard links (or copies) of every file in the toolkit's libs\
            app0\                      the dump, linked in; converted modules laid over it
            anyps5-unlock.json         the unlock record
            _sd\ _trophy\ data\ download0\ temp0\ anyps5-screenshots\ ...   written by the engine
        <TITLEID>-<hex>.partial\       while being made
    Work\<uuid>\                       source\ (unwrapped ELF files), out\ (relinker output); removed after
```

The folder name `<TITLEID>-<hex>` is `Installer.bundle(for:)` without `.app` (6.4). The unlock record is `anyps5-unlock.json`; macOS wrote `anyps5-install.json` (the format is the same, section 6.4).

## 5. Architecture

### 5.1 Workspace

```text
Cargo.toml                       workspace, resolver 3, edition 2024
crates/anyps5-core/              UI-free, no egui, no gilrs, no rodio. Windows API only behind cfg(windows) in `win`
crates/anyps5-launcher/          egui app (eframe + wgpu), gilrs, rodio, model/state machine, views
crates/anyps5-tool/              optional CLI: unwrap | scan | unlock (CI + manual checks), depends on core only
crates/fake-relinker/            test helper binary, never shipped (section 10.5)
```

`rust-version` must be at least 1.88 (`std::io::pipe`, let chains). Core dependencies: `serde`, `serde_json`, `thiserror`, `tracing`, `notify` (8), `regex`, `sha2`, `time` (RFC 3339 formatting, no local time zone needed), `ureq` (blocking HTTP, rustls) and `zip` for toolkit download, `windows` (feature-gated modules: `Win32_Foundation`, `Win32_Storage_FileSystem`, `Win32_System_JobObjects`, `Win32_System_Threading`, `Win32_UI_WindowsAndMessaging`, `Win32_UI_Input_KeyboardAndMouse`, `Win32_System_SystemInformation`, `Win32_Graphics_Dxgi`, `Win32_UI_Shell`), `tempfile` (dev). No async runtime.

### 5.2 Modules of `anyps5-core`

| Module | Interface (what a caller must know) | Hides |
|---|---|---|
| `library` | `scan`, `inspect`, `folder_size`, `LibraryWatcher`, `Game` | param.json/sfo parsing, recursion, skip list |
| `unwrap` | `is_container`, `unwrap_bytes`, `unwrap_file` | the whole SELF algorithm (this is the deepest module; a tiny interface over 400 lines) |
| `toolkit` | `Toolkit::{open, locate}`, `ToolkitStore`, `check_latest`, `download`, `Toolkit::version` | release lookup, zip extraction, fingerprint |
| `unlock` | `UnlockQueue::{new, enqueue, cancel, position}`, `UnlockEvent`, `UnlockRecord`, `Store` | staging, relinker process, linking, swap, carry-over |
| `process` | `ToolProcess` (internal), `LineSplitter` (pure) | pipes, `\r` rewriting, job objects, cancel |
| `launch` | `Launcher::start`, `RunHandle`, `LaunchEvent` | spawn, env, job, log pump, window detection, exit detection |
| `diagnosis` | `Diagnosis::{read, last_session}`, `ExitInfo` | all regexes and NTSTATUS names |
| `report` | `diagnostic_text` | system information |
| `settings` | `Preferences`, `GameSettings`, `SharedSettings`, `AppPaths`, `ConsoleLanguage` | file locations, atomic writes, defaults |
| `controls` | `PadAction`, `InputTuning`, `Controls`, scancode table | ini text, clamps, key names |
| `trophies` | `Trophies::{load, earned}`, `TrophyWatcher`, `TrophySet` | `.ucp` archive, JSON texts, record files |
| `patches` | `PatchSet::{load, relinker_toml, resolution_choices}` | TOML generation |
| `hostcpu` | `HostCpu::detect`, `missing_features` | CPUID |
| `paths`, `fsx`, `error`, `win` | internal | `GameId`, hard links, `\\?\`, error types, Win32 wrappers |

Deletion test: removing `unwrap`, `diagnosis` or `trophies` spreads their rules over the app; removing `process` spreads pipe and cancel handling over `unlock`, `launch` and `toolkit`. All earn their place. `report` is thin on purpose (text assembly); it exists because ARM keeps it with `Diagnosis` and because it is the only code that knows system information.

### 5.3 Core types

```rust
// library
#[derive(Clone, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub struct GameId(String);                 // normalised absolute folder path, 6.4
pub enum ExecutableKind { Elf, Container, Unreadable }
#[derive(Clone, Debug)]
pub struct Game {
    pub id: GameId, pub folder: PathBuf, pub title_id: String, pub name: String, pub version: String,
    pub icon: Option<PathBuf>, pub background: Option<PathBuf>, pub music: Option<PathBuf>,
    pub executable: ExecutableKind,
}
impl Game { pub fn bundle_identifier(&self) -> String }   // "org.anyps5." + lower-case alphanumerics of title_id
// equality and hash: by `id` only (Swift compares folder)

// toolkit
pub struct Toolkit { pub root: PathBuf, pub version: String }
impl Toolkit { pub fn relinker(&self)->PathBuf; pub fn libs(&self)->PathBuf; pub fn patches(&self)->PathBuf }

// unlock
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step { Unwrap, Relink, Copy, Assemble, Finish }          // ARM: unwrap relink copy bundle finish
pub struct UnlockRecord { pub source: String, pub title_id: String, pub toolkit: String, pub date: String /* RFC 3339 UTC */ }

// diagnosis
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Diagnosis {
    MissingModule(String), Unsupported(String), GpuLost, OutOfMemory,
    EngineError(String), Crashed(String),
    HostCpu(String),                       // new on Windows: the list of missing extensions
}
pub struct ExitInfo { pub code: Option<u32> /* None when killed by the launcher */, pub had_window: bool }

// settings
pub struct GameSettings { pub resolution: Option<String>, pub fullscreen: bool, pub window_size: u32,
    pub vsync: bool, pub overlay: Overlay, pub language: Option<i32>, pub controls: Option<BTreeMap<String, Vec<String>>>,
    pub tuning: Option<InputTuning>, pub shader_cache: Option<bool>, pub gpu: Option<String> }
```

### 5.4 Events and threading

Rules:

1. The UI thread never touches the file system beyond reading an already-open handle and never waits on a thread. Every operation that takes longer than about 1 ms runs on a worker and reports by event.
2. One channel into the UI: `std::sync::mpsc::Receiver<CoreEvent>`. Core receives a `Notifier = Arc<dyn Fn() + Send + Sync>` (the app passes `egui::Context::request_repaint`) which it calls after each send. Senders never block (unbounded channel).
3. The app drains the channel at the start of each frame (`while let Ok(e) = rx.try_recv()`) in `AppModel::pump`, mutates state, and draws. All state lives in `AppModel`; core types are plain data.
4. Worker threads (named, so crash reports are readable):

| Thread | Lifetime | Does |
|---|---|---|
| `scan` | per rescan | `library::scan`, reads unlock records, sends `LibraryScanned` |
| `measure` | per game, low priority | `folder_size`, sends `SizeMeasured` |
| `watch` | while a games folder is set | `notify` watcher thread (owned by the crate) + debounce, sends `LibraryChanged` |
| `unlock` | process lifetime, idle when queue is empty | runs one `Unlocker` at a time; owns the relinker child; sends `Unlock(..)` events |
| `unlock-io` | per unlock | log/progress reader of the relinker pipe |
| `run-pump` | per started game | reads the merged pipe, appends to the log, keeps the 400 KiB tail |
| `run-watch` | per started game | waits for the first window (400 ms poll, 240 s limit) and for job end; sends `Launch(..)` |
| `trophy-watch` | while a game runs | every 1 s `Trophies::earned(copy)`; on change `Trophies::load`, sends `TrophyUnlocked` |
| `diagnose` | per game end | reads log tail, builds `Diagnosis`, sends `Diagnosed` |
| `toolkit` | per operation | release check, download, extraction, fingerprint; sends `Toolkit(..)` |
| `report` | per request | system info + file write |

5. Cancellation is cooperative: a `CancelToken(Arc<AtomicBool>)` checked between files and every 64 KiB of a copy, plus `ToolProcess::kill` (job termination) for a running child. A cancelled operation ends with `Err(Cancelled)`, never with a partial result; the app ignores the cancelled event except to clear state (as `AppModel.startNext` does).
6. Shared mutable state in core is limited to: the unlock queue (`Mutex<QueueState>` + `Condvar`), the current relinker process handle (`Mutex<Option<ToolProcess>>`), the cancel flags. No other locks.
7. `Drop` of `LibraryWatcher`, `TrophyWatcher`, `RunHandle` stops their threads (flag + join with 1 s timeout).

```rust
pub enum CoreEvent {
    LibraryScanned { root: PathBuf, games: Vec<Game>, records: HashMap<GameId, UnlockRecord> },
    LibraryChanged,                                          // watcher fired and the debounce allows a rescan
    SizeMeasured { game: GameId, bytes: u64 },
    Unlock(UnlockEvent),
    Launch(LaunchEvent),
    Trophies { game: GameId, set: Option<TrophySet> },
    TrophyUnlocked { game: GameId, trophy: Trophy },
    Diagnosed { game: GameId, diagnosis: Option<Diagnosis>, without_window: bool },
    Toolkit(ToolkitEvent),
    ReportWritten { game: GameId, result: Result<PathBuf, String> },
}
pub enum UnlockEvent {
    Queued { game: GameId },                                 // position = queue index
    Started { game: GameId },
    Progress { game: GameId, step: Step, fraction: f64 },    // coalesced, 6.6
    Log { game: GameId, line: String },
    Finished { game: GameId, result: Result<UnlockRecord, UnlockError> },   // Err(Cancelled) possible
}
pub enum LaunchEvent {
    Spawned { game: GameId, pid: u32 },
    WindowShown { game: GameId, forced: bool },   // forced: the 240 s deadline passed with the process alive
    Exited { game: GameId, info: ExitInfo },
}
pub enum ToolkitEvent {
    Latest(Result<ReleaseInfo, ToolkitError>),
    Progress { stage: DownloadStage, fraction: f64 },
    Installed(Result<Toolkit, ToolkitError>),
}
```

### 5.5 Public functions

```rust
// library
pub fn scan(root: &Path) -> Vec<Game>;                     // sorted by name, case-insensitive
pub fn inspect(folder: &Path) -> Option<Game>;
pub fn folder_size(folder: &Path, cancel: &CancelToken) -> u64;
pub struct LibraryWatcher; impl LibraryWatcher { pub fn start(root: PathBuf, tx: Sender<CoreEvent>, notify: Notifier, busy: Arc<AtomicBool>) -> Result<Self, WatchError>; }

// unwrap
pub fn is_container(path: &Path) -> bool;
pub fn unwrap_bytes(data: &[u8]) -> Result<Vec<u8>, Refused>;
pub fn unwrap_file(input: &Path, output: &Path) -> Result<usize, Refused>;

// toolkit
impl Toolkit { pub fn locate(paths: &AppPaths, prefs: &Preferences) -> Option<Toolkit>; pub fn open(root: &Path) -> Result<Toolkit, ToolkitError>; }
pub fn check_latest(repo: &str) -> Result<ReleaseInfo, ToolkitError>;          // blocking: call from `toolkit` thread
pub fn download_and_install(info: &ReleaseInfo, paths: &AppPaths, progress: impl FnMut(DownloadStage, f64), cancel: &CancelToken) -> Result<Toolkit, ToolkitError>;
pub fn adopt_folder(folder: &Path) -> Result<Toolkit, ToolkitError>;           // user-chosen build folder

// unlock
impl UnlockQueue {
    pub fn new(tx: Sender<CoreEvent>, notify: Notifier) -> Self;
    pub fn enqueue(&self, game: Game, root: PathBuf, toolkit: Toolkit) -> bool;   // false when already queued or running
    pub fn cancel(&self, game: &GameId);          // removes a queued game or stops the running one
    pub fn is_idle(&self) -> bool;
}
pub fn unlocked_copy(game: &Game, root: &Path) -> PathBuf;
pub fn read_record(copy: &Path) -> Option<UnlockRecord>;
pub fn remove_leftovers(root: &Path);              // Work\ and *.partial, only while nothing is unlocking
pub fn remove_unlocked(game: &Game, root: &Path) -> io::Result<()>;

// launch
impl Launcher { pub fn start(req: LaunchRequest, tx: Sender<CoreEvent>, notify: Notifier) -> Result<RunHandle, LaunchError>; }
impl RunHandle { pub fn game(&self)->&GameId; pub fn pid(&self)->u32; pub fn kill(&self); pub fn is_alive(&self)->bool; pub fn activate(&self); }

// diagnosis / report
impl Diagnosis {
    pub const START_MARKER: &'static str = "[anyps5-launcher] start";
    pub fn last_session(log: &str) -> String;
    pub fn read(session: &str, exit: Option<&ExitInfo>) -> Option<Diagnosis>;
    pub fn summary_key(&self) -> (&'static str, Vec<String>);  // localisation key + arguments
}
pub fn log_file(paths: &AppPaths, title_id: &str) -> PathBuf;
pub fn read_last_session(path: &Path) -> String;               // last 4_000_000 bytes -> last_session
pub fn diagnostic_text(input: &ReportInput) -> String;

// settings / controls
impl Preferences { pub fn load(paths: &AppPaths) -> Preferences; pub fn save(&self, paths: &AppPaths) -> io::Result<()>; }
impl GameSettings { pub fn environment(&self, ctx: &EnvContext) -> BTreeMap<String,String>; pub fn write_input_file(&self, file: &Path) -> io::Result<()>; }
impl SharedSettings { pub fn new(game: &Game, s: &GameSettings) -> Self; pub fn to_json(&self) -> String; pub fn read(bytes: &[u8]) -> Result<Self, SettingsError>; }
impl Controls { pub fn sources(a: PadAction, changes: &Changes) -> Vec<String>; pub fn assign(key: &str, to: PadAction, changes: &Changes) -> Changes; pub fn file(changes: &Changes, tuning: &InputTuning) -> String; }

// trophies
impl Trophies { pub fn load(game: &Game, copy: Option<&Path>, languages: &[String]) -> Option<TrophySet>; pub fn earned(copy: &Path) -> HashSet<String>; }
impl TrophyWatcher { pub fn start(game: Game, copy: PathBuf, languages: Vec<String>, tx: Sender<CoreEvent>, notify: Notifier) -> Self; }
```

`EnvContext { launcher_language: i32, input_file: Option<PathBuf>, screenshots: Option<PathBuf>, fonts: Option<PathBuf>, shader_cache: Option<PathBuf> }`.

### 5.6 Errors

One `thiserror` enum per module, each convertible into the module above it, none containing `anyhow`. `Display` strings are the user-facing text (English; the app maps variants to localised strings where Swift did, otherwise shows `Display`).

```rust
pub struct Refused { pub reason: String }                 // unwrap; Display = reason (exact strings in 6.1 of SELF)
pub enum UnlockError {
    Cancelled,
    ToolkitMissing,
    Unwrap { file: String, source: Refused },             // Display: "<file>: <reason>"
    NotAnExecutable { file: String },                     // "<file> is neither an ELF nor a decrypted SELF container (an encrypted dump cannot be used)"
    Relinker { code: Option<i32>, last_line: String },    // "relinker.exe failed (<code>): <last line>"
    NoOutput,                                             // "relinker wrote no executable"
    Io { context: String, source: io::Error },            // "copy <relative>: <os message>" etc.
    Toolkit(ToolkitError),
}
pub enum ToolkitError { Missing(String) /* file */, MissingRuntime(Vec<String>), Network(String), BadArchive(String), Digest, Io(io::Error), NoRelease }
pub enum LaunchError { NotUnlocked, Spawn(io::Error), Job(String), Log(io::Error) }
pub enum SettingsError { NotSettingsFile /* "not an AnyPlay settings file" */, Json(serde_json::Error), Io(io::Error) }
```

### 5.7 Implementation notes (core-library)

Decisions made while implementing `paths`, `library`, `settings`, `controls`, `trophies`, `patches`:

- `events` (new module) holds `Notifier`, `CancelToken` and `CoreEvent`; each module adds its variants. `CoreEvent::LibraryScanned.records` is `HashMap<GameId, String>` (toolkit version) until `unlock` defines `UnlockRecord`.
- `Game` additionally carries `names` (title per language tag), `content_id`, `size` (`None` until `SizeMeasured`) and `unlocked_with` (toolkit version of the unlocked copy; filled by `scan`, which knows the games folder; `inspect` leaves it `None`). `library::diff(old, new)` returns added, removed and changed games.
- `paths` owns `GameId`, `AppPaths` (also the `AppPaths` of 6.6), the FNV-1a copy name (`unlocked_copy_name`, `unlocked_copy_dir`) and `bundle_identifier`. A drive root normalises to `c:/` (the trailing slash stays, `c:` would mean "current folder of drive C").
- `Trophy.unlocked` is Unix microseconds (`i64`); `unlocked_secs()` gives seconds. `TrophyWatcher` sends `Trophies` once at start (no replay of old unlocks), then `TrophyUnlocked` and a fresh `Trophies` when the set grows.
- `Preferences` also has `cpuConversion` (`auto|none|intel`, D1). Unknown enum values in a file fall back to the default instead of rejecting the file. The locale for the first run comes from `GetUserDefaultLocaleName`; the profile name from `USERNAME` (first word).
- `PatchSet::plan` returns the relinker entries and one state per patch; resolution fields count as the patch `resolution` and are all-or-nothing.

### 5.7 As built: unwrap, toolkit, fsx, process, unlock, launch, diagnosis, report, hostcpu

Where the code differs from or adds to the text above (the code is the reference for the signatures):

- **Events go through a sink, not a channel.** Workers take `EventSink<T> = Arc<dyn Fn(T) + Send + Sync>`; the app's closure wraps the event into `CoreEvent` and sends and repaints. `UnlockEvent`, `LaunchEvent` and the toolkit progress callback are therefore module types, not `CoreEvent` variants. The sink runs on the worker thread and must not call back into the queue. `CancelToken` is `events::CancelToken`, re-exported from `error`.
- `unlock`: `UnlockQueue::new(sink)`, `enqueue(UnlockRequest) -> bool`, `cancel(&GameId)`, `position`, `is_idle`; `run_unlock(&request, &UnlockControl, &dyn Fn(UnlockEvent))` is the same pipeline, synchronous (the queue's worker calls it; it sends `Started`, `Progress`, `Log` and returns the result, the worker sends `Finished`). `UnlockRequest { game, folder, title_id, root, toolkit, to_intel, patches_toml, link_mode }` carries the toolkit and the already generated relinker patch text, so `unlock` does not depend on `settings` or `patches`. A retry is `enqueue` again after a `Finished(Err)`. `Queued` is a new event. Unlocking an unlocked game replaces the copy; `carry_over` keeps `_sd`, `_trophy`, `data`, `download0`, `temp0` and the engine's other files.
- `toolkit`: the preference value is `ToolkitChoice` (`{"kind":"release","tag":"0.1.3"}` or `{"kind":"folder","relinker":...,"libs":...}`). `Toolkit` has accessors (`relinker()`, `libs()`, `patches()`, `version()`, `tag()`, `extra_runtime()`); `Toolkit::locate(paths, choice)` reads `ANYPS5_TOOLKIT`. `download_and_install(info, paths, patches_source, progress, cancel)`. `update_available(&ReleaseInfo, &Toolkit)`. The hash cache `adopted.cache.json` for adopted folders is not implemented (hashing is done at every `open`).
- `launch`: `LaunchRequest` holds the final environment map (`GameSettings::environment`), the folders to create, the log file and `WindowPrefs`; the input file is written by the caller. `Launcher::start(request, sink)` returns a `RunHandle`; keep it until `Exited`, dropping it silences the watcher (the game keeps running). The watcher itself places the window (D9) before it reports `WindowShown`; the move is posted with `SWP_ASYNCWINDOWPOS` because a game that does not pump messages would block a synchronous move. The end of the game is polled (`ActiveProcesses` every 400 ms), not waited for on a completion port. `LaunchEvent::Diagnosed` is sent after `Exited` and carries the `without_window` flag.
- `diagnosis`: `Diagnosis::summary()` is the English text, `summary_key()` the localisation key (`diagnosisMissingModule` ... `diagnosisHostCpu`). The log path is `AppPaths::log_file`.
- `hostcpu`: `CpuConversion` (`auto`/`none`/`intel`) with `HostCpu::wants_intel_conversion`.
- `unwrap`: the layout of the output follows `SelfUnwrap.swift` (page alignment first, then the `vaddr` residue), which can place a region up to one alignment further than `unwrap_self.py` does (`cursor + (vaddr - cursor) % align`); both are valid ELF layouts and the vectors of appendix A agree. A guard rejects outputs above 4 GiB (a malformed header could ask for more). The 64-bit little-endian check of the script is included.
- `crates/fake-relinker` is one binary playing the relinker and the game; tests configure a copy through a `fake.env` beside it. `tests/real_dump.rs` is an ignored end-to-end test for a real dump and the real toolkit (see its header).

## 6. Module specifications

### 6.1 `unwrap` (SelfUnwrap.swift, tools/unwrap_self.py)

**Byte-exact.** The Python script and the Swift code write the same bytes (ARM states it in the file header; both read in this work). The Rust port must too. Everything is little-endian. Constants:

```
SELF_MAGIC = 0x1D3D154F        (bytes 4F 15 3D 1D)   SELF_MAGIC_KERNEL = 0xEEF51454 (bytes 54 14 F5 EE)
RECORD_OFFSET = 0x40  RECORD_SIZE = 0x20  FIRST_REGION = 0x400  RECORD_HAS_SEGMENT = 0x800  RECORD_SEGMENT_SHIFT = 20
MAX_EMPTY_LOAD_PADDING = 64 MiB   PT_LOAD = 1  PT_NOTE = 4   ENTROPY_LIMIT = 7.5   ENTROPY_SAMPLE = 0x10000
```

`is_container(path)`: first 4 bytes (little-endian u32) equal either magic. Fewer than 4 bytes or unreadable: false.

`unwrap_file(input, output)`: if the two paths resolve (symlinks resolved, normalised) to the same file: `Refused("output aliases the input container")`. Read the input, `unwrap_bytes`, create the output's parent directories, write. Returns the output length.

`unwrap_bytes(data)`:

1. `len < 0x400` -> `file is smaller than the SELF header area`. `u32(0)` not a magic -> `not a SELF container (magic 0x<hex lower, no padding>)`.
2. **records**: `offset = 0x40`; while `offset + 0x20 <= 0x400`: `flags = u64(offset)`, `segment = u64(offset+8)`, `size = u64(offset+16)`, `again = u64(offset+24)`. Stop if `size == 0 || size != again`; stop if `segment >= len || segment < 0x350 || segment % 0x10 != 0`; stop if `size > len`. Otherwise push `(flags, segment, size)`, `offset += 0x20`.
3. **inner ELF**: `inner` = index of the first occurrence of `7F 45 4C 46` anywhere in the file (first byte scan from 0), else `no inner ELF header`. `inner + 0x40 > len` -> `inner ELF header is truncated`. `elf_header` = the 64 bytes at `inner`. `elf_header[4] != 2 || elf_header[5] != 1` -> `inner ELF is not 64-bit little-endian` (present in `unwrap_self.py`, missing in the Swift port; the Rust port includes it: it only turns garbage into an early error). `phoff = u64(elf_header, 0x20)`, `entsize = u16(0x36)`, `phnum = u16(0x38)`; `entsize != 56 || phnum == 0` -> `program header table is <phnum> entries of <entsize> bytes`. `inner + phoff` overflow or `inner + phoff + phnum*56 > len` -> `ELF program header table does not fit the file`. **The table is read at `inner + phoff` but written to the output at `phoff`** (not `inner + phoff`): keep this asymmetry.
4. For each header `i`, at `a = inner + phoff + 56*i`: `type=u32(a)`, `flags=u32(a+4)`, `offset=u64(a+8)`, `vaddr=u64(a+16)`, `size=u64(a+32)`, `memory=u64(a+40)`, `align=u64(a+48)`. For `type == PT_LOAD`: `size > memory` -> `segment <i> file size exceeds memory size`; `align != 0 && align & (align-1) != 0` -> `segment <i> alignment is not a power of two`; `vaddr + memory` wraps with a non-zero wrapped sum -> `segment <i> memory range exceeds ELF64`. Each header keeps `source = offset` initially and `placed = 0`.
5. **place segments** (mutates `source`, and for non-loads `size`/`memory`):
   a. `placed` = set of header indexes, `used` = set of record indexes. For each record `r` (index `k`): `target = (flags >> 20)` as signed 64; if `flags & 0x800 != 0 && 0 <= target < headers.len() && target not in placed && headers[target].size == r.size`: `headers[target].source = r.segment`; add `target` to `placed`, `k` to `used`.
   b. `recorded` = headers in `placed` with `vaddr == 0 && type != PT_LOAD`. If non-empty: `last` = the header with the greatest `offset`; among equal offsets the **first** in header order (Swift `max(by:)` and Python `max` both keep the first maximum; add a test). `cursor = last.source + last.size`. `trailing` = headers not in `placed`, `vaddr == 0`, `type != PT_LOAD`, `size != 0`, `offset > last.offset`, sorted by `offset` (stable). For each: add to `placed`; if `type == PT_NOTE`: `source = 0; size = 0; continue`; else `source = cursor; cursor += size; if cursor > len { source = 0; size = 0 }`.
   c. For each `PT_LOAD` header with `size != 0` not in `placed`: pick the first unused record with `record.size == header.size`, mark used, `source = record.segment` (not added to `placed`).
   d. `loadFor(vaddr, size)`: the **last** `PT_LOAD` with `memory != 0 && vaddr_h <= vaddr && (vaddr < vaddr_h + memory || (size == 0 && vaddr == vaddr_h + memory))`, addition wrapping.
   e. `tail: Option<(offset, shift)>`. For each header `h` in order (skip `PT_LOAD`): if `h` in `placed`: if `h.size != 0 && (tail none || h.offset > tail.offset)`: `tail = (h.offset, h.source - h.offset wrapping)`; continue. Else if `h.vaddr != 0`: `host = loadFor(h.vaddr, h.size)`; none: if `h.size == 0` { `h.source = 0`; continue } else error `segment <i> vaddr 0x<hex> is in no PT_LOAD`. `off = h.vaddr - host.vaddr`; if `h.size != 0 && (off > host.size || h.size > host.size - off)` -> `segment <i> does not fit the file-backed range of PT_LOAD <host.index>`; `h.source = host.source + off`; update `tail` as above. Else if `h.size != 0` (vaddr 0, unplaced): `h.source = h.offset + tail.shift` (wrapping, `tail` none -> shift 0); `h.source = min(h.source, len)`; `keep = len - h.source`; `h.size = min(h.size, keep)`; `h.memory = min(h.memory, keep)`.
   f. Check: for each header except `PT_LOAD` with `size == 0`: if `source > len || size > len - source` -> `segment <i> does not fit the file`.
6. **decrypted check**: `loaded` = `PT_LOAD` headers with `size != 0`. For each, the sample is `data[source .. source + min(size, 0x10000)]`; Shannon entropy (bits per byte, log2) `>= 7.5` marks it encrypted. If at least one is marked and all are marked: `every load segment looks encrypted: segment <i> at 0x<source hex>(%.2f bits per byte), ...` joined with `", "`, text per item `segment %d at 0x%llx (%.2f bits per byte)`.
7. **regions**: for each `PT_LOAD` with `size != 0`: region `(old=source, size, vaddr, align)`. For each non-load with `size != 0`: if no region fully contains `[source, source+size)` (`old <= source && source+size <= old+size_r`), add a region `(source, size, vaddr, align)`. Sort by `old`, stable.
8. `cursor = max(0x400, phoff + phnum*56)`. For each region in order: `alignment = align if align != 0 && power of two else 1`; `placed = align_up(cursor, alignment)`; `if vaddr % alignment != placed % alignment { placed += (vaddr % alignment + alignment - placed % alignment) % alignment }` (wrapping adds); `region.new = placed`; `cursor = placed + size`.
9. `empty_end = cursor`; for each `PT_LOAD` with `size == 0`: `empty_end = max(empty_end, vaddr % max(align, 1))`. If `empty_end - cursor > 64 MiB` -> `empty load alignment requires more than 67108864 bytes of output padding`. `cursor = empty_end`.
10. `out = vec![0; cursor]`; copy each region's bytes `data[old .. old+size]` to `out[new..]`.
11. For each header: if `size == 0 && (type == PT_LOAD || source == 0)`: `placed = (type == PT_LOAD) ? vaddr % max(align,1) : 0`; continue. Else the containing region (as in 7) gives `placed = region.new + (source - region.old)`; none: `placed = source`. If `placed + size > out.len()` -> `segment <i> escaped the output`.
12. Write: `out[0..0x40] = elf_header`; `u64 out[0x28] = 0` (e_shoff); `u16 out[0x3C] = 0` (e_shnum); `u16 out[0x3E] = 0` (e_shstrndx). For each header at `at = phoff + 56*i`: `u32 type, u32 flags, u64 placed, u64 vaddr, u64 vaddr (paddr = vaddr), u64 size, u64 memory, u64 align`.

All arithmetic on offsets uses checked or explicit wrapping operations exactly where the Swift code uses `&+`, `&-`. Any index that would panic on malformed input must return `Refused("...")` instead; the Swift code would trap, the Python code raises. Use the Python message where Swift has none. No panics on any input (fuzz test, section 10.1).

Test vectors (appendix A) were produced with `tools/unwrap_self.py` from `cb20fd8`.

### 6.2 `library` (GameLibrary.swift)

`scan(root)`: `visit(root, depth 0)`. `visit(folder, depth)`: `inspect(folder)` -> push and **do not descend**. Else if `depth < 3`: list entries (skip hidden files: names starting with `.`; on Windows also entries with `FILE_ATTRIBUTE_HIDDEN`), for each directory (not a symlink/junction to a directory? Swift follows symlinks via `isDirectory`; keep: follow) whose name is not in `{".AnyPS5", "sce_sys", "sce_module", "sce_modules", "prx", "out", "rel"}` (exact, case-sensitive in Swift; compare case-insensitively on Windows) recurse with `depth + 1`. `isPackage` (macOS bundles) has no equivalent. Result sorted by `name` using case-insensitive ordering (`localizedCaseInsensitiveCompare`; Rust: compare `name.to_lowercase()` then `name`; ties keep scan order).

`inspect(folder)` -> `Option<Game>`:
1. `eboot.bin` must exist (any file type) and `sce_sys/param.json` must be readable and parse as a JSON **object**; else **new:** `sce_sys/param.sfo` parsed as PSF (below); else `None`.
2. From JSON: `title_id = parameters["titleId"]` if string else folder name. `localized = parameters["localizedParameters"]` if object else empty. `language = localized["defaultLanguage"]` if string, else `parameters["defaultLanguage"]` if string, else `"en-US"`. `names = localized[language]` if object, else `localized["en-US"]` if object, else empty. `name = names["titleName"]` if string else `title_id`. `version = parameters["contentVersion"]` if string else `"1.0"`. (A non-string value anywhere is treated as absent.)
3. PSF fallback (UP `ParamSfoParser.cpp`): magic `00 50 53 46`, `key_table = u32(8)`, `data_table = u32(12)`, `count = u32(16)`, entries from byte 20, 16 bytes each: `key_offset u16, format u16, length u32, max_length u32, data_offset u32`. Format `0x0204` = UTF-8 string (NUL terminated within `length`), `0x0404` = 32-bit integer. Required: `TITLE`, `TITLE_ID` (non-empty); `name = TITLE`, `title_id = TITLE_ID`, `version = APP_VER` if present and non-empty else `"1.0"`. Bounds errors -> `None`.
4. `icon`, `background`, `music` = `sce_sys/icon0.png`, `pic0.png`, `snd0.at9` when they exist.
5. `executable`: `Elf` if `eboot.bin` starts with `7F 45 4C 46`; else `Container` if `unwrap::is_container(eboot.bin)` **or** `eboot.bin.esbak` starts with the ELF magic; else `Unreadable`.
6. `id = GameId::from(folder)`.

`folder_size(folder)`: sum of file sizes, regular files only, recursive, symlinks not followed; cancellable.

`LibraryWatcher` (AppModel.watch/rescan): `notify::recommended_watcher` on `root`, `RecursiveMode::NonRecursive`, events of kind create/remove/rename/modify of entries. On an event, if `now - last_scan > 2 s` and `busy` is false (busy = an unlock is queued or running, ARM: `progress.isEmpty`), send `LibraryChanged`. Settling: the watcher thread waits 500 ms after an event and drops further events in that window (the Swift code relies on the 2 s rule; both are kept). `last_scan` is updated by the app when it starts a scan. Additionally the app rescans when its window becomes active and more than 3 s have passed.

`rescan` in the app: `remove_leftovers(root)` first if no unlock is running; then scan; then `records[game] = read_record(unlocked_copy(game, root))` for each game; then select the previous game by id or clamp the selection index (AppModel.rescan).

### 6.3 `toolkit`

`Toolkit::locate(paths, prefs)` in order: environment `ANYPS5_TOOLKIT` (a folder), then `prefs.toolkit` (`{kind: "release"|"folder", path}`), then the newest folder under `%LOCALAPPDATA%\AnyPS5\Toolkit\` that opens. The first that `open`s wins. ARM checked only that the relinker is executable.

`Toolkit::open(root)` requires `root\relinker.exe` (a file) and `root\libs\` with at least one `.prx`, and the three MinGW DLLs in `libs\`; the missing ones are `ToolkitError::MissingRuntime(names)`. A toolkit folder is either **installed** (made by `download_and_install`: has `version.txt`, `patches\`) or **adopted** (a user-chosen self-build, see below): `adopt_folder` accepts a folder that has `relinker.exe` and `libs\`, or a build tree with `core\relinker\relinker.exe` and `core\libs\libs\`, or `build\...` below it (search depth 4), and records the paths in `prefs.toolkit = {kind: "folder", relinker, libs}`; nothing is copied. For an adopted toolkit the MinGW DLLs are looked up in `libs\`, then next to the compiler recorded in `build\CMakeCache.txt` (`CMAKE_CXX_COMPILER`, PR3786 `build_compiler`), then on `PATH`; found DLLs are copied into the unlocked copy's `libs\` like the others (the toolkit folder itself is never written).

Version: `version = "<tag>-<fp>"` where `tag` is the release tag without a leading `v` (`0.1.3`), or `local` for an adopted folder, and `fp` = first 12 hex digits of SHA-256 over, in this order, the bytes of `relinker.exe`, then each regular file of `libs\` in ordinal ascending file-name order (all files, so DLLs count), then each `patches\*.toml` and `*.json` in ordinal order. ARM: `sha256(cat relinker libs/*.prx patches/*.json)[:12]`. `version.txt` caches the string (one line, newline) for installed toolkits; for adopted ones it is recomputed on `open` (hashing ~100 MB takes about 0.3 s, done on the `toolkit` thread, result cached by (path, mtime, size) of every file in `%LOCALAPPDATA%\AnyPS5\Toolkit\adopted.cache.json`). An unreadable `version.txt` gives `"unknown"` (Swift behaviour).

`check_latest(repo)`: `GET https://api.github.com/repos/<repo>/releases/latest` with `User-Agent: anyplay-launcher/<version>`, `Accept: application/vnd.github+json`, 10 s timeout. Default repo `boykopovar/AnyPS5` (setting `toolkitRepository`). Result `ReleaseInfo { tag, published, relinker: Asset, libs: Asset, notes_url }` where the assets are found by name: `relinker-<tag>.exe` and `prx-windows-<tag>.zip` (exact). Missing asset: `ToolkitError::NoRelease`. Rate limit 403/429: `Network("rate limited, try again later")`. No token is ever sent. `Asset { name, url, size, sha256: Option<String> }` takes `digest` (`sha256:<hex>`) from the API when present.

`download_and_install(info)`: stages into `%LOCALAPPDATA%\AnyPS5\Toolkit\<tag>.partial\`: download `relinker` (progress stage `Relinker`), download the zip (`Libraries`, with `Content-Length` progress), verify the SHA-256 when the API gave one (else only the size), extract `libs/*` (entry names normalised; any entry with `..`, an absolute path or a drive prefix is rejected: `BadArchive`), copy the per-title patches shipped with the launcher (`<exe dir>\patches\*.toml|*.json`, section 6.9) into `patches\`, compute version, write `version.txt` and `toolkit.json` (`{tag, published, source: url, installedAt}`), then rename to `<tag>\`. An existing `<tag>\` is replaced only after the new one is complete. The previous toolkit folder stays until the next successful installation, after that all but the newest two are deleted, except the releases games stay on (a toolkit per game, below). `prefs.toolkit` is set to `{kind: "release", tag}`.

**Update detection.** The app checks once at start (if the last check is older than 12 h, `prefs.toolkitCheckedAt`) and when the player opens Settings > Toolkit. `update_available = latest.tag != installed.tag` for a release toolkit, compared as dotted numbers (`0.1.10 > 0.1.9`); an adopted toolkit never reports an update. The check is offline-safe: a network error sets `ToolkitEvent::Latest(Err)` and nothing else. The player can disable the check (`prefs.checkForUpdates`, default true). The check sends only the request above.

**Outdated games**: `state(of:)` returns `installed(outdated: toolkit.version != record.toolkit)` (AppModel.state). So installing a new toolkit makes every unlocked game "outdated" and offers to unlock again; nothing is re-unlocked automatically. `toolkit` is the game's own: the release in `GameSettings.toolkit` when it has one, else the global toolkit.

**A toolkit per game.** `GameSettings.toolkit` is a release tag (`"v0.1.2"`, absent = follow the global choice). `toolkit::pin_tag` accepts only dotted numbers (a pin is joined to `Toolkit\<tag>\` and to nothing else). `game_toolkit(paths, global_version, pin)` gives `Global(version)`, `Pinned { tag, version }` or `Missing { tag }`; a pin whose folder is gone or incomplete (the files `open` asks for on that system, without hashing: relinker, `libs\*.prx`, the MinGW DLLs on Windows, the Vulkan runtime on macOS) is `Missing` and never the global toolkit. `open_for_game(paths, global, pin)` is the opened toolkit for an unlock (`Err(PinMissing)` for a missing pin). The layout of a release folder is the same on Windows, Linux and macOS (relinker, `libs\`, `version.txt`, `toolkit.json`; macOS adds `vulkan\`). `download_and_install_keeping(.., keep, ..)` does what `download_and_install` does and leaves the tags in `keep` (`Preferences::kept_toolkit_tags`: the pinned channel's and every game's) out of the clean-up. `remove_release(paths, tag, in_use)` deletes one installed release and refuses one in use; `installed_with_sizes` lists them. `releases::by_tag` finds a release outside the last ten by asking for its tag. A settings file exported from a game leaves `toolkit` out and an import keeps the player's own.

If no toolkit is found at start, the first-run screen offers: download latest release / choose a folder.

### 6.4 `unlock`: names, record, store (Installer.swift)

`Store` paths: `store(root) = root\.AnyPS5`; `games = store\Games`; `work = store\Work`.

`unlocked_copy(game, root)`: `name = alnum(game.title_id) + "-" + hex(fnv1a64(game.id) & 0xFFFFFF)` and the folder is `store\Games\<name>`.

- `alnum(s)`: keep characters for which `char::is_alphanumeric`, original case (title IDs are already upper case).
- `fnv1a64`: offset basis `0xCBF29CE484222325`, prime `0x100000001B3`, over the UTF-8 bytes of the string; `hash ^= byte; hash = hash.wrapping_mul(prime)`.
- `hex`: lower-case, **no zero padding** (`String(x, radix: 16)`; a value below `0x100000` gives fewer than 6 digits).
- The hashed string on macOS is `folder.standardizedFileURL.path`. On Windows it is `GameId` = the absolute path with `\\?\` stripped, `\` -> `/`, no trailing `/`, lower-cased (`str::to_lowercase`). `GameId` is also the key of every per-game map in the preferences (D23).

Vectors: macOS formula, `title_id=PPSA02100`, path `/Users/x/Games/Stray` -> `PPSA02100-6d6bf` (hash `f8de232e6e06d6bf`). Windows: `PPSA01234` + `c:/games/ppsa01234 - stray` -> `PPSA01234-d356b9`; `c:/games` -> hash tail `602bcc`.

Record file `anyps5-unlock.json`: `{"date": "2026-10-22T09:30:00Z", "source": "<dump folder, OS path>", "titleID": "...", "toolkit": "..."}` keys sorted, pretty printed with 2 spaces. `read_record(copy)` requires `copy\eboot.exe` to exist and the JSON to parse; else `None`. (ARM: `Contents/MacOS/eboot` exists.) Reading also accepts `anyps5-install.json` (name used by ARM) for the same folder.

`remove_leftovers(root)`: delete `store\Work` and every `store\Games\*.partial` directory (ignore errors). The app calls it only when no unlock is queued or running (`installer == nil`), at each rescan.

### 6.5 `unlock`: the pipeline (Installer.run)

`UnlockQueue` owns one worker thread. `enqueue` returns false when the game is queued or running (ARM: `guard !queue.contains, progress[id] == nil`). The worker takes the oldest entry; a game that is no longer in the library when its turn comes (ARM: `games.first(where:)`) is skipped (the app removes it from its queue view on `Finished`). One unlock at a time. `cancel(game)`: queued -> removed without events other than `Finished(Err(Cancelled))`; running -> sets the flag and kills the child.

Events: on start `Started`, then `Progress(Unwrap, 0)`. Progress coalescing (AppModel.startNext): the worker only sends `Progress` when the step changed, or `|fraction - last| >= 0.005`, or `fraction >= 1`. Log lines are sent as `Log`; the app keeps at most 600 lines per game and drops the oldest 100 on overflow.

Steps (`work = store\Work\<uuid>`; on every exit path `work` and `<copy>.partial` are removed):

**1. Unwrap** (`progress(.unwrap, 0)`).
`source = work\source`. `modules = modules_in(game.folder)`; `inputs = ["eboot.bin"] + modules` (relative paths). For each input (index i): check cancel; `original = folder\rel`; `target = source\rel`; create parent; `unwrap_one(original, target)`; `progress(.unwrap, (i+1)/inputs.len())`.

`unwrap_one(original, target)`:
- `original` starts with ELF magic -> copy it byte for byte (no log line).
- else `original + ".esbak"` starts with ELF magic -> copy that; log `"<file name>: from .esbak"`.
- else `is_container(original)` -> `unwrap_file`; log `"<file name>: unwrapped, <n> bytes"`; a `Refused` becomes `UnlockError::Unwrap{file, reason}`, Display `"<file name>: <reason>"`.
- else `NotAnExecutable`.
(The `.esbak` rule is the dump tool's backup of the original ELF next to a re-signed container. UP's relinker has a similar rule for modules that are already ELF.)

`modules_in(folder)`: recursive enumeration with hidden files skipped; keep regular files whose name lower-cased ends with `.prx` or `.sprx`; skip everything under `sce_sys/` (a relative path starting with `sce_sys/`, the walk does not descend); skip a file whose relative path has **more than 3 components**; result relative paths sorted by their string (ordinal; use `/` separators for sorting and for the relinker arguments, convert to `\` for the file system).

`module_directories(modules)`: start `["."]`; for each module with extension exactly `prx` (case-insensitive; **`.sprx` files do not add directories**): `dir = parent of the relative path` (`"."` when none); `first = first component of dir`; skip when `first.to_lowercase()` is in `{sce_module, sce_modules, prx, fakelib}` or when `dir` is already present; else append `dir`. (`fakelib` holds a dump tool's stand-ins for system libraries; AnyPS5 implements them itself.) Each directory becomes `--module-dir <dir>`.

**2. Relink**. `progress(.relink, 0)`. `out = work\out`. Command (working directory `source`, arguments as separate `CreateProcess` arguments, never through a shell):

```
<toolkit>\relinker.exe --windows [--to-intel] --module-dir . [--module-dir <d>]... [--patches <work>\patches.toml] eboot.bin <out>\eboot.exe
```

`--to-intel` per D1. `--patches` only when the title has patches for this build (6.9). The relinker's `sce_module` detection: a dump without `sce_module`, `sce_modules` or `prx` works because `--module-dir .` is passed (UP USAGE.md: "All three absent is an error unless an explicit `--module-dir` is supplied"). Stage `sce_sys\icon0.png` and `sce_sys\param.json` (or `.sfo`) beside `eboot.bin` by hard link (A5). Output is read from stdout+stderr of the child, merged. The pipe is split into segments at `\n` **and** `\r` (`OutputCollector`): every non-empty segment goes to the status callback; a line ends at `\n`, its last rewritten state goes to the log; `lines` keeps the last 400. `RelinkProgress.read(segment)` -> fraction:

- split on `|`, trim each field; need at least 3 fields: `phase`, `file`, `value`. `percent` = the number before the first `%` in `value` (trimmed, parsed as f64) else `None`. `done = clamp(percent/100, 0, 1)`.
- estimate: `Instruction scan` and `file == "eboot.bin"`: `0.2*done`. `Instruction conversion` and `eboot.bin`: `0.2 + 0.25*done`. `Syscall scan` and `eboot.bin`: `0.45 + 0.1*done`. `PRX conversion`: if `value` contains `(`, `/`, `)` and the parts parse as numbers `converted` and `total > 0`: `0.55 + 0.3*converted/total`, else `0.55`. `Output image`: `0.85 + 0.1*done`. `Write output`: `0.95 + 0.05*done`. Anything else (`Executable`, `Import analysis`, unknown file names): no estimate.
- the value only counts when it exceeds the highest value so far (monotone); then it is returned and sent as `Progress(Relink, value)`.

After the child exits: if cancelled -> `Cancelled`; exit status != 0 -> `Relinker{code, last_line: lines.last}` (when `quiet` the collected lines are logged first); `out\eboot.exe` missing -> `NoOutput`. `progress(.relink, 1)`.

**3. Copy** (`progress(.copy, 0)`). `partial = games\<name>.partial` (remove any existing first). Create `partial\app0` and `partial\libs`. `link_tree(folder, partial\app0)`:

- enumerate the dump recursively (hidden files **included**, unlike `scan`); skip `.AnyPS5` at the top (the store may sit inside the dump's parent; ARM compares the relative path to `.AnyPS5` or a `.AnyPS5/` prefix).
- directories are created; files are collected with their sizes; `total = sum`.
- for each file, in enumeration order: check cancel; remove an existing destination; `fsx::link_or_copy(src, dst)`; `done += size`; `progress(.copy, done/total)` (1 when `total == 0`).
- error text `copy <relative>: <OS message>`.

`fsx::link_or_copy`: (1) if source and destination are on an ReFS volume (`GetVolumeInformationW` file system name `ReFS`) try `FSCTL_DUPLICATE_EXTENTS_TO_FILE` (block clone) — optional in v1, behind `cfg`; (2) `CreateHardLinkW` (`std::fs::hard_link`); on `ERROR_NOT_SAME_DEVICE (17)`, `ERROR_INVALID_FUNCTION (1)`, `ERROR_NOT_SUPPORTED (50)`, `ERROR_TOO_MANY_LINKS (1142)`, `ERROR_ACCESS_DENIED` for a read-only source on some network shares -> (3) `std::fs::copy`. The first fallback to copying logs once `"files copied, not linked: <volume reason>"` (PR3786's note). Rationale and hazard (D4): a hard-linked file is the dump's file; **nothing may open a file under `app0` for writing**. The overlay below and the carry-over only create new files or rename; the engine does not write into `app0`.

Then the relinked guest modules: if `out\app0` exists, `link_tree(out\app0, partial\app0)` with the same remove-then-link rule (the files in `out` are ours, so linking them into place is fine; copying instead of linking would also do; linking avoids a second copy). Not needed on Windows: `link_guest_module_libraries` (no symlinks; modules find libraries through `$ORIGIN`, A4).

**4. Assemble** (`progress(.bundle -> .assemble, 0)`).
- `eboot.exe` = copy of `out\eboot.exe` into `partial\`.
- `libs\`: for every regular file in the toolkit's `libs\` (all `.prx` and `.dll`, including `mimalloc*.dll`) `link_or_copy` into `partial\libs\`. For an adopted toolkit add the MinGW DLLs found as in 6.3. After linking verify the three MinGW DLLs exist in `partial\libs`, else `Io("libgcc_s_seh-1.dll (MinGW runtime) not found ...")`.
- patches: the resolution/byte patches were given to the relinker in step 2; log lines the relinker prints as `Patch: ...` pass through to the log (UP USAGE.md "game patches"). The result summary `"<TITLEID> patch <name>: applied|skipped|already applied"` that ARM logs is produced from the relinker's `Patch:` lines and the written `<out stem>.patches.toml` (6.9).
- `progress(.assemble, 0.6)` after the files.
- record: `anyps5-unlock.json` with `source = game.folder` (OS path), `titleID`, `toolkit = toolkit.version`, `date = now UTC`.
- no `Info.plist`, no icon file (D14), no launch script (D5).

**5. Finish** (`progress(.finish, 0)`). No quarantine step (D13). If `copy` exists: `carry_over(copy, partial)` then remove `copy` (with retries, D22). Rename `partial` to `copy`. `progress(.finish, 1)`. Return `UnlockRecord`.

`carry_over(old, new)`: for each entry `name` of `old` (top level only) such that `new\name` does not exist: move it (`rename`; same volume) into `new`. Skips automatically what the new copy already has (`app0`, `libs`, `eboot.exe`, `anyps5-unlock.json`). A symbolic link is never moved. This keeps `_sd`, `_trophy`, `data`, `download0`, `temp0` and the logs the engine wrote there; "unlocking again keeps its saves and trophies". Failure to move one entry is logged and does not fail the unlock (ARM throws; a half-moved state is worse than a lost screenshot, so the port fails only when moving `_sd` or `_trophy` fails).

`remove_unlocked(game, root)`: delete `copy` recursively (saves and trophies in it go too; the app asks first: `confirmUninstall`). Hard links mean the dump is untouched. Use `remove_dir_all` with read-only attribute clearing on error.

### 6.6 `settings`

**Preferences** (file `%APPDATA%\AnyPS5\preferences.json`, replaces `UserDefaults` key `AnyPS5.preferences.v1`): load -> `serde_json` with `#[serde(default)]` on every field, unknown fields kept in a `#[serde(flatten)] extra: Map` so a newer file survives an older launcher. A file that fails to parse is moved to `preferences.json.bad` and defaults are used. Save is atomic (write `preferences.json.tmp`, flush, `rename` over; `ReplaceFileW` when the target exists), triggered by the app whenever the value changes (`didSet` with `!=` check), debounced to at most one write per 250 ms on the `io` worker.

```json
{ "version": 1, "onboarded": false, "language": "en", "theme": 0, "accent": null,
  "backdrop": "theme", "backdropFile": null, "gameArtwork": true, "reducedMotion": false,
  "sounds": true, "hideWhilePlaying": false, "profileName": "Player", "gamesFolder": null,
  "screenshotsFolder": null, "fontsFolder": null,
  "games": { "<GameId>": { ...GameSettings } }, "lastPlayed": { "<GameId>": "2026-10-22T09:30:00Z" },
  "toolkit": null, "checkForUpdates": true, "toolkitCheckedAt": null, "toolkitRepository": "boykopovar/AnyPS5" }
```

`theme`: 0 cosmic, 1 aurora, 2 sunset (Swift raw Int). `accent`: `blue|mint|coral|violet|gold|white` or null (theme default: cosmic blue, aurora mint, sunset coral). `language`: `tr` or `en`; default from the Windows user locale (`GetUserDefaultLocaleName` starting with `tr` -> `tr`). `backdrop`: `theme|image|video`. `profileName`: first word of the Windows display name (`GetUserNameExW(NameDisplay)`), fallback `USERNAME`, fallback `Player`. Console language of the launcher: `tr` -> 19, `en` -> 1 (`Language.consoleLanguage`). Default screenshots folder: Pictures known folder + `AnyPS5`.

**GameSettings** (JSON identical to Swift `Codable` output, so a macOS settings file can be imported):

```json
{ "resolution": null, "fullscreen": false, "windowSize": 75, "vsync": true, "overlay": "off",
  "language": null, "controls": null, "tuning": null, "shaderCache": null }
```

`overlay`: `off|topLeft|topRight|bottomLeft|bottomRight` -> `ANYPS5_OVERLAY_POS` `tl|tr|bl|br`; `off` sets nothing. `controls`: object from action name to array of sources (`"KEY:K"`). `tuning`: the 12 integers of `InputTuning`. Optional fields absent in older files keep their defaults; Rust uses `#[serde(default)]`, whereas Swift's synthesised decoder requires the four non-optional keys, so files **written by the port** always contain all four. Added field (Windows only, ignored by macOS because Swift's decoder ignores unknown keys): `gpu: Option<String>`, `cpuConversion` is a global preference not a per-game one.

`environment(ctx)` -> map, exactly:

```
ANYPS5_PRESENT_MODE = vsync ? "fifo" : "immediate"
ANYPS5_LANGUAGE     = (language ?? launcher_language).to_string()
ANYPS5_OVERLAY = "1", ANYPS5_OVERLAY_POS = tl|tr|bl|br        only when overlay != off
ANYPS5_INPUT_CONFIG = <input file>                             only when Some
ANYPS5_SCREENSHOTS, ANYPS5_SYSTEM_FONTS = <dir>                only when Some
                                                               (fonts: the picked folder, else the built-in
                                                               Noto folder from `fonts::resolve`)
ANYPS5_SHADER_CACHE_DIR = <dir>                                always (the launcher passes it; ARM's launch script did)
ANYPS5_NO_SHADER_CACHE = "1"                                   only when shader_cache == Some(false)
ANYPS5_GPU = <text>                                            only when gpu is Some and non-empty
```

Not set (not in UP): `ANYPS5_FULLSCREEN`, `ANYPS5_WINDOW_SIZE` (swap for D9: `fullscreen` and `window_size` go to the launcher's window logic, `window_size` clamped to 30..=100), `APS5_RESOLUTION`. Values are `String`s; paths are passed in their `\` form; the environment is UTF-16 on Windows so any character works.

`write_input_file(file)`: create parent directories, write `Controls::file(controls or empty, tuning or default)` atomically (temp + rename). On failure the app starts the game **without** `ANYPS5_INPUT_CONFIG` (AppModel.play: `written ? inputFile : nil`), and the game then uses the engine's built-in keys. The input file path is `%APPDATA%\AnyPS5\Controls\<unlocked copy folder name>.ini` (ARM used `<game.id>.ini` with the full folder path as a file name, which creates nested folders; not repeated).

**InputTuning** JSON keys and defaults: `leftDeadzone 0, rightDeadzone 0, mouseSensitivity 10, mouseVerticalScale 10, mouseCurve 10, mouseDeadzoneOffset 20, mouseMaxSpeed 10, mouseSmoothingMs 10, mouseDecayMs 30, stickModifierScale 50, keyboardRampMs 0, triggerRampMs 0`. Missing keys take their default.

**ConsoleLanguage**: `all` = (19 Turkce, 1 English (US), 18 English (UK), 4 Deutsch, 2 Francais, 3 Espanol, 20 Espanol (Latinoamerica), 5 Italiano, 17 Portugues (Brasil), 7 Portugues, 8 Russian, 16 Polski, 6 Nederlands, 0 Japanese, 9 Korean, 11 Simplified Chinese, 10 Traditional Chinese, 21 Arabic) with native names exactly as in `GameSettings.swift`. `tag(n)` = `["ja-JP","en-US","fr-FR","es-ES","de-DE","it-IT","nl-NL","pt-PT","ru-RU","ko-KR","zh-Hant","zh-Hans","fi-FI","sv-SE","da-DK","no-NO","pl-PL","pt-BR","en-GB","tr-TR","es-419","ar-AE"][n]` or `None`.

**SharedSettings** (export/import): `{"format":"anyps5-settings","version":1,"titleID":"...","game":"<name>","settings":{...GameSettings}}`, pretty printed, sorted keys. `read(bytes)`: parse; `format != "anyps5-settings"` -> `NotSettingsFile`; a parse failure -> `Json`. Import rules (AppModel.importSettings): a `resolution` not offered by this title's patch choices is cleared; the whole `GameSettings` is replaced; the toast says whether `titleID` matched. A file from the macOS launcher imports: `fullscreen`/`windowSize` are honoured, `resolution` is dropped unless valid on Windows, unknown fields ignored.

### 6.7 `controls` (Controls.swift)

`PadAction` (raw names, in this order, also the order of the ini lines): `Cross Circle Square Triangle L1 R1 L2 R2 L3 R3 Options TouchLeft TouchRight Up Down Left Right LeftStickUp LeftStickDown LeftStickLeft LeftStickRight RightStickUp RightStickDown RightStickLeft RightStickRight StickModifier`.

Default `layout` (sources are `KEY:<SDL scancode name>`, `MOUSE:<Left|Middle|Right|X1|X2>`, `WHEEL:<Up|Down>`):

```
Cross: KEY:K KEY:Space KEY:Return     Circle: KEY:L           Square: KEY:J MOUSE:Left   Triangle: KEY:I
L1: KEY:Q   R1: KEY:E                  L2: KEY:U MOUSE:X1      R2: KEY:O MOUSE:Right
L3: KEY:Left Shift   R3: KEY:Left Ctrl Options: KEY:Escape      TouchLeft: KEY:Backspace   TouchRight: KEY:Tab
Up: KEY:Up WHEEL:Up  Down: KEY:Down WHEEL:Down  Left: KEY:Left  Right: KEY:Right
LeftStick Up/Down/Left/Right: W S A D           RightStick Up/Down/Left/Right: T G F H
StickModifier: (none)
```

This differs from UP's built-in defaults (UP: `L2` = R, `R1` = E + Alt, `Circle` = C), on purpose: the layout is the launcher's own (GUIDE.md table). Because the file lists every action, none of the engine's scattered defaults stay (comment in `Controls`).

- `sources(action, changes)` = `changes[action]` if present (even empty) else `layout[action]` else empty.
- `assign(key, to action, changes)`: copy `changes`; for every other action `a` (in enum order): `current = sources(a, changes)` (the **original** `changes`); if it contains `key`, `result[a] = current minus key` (this may produce an empty array = explicitly unbound). Then `result[action] = [key] + sources(action, changes).filter(not starting with "KEY:")` (replaces its keys, keeps its mouse and wheel sources). Finally remove from `result` every entry equal to the default layout for that action (`layout[action] == value`; an action name that is not a `PadAction` is removed too). Mouse buttons stay on their action.
- `file(changes, tuning)`: first line `# Written by AnyPS5.app at each start; change the keys in the game's settings there.` (port: `# Written by the AnyPS5 launcher at each start; change the keys in the game's settings there.`), then for each action in order its sources; an action with no sources writes `<Action> = KEY:F24` (unbound; the engine refuses an empty source); then `tuning.lines`; every line ends with `\n`. UTF-8, no BOM.
- `InputTuning.lines`: for each setting, clamp then write only when the **clamped** value differs from the default: `LeftStickDeadzone 0..=90 (integer)`, `RightStickDeadzone 0..=90`, `MouseSensitivity 1..=100 (/10)`, `MouseVerticalScale 1..=100 (/10)`, `MouseCurve 2..=50 (/10)`, `MouseDeadzoneOffset 0..=90 (/100)`, `MouseMaxSpeed 1..=100 (/10)`, `MouseSmoothingMs 0..=500 (integer)`, `MouseDecayMs 1..=1000`, `StickModifierScale 5..=100 (/100)`, `KeyboardRampMs 0..=2000`, `TriggerRampMs 0..=2000`. Format `"<Name> = <value>"`. A divided value prints as a Swift `Double`: `25/10` -> `2.5`, `20/10` -> `2.0`, `1/10` -> `0.1`, `5/100` -> `0.05`, `100/100` -> `1.0`: in Rust `format!("{:?}", v as f64 / 10.0)` (Debug of `f64` prints the shortest round-trip form with a `.0` for integers, like Swift); `{}` would print `2`. These ranges match UP INPUT_MAPPING.md (checked: sensitivity (0,10], vertical 0.1..10, curve 0.2..5, offset 0..0.9, max speed 0.1..10, smoothing 0..500, decay 1..1000, scale 0.05..1, ramps 0..2000, deadzone 0..90).
- Key capture (D15): `fn key_name(scancode: u16 /* set 1, E0 prefix as 0xE000 | code */) -> Option<&'static str>` per appendix B. The app captures by polling `GetAsyncKeyState` for every VK while capturing, converting a new press with `MapVirtualKeyW(vk, MAPVK_VK_TO_VSC_EX)`; modifier keys count only on press (ARM: `flagsChanged` only when pressed), Escape cancels the capture (and is never assigned), F24 and keys without a name are swallowed without effect.
- The UP file format rules the port relies on: `#`/`;` comments, first line of an action replaces the built-ins, later lines add, duplicates ignored, an invalid line stops input initialisation and names file and line. Hence the launcher writes only names from the table and the three source types.

### 6.8 `trophies` (Trophies.swift) — logic unchanged

`TrophySet { title, icon: Option<Vec<u8>>, trophies: Vec<Trophy> }`, `Trophy { service_label, number, grade, hidden, name, detail, icon: Option<Vec<u8>>, unlocked: Option<i64 /* unix seconds, microsecond precision kept as i64 micros */>, progress: Option<(u64 value, u64 target)> }`, `id() = "<label>/<number>"`. `Grade` letters `P G S B` with weights 300, 90, 30, 15. `percent = unlocked_points * 100 / total_points` (integer division, 0 when total 0). `count(grade) = (unlocked, total)`.

`load(game, copy, languages)`: for `label` in `0..100`: file `game.folder\sce_sys\trophy2\trophy%02d.ucp`; read failure: if `label == 0` continue (labels may start at 1), else stop. `readUcp` failing or `parse` returning `None` -> continue. For `label == 0` or while no trophies were collected, `title` and `icon` come from that package. Concatenate trophies. Result `None` when empty. `records = copy\_trophy` (`None` when no copy: every trophy shows locked).

`.ucp` archive (big-endian): `u32@0 == 0xB228C60A`, `u32@4 == 1`, `count = u32@0x10 <= 4096`, `toc = u32@0x14`; entry `i` at `toc + 0x20 + 0x40*i`: name = 0x20 bytes up to the first NUL (UTF-8), `offset = u64@+0x20`, `length = u64@+0x28`; `offset + length <= file length` else the whole archive is invalid (`None`). Files are keyed by name (a later entry with the same name replaces an earlier one).

`parse(files, label, languages, records)`: `tropconf.json` must be a JSON object (UTF-8 BOM stripped) with `trophies` array. `fallback = conf.defaultLanguage ?? "en-US"`; `candidates = languages + ["en-US","en-GB", fallback]`; `available` = file names `tropmeta_<lang>.json` -> `<lang>` (drop 9 chars from the front, 5 from the back); `language` = first candidate in `available`, else the alphabetically first available, else `None`. Texts: `tropmeta_<language>.json` -> `metadata.trophyMetadata[]` entries `{id, name, detail}` (`id` numeric or `"0007"` string; ids above `i32::MAX` ignored). Per definition `{id, grade, hidden, unlockCondition{progressive, udsStatId, targetValue}}`: skipped when `id` or `grade` is invalid; `progress` only if `progressive == true`, `udsStatId` and `targetValue` parse as unsigned and `target > 1`, value `min(stats[stat], target)`. Icon of a trophy: `trop%04d.png` (id padded to 4). Set icon: `icon0_<language>.png`, else `icon0.png`, else the first `icon0*` in name order. Set title: `metadata.titleMetadata.name` or empty.

`unlocked(records, label)`: every file `<digits>_<label>.txt` (user ids all digits, `ends_with("_<label>.txt")`, excluding `.stats.txt` since `.stats.txt` ends with `_<label>.stats.txt`) is read as UTF-8, lines split at any newline, each line split at single spaces into exactly two numbers `<trophy id> <tick>`; `tick >= 62_135_596_800_000_000` (microseconds since 0001-01-01; the Unix epoch) else ignored; time = `(tick - epoch)` micros; the earliest time across users wins. `stat_values`: `<user>_<label>.stats.txt`, lines `<stat id> <value>`, the maximum across users. Files that cannot be read (the engine is writing) are skipped. `earned(copy)`: labels from the `.txt` names (the last `_`-separated part before `.txt`, not `.stats.txt`), then every `"<label>/<id>"` unlocked. The watcher calls `earned` once a second while a game runs and `load` only when the set grew.

### 6.9 `patches`

macOS patches (`tools/macos/patches/<TITLEID>.json`, one file today: `PPSA02100.json`, Stray) are addresses in the **relinked Mach-O** and edit the finished executable (`game_patches.py`, `GamePatches.swift`). Neither the addresses nor the in-place edit carry over:

- UP's relinker applies byte patches *during* conversion, from a TOML subset, in the ELF's virtual address space, and checks `expected` bytes ("a patch for another version of the title fails instead of corrupting it"; the conversion fails before writing output when bytes differ). Format:

```toml
format = 1
[[patch]]
name = "keep_window_size"
target = "eboot.bin"
va = 0x40F675E
expected = "0f84d2000000"     # hex byte string (no 'type'), same length as value
value    = "90e9d2000000"
description = "..."
```

(`expected`/`value` are "hexadecimal byte strings of the same length" when `type` is absent; the exact quoting of byte strings is assumption A9, to confirm against `core/relinker/relinker/tests` before coding.)

**Port design.** The launcher ships its own per-title files `patches\<TITLEID>.json` (in the launcher's install folder, copied into the toolkit folder on install so they take part in the fingerprint). Schema = the ARM schema plus `"space": "elf"`: `address` fields are ELF virtual addresses. `PatchSet::load(title_id, dir)` ignores files without `"space": "elf"` (the macOS file is not usable as is). From the JSON the core generates `relinker-patches.toml` text:

- every `patches[].edits[]` -> one `[[patch]]` with `name = <patch name>_<index>` (`[a-z0-9_]` only), `target = "eboot.bin"`, `va = <address>`, `expected = original`, `value = replacement`;
- the resolution section -> for each field one `[[patch]]` with `expected` = the field's `instruction` bytes and `value` = the same bytes with the 4 bytes at `offset` replaced by the chosen width/height as little-endian u32 (default from `resolution.default`, or the game's `settings.resolution` when it is one of `choices`). Changing the choice changes the file, so the player must unlock again; the unlock record stores `resolution` (extra key) and `state(of:)` reports `installed(outdated: true)` when `settings.resolution` differs from the record's (D10).

ARM's apply semantics are kept as far as the relinker allows: a patch whose bytes already equal the replacement is "already applied" (not expressible in the TOML; the generated file lists only entries; the dump holds the original bytes so this does not occur), a mismatch fails the whole conversion (stricter than ARM's "skipped: ..." per patch). To keep ARM's softer behaviour the core first **verifies each patch itself** against the staged `eboot.bin` through an ELF program header walk (`elf_file_offset(vaddr)` over `PT_LOAD`: same maths as `GamePatches.fileOffset` but on ELF segments: `p_vaddr <= va && va + len <= p_vaddr + p_filesz` -> `p_offset + (va - p_vaddr)`), and only passes entries whose original bytes match; the others are logged as `<TITLEID> patch <name>: skipped: <va> holds <hex>, expected <hex>`. Applied ones are logged `... : applied`. (Pure function: `PatchSet::plan(&PatchSet, elf: &[u8], resolution: Option<&str>) -> (Vec<PatchEntry>, Vec<(String, PatchState)>)`, fully unit-testable with a synthetic ELF.)

- `PatchState` strings: `applied`, `already applied`, `skipped: <address as written> holds <hex>, expected <hex>`.
- `parse_resolution("WxH")`: `lower-case`, split at `x`, exactly two parts, both `u32` > 0.
- `resolution_choices(game)` = the `choices` of the title's `elf`-space file, else empty (then the interface shows no resolution row).

Nothing in the Windows patch set exists yet: v1 ships none. The conversion of the Stray file is blocked by A1 (address mapping) and needs a machine that may run the relinker's `--verify-patches` (which writes nothing) on the real dump.

### 6.10 `launch` (AppModel.play, watchForWindow, watchForExit, diagnose)

`LaunchRequest { game, copy: PathBuf, settings, launcher_language, input_file: Option<PathBuf>, screenshots, fonts, shader_cache, log_file, window: WindowPrefs }`.

`Launcher::start`:

1. Precondition: `copy\eboot.exe` exists (`NotUnlocked`). The app also guards `starting == nil && running == nil` (one game at a time).
2. **Log start marker** (AppModel.markStart): create the log directory; append `"\n[anyps5-launcher] start <ISO-8601 UTC> <TITLEID>\n"`; the file is created when missing. If the log is larger than 50 MiB, first truncate it to its last 4 MiB (new; ARM never truncates). Record `started_at`.
3. Create the screenshots folder (ignore errors); write the input file (6.6); create the shader cache folder.
4. **Spawn**: `Command::new(copy\eboot.exe)`, `current_dir(copy)`, `env` = inherited + `settings.environment`, `creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)`, `stdin(null)`, stdout and stderr both the write end of one `std::io::pipe()` (cloned) so that the order of lines is the order written. Assign the process to a new job object (`CreateJobObjectW`, no kill-on-close) immediately after spawn (race with the title's own children is negligible: the title needs seconds to load before it can start another process). `AllowSetForegroundWindow(pid)`.
5. **Pump thread** `run-pump`: reads chunks (8 KiB), appends to the log file (open once in append mode, `write_all`, flush per chunk), and keeps a ring buffer of the last 400 KiB. Ends at EOF (all handles closed: i.e. when the whole job has ended or the title closed its stdio).
6. **Watcher thread** `run-watch`: loop every 400 ms up to a deadline of 240 s from spawn: `alive = job.active_processes > 0`; if not alive -> exit path; if `has_window(job pids)` -> send `WindowShown`; the app then ends the splash, brings the game forward (D8), applies D9. After `WindowShown` the thread waits for the job to end (blocking on the job's completion port, `JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO`). If the 240 s pass without a window and the job is alive: send nothing more and keep waiting for the end (ARM: the splash simply ends at the deadline: `guard Self.isAlive(pid)` else `applicationEnded`, then splash cleared). To match: at the deadline send `WindowShown { forced: true }` so the splash does not hang forever.
7. **Exit path**: `exit_code` = exit code of the main process (`GetExitCodeProcess`; `None` when the launcher killed it: `kill`/`cancel_start` sets a flag); join the pump thread (so the log tail is complete, max 2 s); send `Exited { info: ExitInfo { code, had_window } }`; start the diagnosis (6.11). ARM waited 1.5 s for the macOS crash report; Windows has none, so there is no delay.

`has_window(pids)`: `EnumWindows`; for each window whose `GetWindowThreadProcessId` pid is in the job: `IsWindowVisible`, `GetWindow(GW_OWNER) == 0`, extended style without `WS_EX_TOOLWINDOW`, `DwmGetWindowAttribute(DWMWA_CLOAKED) == 0`, and `GetWindowRect` width > 200 and height > 150 (ARM: width > 200, height > 150, layer 0). The set of pids is `QueryInformationJobObject(JobObjectBasicProcessIdList)`.

`RunHandle::kill`: `TerminateJobObject(job, 1)` (cancel on the splash: AppModel.cancelStart, also stops children). `activate`: `SetForegroundWindow` of the found window.

Window placement (D9), run by the app after `WindowShown` through `win::place_window(hwnd, prefs)`: `windowSize` percent (30..=100) of the **work area** of the monitor containing the window (`MonitorFromWindow`, `GetMonitorInfoW`), centred, with `SWP_NOZORDER | SWP_NOACTIVATE`; skipped when `fullscreen`; then fullscreen via F11 as in D9. Both only once per start. Documented as a deviation; if upstream adds window environment variables the code is replaced.

`hideWhilePlaying`: after `WindowShown` the launcher's window is minimised (not hidden: a hidden egui viewport stops repainting and the trophy banner must keep working) and restored on `Exited`.

Trophy banner: shown by the app above all windows (separate always-on-top, click-through, no-activate window, `WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_LAYERED`) so it appears over the game, also fullscreen windowed. Exclusive fullscreen (Vulkan) can hide it; A10.

### 6.11 `diagnosis` (Diagnosis.swift)

`last_session(log)`: if `START_MARKER` occurs, the text after its **last** occurrence (not including the marker); else the last 64 KiB of the log. `read_last_session(path)`: read the last 4 000 000 bytes (lossy UTF-8) then `last_session`.

`read(session, exit)`: the first rule that matches wins, in this order. The order follows ARM (module, uncaught exception, device lost, out of memory, crash); (W) marks Windows additions or replacements. All regexes are multi-line (`(?m)`); `firstMatch` returns capture group 1 of the first match.

1. **Missing module**. ARM: `Library not loaded: (?:@rpath/)?(\S+)` -> `MissingModule(basename)` (basename = last path component). Windows: `(?m)^Failed to load module: (.+?)\s*$` -> `MissingModule(basename(group1, split on both \ and /))`. Else `(?m)^FAIL: unresolved ELF import (\S+)` -> `Unsupported(group1)`. Else, if `exit.code == Some(0xC0000135)` (STATUS_DLL_NOT_FOUND: an import of the executable itself is missing) -> `MissingModule("MinGW runtime (libgcc_s_seh-1.dll, libstdc++-6.dll, libwinpthread-1.dll)")`.
2. **Host CPU** (W, new): `The host CPU \(.*?\) lacks ([^.]+)\.` -> `HostCpu(group1)`. Before rule 3 because the same text arrives inside an uncaught exception and must not become `EngineError`.
3. **Uncaught C++ exception**: ARM macOS regexes `uncaught C\+\+ exception \S+ of type [^\n,]+, what\(\): ([^\n]+)` and `terminating (?:due to|with) uncaught exception of type \S+?: ([^\n]+)`. Windows (W): `(?m)^terminate called after throwing an instance of '[^'\n]+'\r?\n\s*what\(\):\s+([^\r\n]+)` -> `classify(group1)`. Keep the two ARM expressions as well (a user may attach a macOS log; same code runs in `anyps5-tool`).
4. `session.contains("VK_ERROR_DEVICE_LOST")` -> `GpuLost`. Also (W) `Vulkan result -4`.
5. `session.contains("VK_ERROR_OUT_OF_DEVICE_MEMORY") || contains("std::bad_alloc")` -> `OutOfMemory`. Also (W) `Vulkan result -2` or `Vulkan result -1`.
6. (W, new) `[gpu] driver failure: (.+)` -> `classify(group1)`. `(?m)^FATAL: unhandled exception 0x([0-9a-fA-F]{8}) on thread` -> `Crashed(name(code))`, with the following line appended when it has the form `  read of 0x...` / `  write of` / `  execute of`: `EXCEPTION_ACCESS_VIOLATION (write of 0x0000000000000008)`. `FATAL: abort() (SIGABRT)` -> `Crashed("abort()")`. `FATAL: UCRT invalid parameter` -> `Crashed("invalid parameter")`. `FATAL: C++ exception (\S+) raised on thread \d+ cannot be delivered` -> `EngineError("C++ exception <type> cannot be delivered")`.
7. ARM: the crash report's JSON `exception` -> `Crashed("<type> (<signal>)")`. Windows replacement: `exit.code` is a crash status and no earlier rule fired -> `Crashed(name(code))`, `0xC0000017 STATUS_NO_MEMORY` -> `OutOfMemory`; exit code 3 -> `Crashed("abort()")`. Names: `0xC0000005 EXCEPTION_ACCESS_VIOLATION`, `0xC000001D EXCEPTION_ILLEGAL_INSTRUCTION`, `0xC0000094 EXCEPTION_INT_DIVIDE_BY_ZERO`, `0xC00000FD EXCEPTION_STACK_OVERFLOW`, `0xC0000409 STATUS_FAIL_FAST_EXCEPTION`, `0xC0000374 STATUS_HEAP_CORRUPTION`, `0xC0000142 STATUS_DLL_INIT_FAILED`, `0xC000007B STATUS_INVALID_IMAGE_FORMAT`, `0x80000003 EXCEPTION_BREAKPOINT`, any other code whose top nibble is `C` or `8`: `0x%08X`. Exit code 0, other small codes and `None` (killed by the launcher) give no diagnosis.
8. `None`.

`classify(message)`: trim spaces and tabs. `^(\S+) not implemented` -> `Unsupported(group1)`. Contains `VK_ERROR_DEVICE_LOST` or (case-insensitively) `device lost` -> `GpuLost`. Contains `VK_ERROR_OUT_OF_DEVICE_MEMORY` or case-insensitive `out of memory` -> `OutOfMemory`. W: `Vulkan result -4` -> `GpuLost`, `Vulkan result -1|-2` -> `OutOfMemory`. W: starts with `The host CPU (` and contains ` lacks ` -> `HostCpu(missing list)`. Otherwise `EngineError(trimmed)`; longer than 200 **characters**: first 200 characters followed by `…`.

`summary_key` / texts (English from Localization.swift; the app has Turkish too): `missingModule`: "Missing module: %@. Unlocking the game again may fix it." (on Windows the second sentence only when the module is not a MinGW DLL; for MinGW names: "Missing module: %@. Install or choose a toolkit that includes the MinGW runtime."), `unsupported`: "The game asked for a system function that is not supported yet: %@", `gpuLost`: "The GPU stopped responding.", `outOfMemory`: "The game ran out of memory.", `engineError`: "Engine error: %@", `crashed`: "The game crashed (%@).", new `hostCpu`: "This CPU lacks %@, which the game needs." The Turkish strings of ARM are kept for the existing six.

`without_window`: the game ended before `WindowShown` and no diagnosis was found -> the app shows "The game did not start" (`startFailed`).

**Report** (`DiagnosticReport.text`), lines in this order, then a trailing newline:

```
AnyPlay Launcher report, <ISO-8601 UTC now>

Game: <name> (<titleID>, version <version>)
PC: <CPU brand string>, <logical cores> threads, <RAM GB> GB
GPU: <adapter names from DXGI, "; "-joined, software adapters excluded>
Windows: <ProductName> <DisplayVersion> (build <CurrentBuild>.<UBR>)
Launcher: <version>
Toolkit: <toolkit version or "none">
Reason: <diagnosis summary or "none found">

Settings:
<GameSettings as pretty JSON, sorted keys>

[Crash report: <exception name>]        // only when the exit code or a FATAL block gave one; preceded by a blank line

Log of the last start:
<last 400 KiB of the session>
```

`PC:` replaces ARM's `Mac: <hw.model>, <cpu brand>, <GB> GB`, `Windows:` replaces `macOS:`. CPU brand via CPUID leaves `0x80000002..4`, RAM via `GlobalMemoryStatusEx` (total physical, whole GB, rounded down like ARM's integer division), Windows version from `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion`. File: `%LOCALAPPDATA%\AnyPS5\Logs\Reports\<TITLEID>-<yyyyMMdd-HHmmss local>.txt`; after writing, `explorer.exe /select,<file>`.

### 6.12 `hostcpu`

UP: titles run natively and need AVX2, FMA, F16C, BMI1, BMI2, LZCNT and MOVBE; the engine stops at start and says so (`HostCpu.cpp`). `HostCpu::detect()` uses `std::arch::is_x86_feature_detected!` for `avx2 fma f16c bmi1 bmi2 lzcnt movbe` and CPUID vendor. `missing_features()` -> names. The app shows a warning row on the home screen and in Settings (replacement of `rosettaMissing`) and does not block Play.

## 7. Persistence summary

| What | Path | Written by |
|---|---|---|
| Preferences | `%APPDATA%\AnyPS5\preferences.json` | `Preferences::save` |
| Input files | `%APPDATA%\AnyPS5\Controls\<TITLEID>-<hex>.ini` | `write_input_file` at each start |
| Toolkit | `%LOCALAPPDATA%\AnyPS5\Toolkit\<tag>\` | `download_and_install` |
| Logs | `%LOCALAPPDATA%\AnyPS5\Logs\<TITLEID>.log` | `run-pump` |
| Reports | `%LOCALAPPDATA%\AnyPS5\Logs\Reports\` | `report` |
| Shader caches | `%LOCALAPPDATA%\AnyPS5\Cache\org.anyps5.<titleid>\shader_cache` | engine (`ANYPS5_SHADER_CACHE_DIR`); "clear" deletes `Cache\org.anyps5.<titleid>` |
| Screenshots | `Pictures\AnyPS5` default | engine (`ANYPS5_SCREENSHOTS`) |
| Unlocked copies, work | `<games folder>\.AnyPS5\` | `unlock` |

`AppPaths` is a struct with these roots, constructed once (from known folders) and injected everywhere, so tests use temp directories. `bundle identifier` for caches = `"org.anyps5." + lowercased alphanumerics of the title ID` (Game.bundleIdentifier).

## 8. The app: state machine (AppModel.swift)

`anyps5-launcher::model::AppModel` owns all state, is a plain struct updated only by `pump()` and by UI actions on the UI thread, and has no egui types. Views read it. No view starts a thread; actions call core functions that spawn.

### 8.1 State

```rust
pub struct AppModel {
    prefs: Preferences, games: Vec<Game>, scanning: bool, selected: usize, focus: HomeFocus, overlay: Option<Overlay>,
    progress: HashMap<GameId,(Step,f64)>, queue: Vec<GameId>, failures: HashMap<GameId,String>,
    running: Option<RunHandle>, starting: Option<GameId>, sizes: HashMap<GameId,i64 /* -1 = measuring */>,
    records: HashMap<GameId,UnlockRecord>, trophies: HashMap<GameId,TrophySet>, trophies_read: HashSet<GameId>,
    diagnoses: HashMap<GameId,Diagnosis>, toast: Option<Toast>, starts: HashMap<GameId,Instant>,
    install_log: HashMap<GameId,Vec<String>>, toolkit: Option<Toolkit>, latest: Option<ReleaseInfo>, host_cpu_missing: Vec<String>,
    started_without_window: bool, last_scan: Instant, ...
}
pub enum GameState { Unreadable, Locked, Queued, Installing{step,fraction}, Failed(String), Installed{outdated: bool}, Starting, Running }
```

`Overlay`: `Settings, GameSettings, Library, Log, ConfirmUninstall, Controls, Trophies, Advanced`. `HomeFocus`: `Top(i), Tiles, Actions(i)`. Panel navigation fields (`panelCategory`, `panelRow`, `panelInRows`, `trophyIndex`, `libraryIndex`, `onboardingStep`, `onboardingChoices`, `capturing`) stay in the model exactly as in Swift; `open(overlay)` sets `panelCategory = 0`, `panelRow = 0`, `panelInRows = overlay in {GameSettings, ConfirmUninstall, Controls, Advanced}`, with `panelRow = 1` for Controls and Advanced and `trophyIndex = 0` for Trophies.

### 8.2 `state(of:)`

First match: `starting == game` -> `Starting`; `running.game == game` -> `Running`; `progress[game]` -> `Installing`; `queue.contains` -> `Queued`; `failures[game]` -> `Failed`; `records[game]` -> `Installed{ outdated: toolkit.version != record.toolkit (or resolution differs, 6.9) }` — when there is no toolkit `outdated = false`; `executable == Unreadable` -> `Unreadable`; else `Locked`.

### 8.3 Actions and transitions

| Action | Guard | Effect |
|---|---|---|
| `primary(game)` | | `Installed` -> `play`; `Locked` or `Failed` -> `unlock`; `Installing` or `Queued` -> `cancel_unlock`; `Running` -> `running.activate()`; `Starting` -> nothing; `Unreadable` -> toast `unreadableDump` |
| `unlock(game)` | root set | toolkit missing -> toast `noToolkit`, stop. `failures.remove`. Return when queued or `progress` exists. `queue.push`, play sound `select`, `UnlockQueue::enqueue` |
| event `Unlock(Started)` | | `progress[g] = (Unwrap, 0)`, `install_log[g] = []`, remove from `queue` |
| event `Unlock(Progress)` | | `progress[g] = (step, fraction)` (already coalesced by core) |
| event `Unlock(Log)` | | push; at 600 lines drop the first 100 |
| event `Unlock(Finished Ok)` | | `progress.remove`, `records[g] = rec`, sound `unlocked`, toast "<name> · Ready" |
| `Finished Err(Cancelled)` | | `progress.remove`, nothing else |
| `Finished Err(e)` | | `progress.remove`, `failures[g] = e.to_string()`, toast "Unlock failed: <name>" |
| `cancel_unlock(game)` | | queued: remove from `queue`; running: `UnlockQueue::cancel`; sound `back` |
| `uninstall(game)` | root | `remove_unlocked`, `records.remove`, toast `uninstallDone` |
| `clear_shader_cache(game)` | | delete cache folder, toast `cacheCleared` |
| `play(game)` | root set, `starting == None`, `running == None` | `diagnoses.remove`; `starts[g] = now`; sound `select`; `prefs.lastPlayed[g] = now`; `starting = Some(g)`; `Launcher::start`; on `Err`: `starting = None`, toast `startFailed: <error>` |
| event `Launch(Spawned)` | `starting == g` | `running = Some(handle)`; start `TrophyWatcher`. If `starting != g` (cancelled meanwhile) kill the handle |
| event `Launch(WindowShown)` | | place window and foreground (6.10); `starting = None`; hide launcher if `hideWhilePlaying` |
| `cancel_start()` | `starting.is_some()` | sound `back`, `starting = None`, `running.kill()` |
| event `Launch(Exited)` | `running` matches | `running = None`; `loadTrophies(game, again)`; stop `TrophyWatcher`; if `starting.is_some()` { `starting = None`; `started_without_window = true` }; restore launcher window and bring forward; the core then sends `Diagnosed` |
| event `Diagnosed` | | `diagnoses[g] = d`; if `Some`: toast "<name>: <summary>"; else if `without_window`: toast `startFailed` |
| `make_report(game)` | | core `report` thread; on `ReportWritten`: reveal, toast `reportSaved` / `reportFailed` |
| `rescan()` | root set | `last_scan = now`; `scanning = true`; if no unlock is running `remove_leftovers`; spawn `scan`; restart the watcher |
| event `LibraryScanned` | | replace `games` and `records`; keep selection by id else clamp; `scanning = false`; `measure_selected()` |
| `measure_selected()` | | `load_trophies(selected)`; if no size yet: `sizes[g] = -1`, spawn `measure` |
| `load_trophies(game, again)` | root | return when `!again && trophies_read.contains`; insert; spawn load with `trophy_languages(game)` |
| `trophy_languages(game)` | | `[ConsoleLanguage::tag(settings.language ?? prefs.language.console_language())]` or empty |
| `update(game, f)` | | apply `f` to the game's settings (default if absent), store; if `language` changed `load_trophies(again)` |
| `choose_folder(path)` | | `gamesFolder = path`, `selected = 0`, `rescan()` |
| window activated | | `active = true`; rescan when `now - last_scan > 3 s`; reload the selected game's trophies (again) |
| `export_settings` / `import_settings` | | native file dialog (`rfd`), 6.6 |
| `open_log(game)` | | text = `install_log` joined + blank line + failure + blank line + last 60 000 bytes of the title log; empty -> "—"; `open(Log)` |

Timers: toast 3.2 s; trophy poll 1 s; window poll 400 ms; window deadline 240 s; watcher debounce 2 s; rescan on activation > 3 s.

Game controller/keyboard navigation of the UI (`Input.swift`) and sounds are app-crate concerns and follow `Input.swift`, `Sounds.swift`; `gilrs` replaces `GCController`.

## 9. Test plan

All tests are `cargo test` unit or integration tests in `anyps5-core` unless marked. **No game data, keys, firmware, or relinker output is in the repository.** Fixtures are built in code by helpers in `crates/anyps5-core/tests/support/` (`self_builder`, `ucp_builder`, `dump_builder`, `sfo_builder`, `log_samples`), written into `tempfile::tempdir()`.

### 9.1 `unwrap`

- Table-driven vectors from appendix A: input generated by `self_builder`, expected output length and SHA-256 recorded from `tools/unwrap_self.py`. V1 (4352 bytes), V2 (32896 bytes, two loads, PT_NOTE zeroed, GNU_STACK), V3 (refused: `every load segment looks encrypted: segment 0 at 0x1000 (8.00 bits per byte)`).
- Error strings, one test each: smaller than 0x400; wrong magic (`not a SELF container (magic 0x...)`); no inner ELF; truncated header; `entsize != 56`; table beyond file; `size > memory`; alignment 3; vaddr in no `PT_LOAD`; empty load padding > 64 MiB.
- Property test (`proptest`, dev-dependency): for random mutations of V1 and V2 `unwrap_bytes` never panics.
- Cross-check: when `UNWRAP_SELF_PY` points to a checkout (CI job on a runner with Python and the ARM clone), generate 200 random valid containers and compare to the script's output byte by byte (job `oracle`, not in the default `cargo test`).
- `is_container` for both magics, a 3-byte file, a missing file, a directory.
- `unwrap_file` aliasing: same path, hard link, and symlink (skipped on platforms without symlink privilege) -> `output aliases the input container`.

### 9.2 `library`

- `inspect`: param.json with all fields; missing `titleId` (folder name); `localizedParameters` with `defaultLanguage`; fallback to `en-US`; non-string values; JSON array instead of object (-> `None`); param.sfo only (builder) -> game; neither -> `None`; no `eboot.bin` -> `None`; executable kinds (ELF magic, both SELF magics, `eboot.bin.esbak` ELF next to a container, garbage).
- `scan`: nested depth (found at depth 3, not at 4), skip list (`sce_module` containing a fake game is ignored), game folder is not descended into, hidden dirs skipped, sorted case-insensitively (`alpha`, `Beta`, `charlie`), `.AnyPS5` skipped.
- `folder_size` with nested files and a cancelled token.
- `GameId` normalisation: `C:\Games\Stray\`, `c:/games/stray`, `\\?\C:\Games\Stray` are equal.
- Watcher (integration, `#[cfg(windows)]` and `ubuntu` via inotify): create a folder in the root -> `LibraryChanged` within 3 s; no event while `busy` is true.

### 9.3 `unlock`

- Names: vectors of 6.4 (`PPSA02100-6d6bf`, `PPSA01234-d356b9`); a hash with leading zeros gives fewer digits.
- `modules_in`: depth rule (3 components ok, 4 not), `sce_sys` skipped, `.PRX` upper-case accepted, `.sprx`, hidden skipped, ordering.
- `module_directories`: `.sprx` adds nothing; `Media/Plugins/a.prx` adds `Media/Plugins`; `sce_module/x.prx`, `prx/x.prx`, `fakelib/x.prx` add nothing; duplicates collapse; `.` first.
- `RelinkProgress.read`: every row of 6.5 with the exact strings the relinker prints (`"Instruction scan       | eboot.bin  |  42%"`, `"PRX conversion         | libc.prx   | 100% (3/7)"`), monotonicity, unknown phase, fewer than three fields, non-numeric percent.
- `LineSplitter`: `\r` rewriting, `\r\n`, partial chunk across reads, 400-line cap, `flush`.
- `carry_over`: moves `_sd`, `_trophy`, keeps `app0` of the new copy, ignores existing names, skips a symlink.
- `link_tree`: replaces an existing destination without modifying the source file (write through hard link regression: assert the source content and mtime unchanged after overlaying a different file); fallback to copy when the link call is forced to fail (`fsx` has a test hook `LinkMode::ForceCopy`); the `.AnyPS5` subtree is skipped.
- Pipeline with `fake-relinker` (integration): a binary built in the workspace that parses the same arguments, writes `\r` progress lines in the real format, creates `eboot.exe` and `app0/sce_module/x.prx.guest.prx`, and can be told by environment to fail (exit 2, message on stderr), to write nothing, or to sleep (for cancel). Tests: success path (record written, `libs` linked, `.partial` and `Work` gone, progress events monotone and coalesced), relinker failure (`Relinker{code: 2, last_line}`), no output (`NoOutput`), cancel while queued, cancel while running (child killed, no leftovers), unlock again (carry-over keeps `_sd/file`), encrypted dump (V3-style) -> error before the relinker starts, two games queued run strictly one after the other (events interleave never).
- Toolkit missing -> `ToolkitMissing` and no events other than `Finished`.

### 9.4 `toolkit`

- Fingerprint vector: files `relinker.exe`=`RELINKER`, `libs/b.prx`=`BBBB`, `libs/a.prx`=`AAA`, `libs/libx.dll`=`D`, `patches/p.json`=`patches!` -> SHA-256 of `RELINKER` + `AAA` + `BBBB` + `D` + `patches!` = `3549fbc1b2f847f7155da1e47723b2381ce3bdba0e382998417fd392cb3335c5`, `fp = 3549fbc1b2f8` (the file-name order is ordinal: `a.prx`, `b.prx`, `libx.dll`).
- `open`: missing relinker, empty libs, missing MinGW DLL -> `MissingRuntime(["libwinpthread-1.dll"])`; adopted layouts (flat, build tree).
- Release JSON parsing from a recorded `v0.1.3` response (a trimmed copy of the API JSON committed as `tests/fixtures/release-v0.1.3.json`, no binaries): asset selection by name, `digest` parsing, missing asset -> `NoRelease`.
- Tag comparison: `0.1.10 > 0.1.9`, `v` prefix, `0.2 == 0.2.0`, garbage -> not newer.
- Zip extraction with a generated archive: path traversal (`../x`), absolute path, drive prefix -> `BadArchive`; normal entries land in `libs/`.
- Download against a local `tiny_http` server (dev-dependency): progress events, digest mismatch -> `Digest`, cancel mid-download leaves no `.partial`.

### 9.5 `settings` and `controls`

- `environment`: every row of 6.6 (table test for `vsync` x `overlay` x `language` x `shader_cache`), clamp of `window_size` never leaks into the environment, `ANYPS5_FULLSCREEN`/`ANYPS5_WINDOW_SIZE`/`APS5_RESOLUTION` never present.
- `InputTuning.lines`: defaults give no lines; each setting alone; clamp edges (0, max, max+1, below min); formats `2.0`, `2.5`, `0.1`, `0.05`, `1.0`, `9.0`, `0.9`; order of lines.
- `Controls.file`: default layout text equals a golden string committed in the test (first line, all 26 actions, `StickModifier = KEY:F24`); a changed key; an action emptied by `assign` writes `= KEY:F24`; tuning lines appended.
- `Controls.assign`: reassigning `W` to `Cross` removes it from `LeftStickUp` (which becomes empty), keeps `MOUSE:` sources, removes entries equal to the default, idempotence, mouse sources kept.
- Preferences: round trip; unknown fields survive; corrupt file -> defaults and `.bad` file; a settings file exported by the macOS launcher (fixture JSON with sorted keys, space before colon as `JSONEncoder` writes it) decodes; the Swift synthesised key set (`fullscreen, windowSize, vsync, overlay` always present) is produced.
- `SharedSettings`: round trip, wrong `format` -> `NotSettingsFile`, extra keys ignored.
- Scancode table: all 100+ entries listed in appendix B are unique names, every name is accepted by the engine's parser (a text list of SDL scancode names is committed as `tests/fixtures/sdl-scancode-names.txt`; the test checks membership), F24 absent, left/right modifiers distinct.

### 9.6 `trophies`

`ucp_builder` writes the big-endian archive from a map of name -> bytes. Tests: bad magic, version 2, count 5000, entry beyond the file; two packages (labels 0 and 1) concatenate; `tropconf.json` with BOM; ids as `"0007"` and as numbers; language selection (`languages` first, then `en-US`, `en-GB`, default, alphabetical); hidden; progressive with `target 1` (no progress) and `target 10, value 14` (clamped); icons present/absent; `percent` (all bronze 1 of 4 -> 25; mixed grades 90+15 of 300+90+30+15 -> 24); `unlocked`: two users, earliest wins, tick below the epoch ignored, malformed line ignored, CRLF lines, file being written (unreadable) skipped; `earned` returns `"0/7"` style ids; `.stats.txt` is not parsed as unlock.

### 9.7 `diagnosis` and `report`

Log samples in `log_samples` are written from the formats in 3.5 (no real logs):

1. `Failed to load module: libSceFoo.prx` + `GetLastError: 126 - ...` -> `MissingModule("libSceFoo.prx")`; with a path `C:\x\libs\libSceFoo.prx` -> basename; the ARM macOS line `Library not loaded: @rpath/libSceFoo.prx` too.
2. `terminate called after throwing an instance of 'std::runtime_error'\n  what():  sceFooBar not implemented` -> `Unsupported("sceFooBar")`; CRLF variant.
3. `[gpu] driver failure: AGC graphics: vkQueueSubmit: Vulkan result -4` -> `GpuLost`; `-2` -> `OutOfMemory`; `VK_ERROR_DEVICE_LOST` text; `std::bad_alloc`.
4. `terminate called ... what():  The host CPU (Intel(R) Core(TM) i3) lacks AVX2, BMI2. The PS5's Zen 2 ...` -> `HostCpu("AVX2, BMI2")`, not `EngineError`.
5. `FATAL: unhandled exception 0xc0000005 on thread 12 'GfxQueue'` + `  write of 0x0000000000000008` -> `Crashed("EXCEPTION_ACCESS_VIOLATION (write of 0x0000000000000008)")`; `FATAL: abort() (SIGABRT) on thread 3` -> `Crashed("abort()")`.
6. Priority: a log with both `Failed to load module` and a `FATAL` block -> `MissingModule`; with `Vulkan result -4` and a `FATAL` -> `GpuLost`; the uncaught exception beats `VK_ERROR_DEVICE_LOST` in the same log (ARM order); the host-CPU text beats the generic uncaught exception.
7. Exit codes alone: `0xC0000005` -> `Crashed("EXCEPTION_ACCESS_VIOLATION")`, `0xC0000135` -> `MissingModule(MinGW...)`, `0`, `1`, `None` -> `None`, `3` -> `Crashed("abort()")`.
8. `classify`: 200-character cut with `…` on a 300-character message counted in characters (use a multi-byte message), trimming, `^(\S+) not implemented` at the start only.
9. `last_session`: no marker (last 64 KiB), two markers (after the last), marker at the end (empty string), marker text itself excluded.
10. `read_last_session` on a 5 MB file reads only the last 4 000 000 bytes.
11. Report text: golden test with fixed inputs (system info injected through `ReportInput`).

### 9.8 `patches`

`PatchSet::load` ignores a macOS-space file; `plan` against a synthetic ELF built in code (one `PT_LOAD`, bytes at a known `va`): applied, already applied (replacement present), skipped with the exact message, address outside any segment (error text `0x<hex> is not in a file-backed segment`), length mismatch (`<name>: original and replacement at <address> differ in length`); resolution fields: instruction prefix check, little-endian value bytes, `parse_resolution` (`1920x1080`, `1920X1080`, `0x5`, `1920`, `ax2`); TOML text golden file for one patch and one resolution field.

### 9.9 `launch` and `process`

- `LaunchRequest` -> `Command` construction is a pure function (`build_command(&req) -> CommandSpec { program, args, cwd, env }`), tested without spawning.
- Spawn tests use a tiny helper binary in the workspace (`crates/fake-relinker` doubles as `fake-game` by mode): prints lines to stdout and stderr interleaved, exits with a chosen code, opens a window-less lifetime, spawns a child that outlives it. Asserts: merged output order preserved, log file gets the start marker then the output, ring tail capped at 400 KiB, exit code propagated, `kill` terminates the child too (job), `Exited` arrives after the pump drained (log complete when `Diagnosed` is computed).
- `has_window` is tested with a hidden window created by the test itself via `windows` (`CreateWindowExW`, `#[cfg(windows)]`, runs on CI only), asserting the size/visibility/owner rules.

### 9.10 Model tests (`anyps5-launcher`)

`AppModel` is UI-free: feed it a scripted sequence of `CoreEvent`s and actions and assert `state(of:)` and side effects (`Vec<Command>` it returns instead of spawning, behind a `Backend` trait with a fake): the table of 8.3 row by row, the priority order of 8.2, queue ordering, guards (`play` while starting), settings change reloading trophies, `cancel_start`, exit before window sets `started_without_window` and the toast, import settings clears an invalid resolution.

### 9.11 CI

`windows-latest`: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. `ubuntu-latest`: `cargo test -p anyps5-core` (the portable parts; `win` modules are `cfg(windows)`). Optional manual job `oracle` (9.1). No job downloads or runs AnyPS5 binaries or games. A nightly job may run `anyps5-tool scan` on the synthetic dump builder output only.

## 10. Build and packaging (build_launcher_app.sh, install.sh)

- `cargo build --release -p anyps5-launcher` (LTO thin, strip, panic abort per workspace). Resources compiled in: fonts (a freely redistributable face, not SF Pro), icons, sounds. App manifest (via `winres`): `longPathAware`, per-monitor DPI v2, `asInvoker`, Windows 10+ compatibility GUID.
- Artifacts: NSIS installer (per user, `%LOCALAPPDATA%\Programs\AnyPlay Launcher`, no admin rights) and a portable zip (`AnyPlay-Launcher.exe` + `patches\`). The portable mode is on when a file `portable.flag` sits beside the exe: then `AppPaths` roots become `<exe dir>\data\{roaming,local}`.
- ARM's `install.sh` has no equivalent; the installer replaces Rosetta installation, quarantine removal and `/Applications` copying.
- Version string: `git describe --tags --always` at build (as `build_launcher_app.sh`), shown in the report as `Launcher:`.
- Licence files shipped in the installer: GPL-2.0 text, the launcher's credits (utkuhalis/AnyPS5-ARM, boykopovar/AnyPS5), third-party licences (`cargo about`).

## 11. Assumptions to verify and open decisions

| Id | Assumption / question | Who can settle it |
|---|---|---|
| A1 | ELF virtual address of a macOS patch = Mach-O address - `0x140010000` (lowest ELF load page of vaddr 0 placed at `0x140010000` by default, UP USAGE.md). Needed to derive the Windows patch file for Stray. Not needed for v1 (no patch ships) | a machine that may run `relinker --verify-patches` on the real dump |
| A2 | `relinker --windows --module-dir .` with a dump that has `sce_module/` behaves as on macOS (module folder detection, `.guest.prx` layout) | first CI/real run |
| A3 | `relinker.exe` writes progress to stderr with `\r` rewrites when its stderr is a pipe (macOS build did) | first real run; the splitter handles both `\r` and `\n` |
| A4 | Libraries and MinGW DLLs are found through `$ORIGIN/libs`; the release zip also lists them only under `libs/`, but PR3786 copies the three MinGW DLLs next to the executable as well. If the loader does not find them via `libs\`, copy them next to `eboot.exe` (harmless) | first real run |
| A5 | Staging `sce_sys/icon0.png` beside `eboot.bin` embeds the title icon in the PE (`Conversion.cpp:132` takes `input.Icon`; where it is loaded from is not verified) | read `core/relinker/relinker/src` input loading |
| A6 | Keeping the dump's original modules in `app0/sce_module` next to the converted `*.guest.prx` is correct on Windows as on macOS (PR3786 skips them) | first real run; harmless if the engine ignores them |
| A7 | Moving/resizing the game's window with `SetWindowPos` is respected by the engine's swapchain (UP: "the display window can be resized") | real run |
| A8 | A synthesised F11 toggles fullscreen (`ToggleFullscreen` is a keyboard-only action) | real run; fallback: document F11 only, hide the setting |
| A9 | Exact quoting of byte strings in relinker patch TOML (`expected = "0f84..."` vs bare hex) | tests in `core/relinker/relinker/tests` |
| A10 | Trophy banner window shows above an exclusive-fullscreen Vulkan window | real run; fallback: in-game text is not possible, show after exit |
| A11 | `ANYPS5_SHADER_CACHE_DIR` outside the run dir is accepted for paths with non-ASCII characters | UP INPUT_MAPPING.md says paths may contain `ü`/`日本` for the input file only |
| Q1 | Ask upstream for `ANYPS5_WINDOW_SIZE`/`ANYPS5_FULLSCREEN`-like variables or a `--set` for patches to remove D9/D10 | upstream PR |
| Q2 | Per-title input profiles (PR7499, `anyps5-input.<TITLEID>.ini` beside the shared file) are not in `origin/main`. The launcher writes one complete file per game and sets `ANYPS5_INPUT_CONFIG`, so it does not depend on them. When PR7499 merges, nothing changes unless the shared file sits beside the executable (then a profile would override the launcher's file for that title: the run dir must not contain `anyps5-input.ini`, so the launcher must never create one there) | track PR7499 |
| Q3 | `.AnyPS5` store location when the games folder is on a network share or exFAT (no hard links): copying doubles space. Offer a setting "store folder" on another volume? (defeats linking) | decide with first user feedback |

First real run (22.10., release v0.1.3 toolkit downloaded by `download_and_install`, Minecraft Dungeons II dump PPSA16064, unlocked with `run_unlock`, started with `Launcher::start`): A2 holds (`--module-dir .` plus the module folders works; 58 modules converted), A3 holds (the relinker prints its progress lines through the pipe and they parse), A4 holds (`libs\` with the MinGW DLLs was enough, nothing next to `eboot.exe`), A6 holds (the original modules stay in `app0`), the first window was found and reported. Not checked: A5 (icon), A7 and A8 (window placement and F11 were posted but the result was not looked at), A10, A11 (the shader cache folder had an ASCII path). New finding: this dump holds a second copy of its modules under `decrypted/`, and the relinker refuses two modules with one file name ("Conflicting guest output filename"); `module_directories` therefore skips a folder whose file names were already taken. The game was started although the lab lock's 13 GiB RAM gate was not met (about 9 GiB were free); it ran for about ten seconds and was killed with no crash.

## Appendix A: SELF test vectors

Generator (`tests/support/self_builder`), shown in Python for exactness; the Rust helper writes the same bytes. Little-endian.

```python
def phdr(t, fl, off, va, fs, ms, al): return struct.pack("<IIQQQQQQ", t, fl, off, va, va, fs, ms, al)   # paddr = vaddr
def elf_header(phoff, phnum, shoff=0x1234, shnum=5, shstr=4):
    h = bytearray(64); h[0:16] = b"\x7fELF\x02\x01\x01\x00" + b"\0"*8
    struct.pack_into("<HHIQQQIHHHHHH", h, 16, 2, 0x3E, 1, 0x401000, phoff, shoff, 0, 64, 56, phnum, 64, shnum, shstr)
    return bytes(h)
def record(flags, seg, size): return struct.pack("<QQQQ", flags, seg, size, size)
def build(inner_at, phoff, phs, records, blobs, total):
    d = bytearray(total); struct.pack_into("<I", d, 0, 0x1D3D154F)
    for i, r in enumerate(records): d[0x40+0x20*i : 0x40+0x20*(i+1)] = r
    d[inner_at:inner_at+64] = elf_header(phoff, len(phs))
    for i, p in enumerate(phs): a = inner_at + phoff + 56*i; d[a:a+56] = p
    for off, b in blobs: d[off:off+len(b)] = b
    return bytes(d)
def pattern(n, seed): return bytes((seed*7 + i*13 + (i >> 3)) % 61 for i in range(n))
```

| Id | Construction | Input (len, SHA-256) | Expected |
|---|---|---|---|
| V1 | `build(0x100, 0x40, [phdr(1,5,0x400,0x400000,0x100,0x100,0x1000)], [record(0x800|(0<<20), 0x1000, 0x100)], [(0x1000, pattern(0x100,1))], 0x1100)` | 4352, `d53440efc25dc2df09a7e2549506e3c9cb50b354b607cf0dbbb31c8c70a0c9d0` | 4352 bytes, SHA-256 `1384a1dae8b587352a342472c4b37f1cac70d76e7454b61414cb526ba94bdffd`; `e_shoff = 0`, `e_shnum = e_shstrndx = 0`; one header `(1, 5, 0x1000, 0x400000, 0x400000, 0x100, 0x100, 0x1000)` at `0x40` |
| V2 | phs = `[phdr(1,5,0x400,0x400000,0x200,0x200,0x4000), phdr(1,6,0x4400,0x404000,0x80,0x300,0x4000), phdr(4,4,0x4500,0,0x10,0x10,8), phdr(0x6474e551,6,0,0,0,0,0x10)]`; records `[record(0x800, 0x1000, 0x200), record(0, 0x1400, 0x80)]` (the second has no segment flag: matched by size); blobs `[(0x1000, pattern(0x200,2)), (0x1400, pattern(0x80,3))]`; total `0x1500`; inner at `0x100`, `phoff 0x40` | 5376, `ae7efa53d323d80176a0597aa1779fbaf0be1efb11317085ba241950cdaa6c62` | 32896 bytes, SHA-256 `29ee0d7e0b97848a55b038e8a4fd5b410403ea4dcd3a30d5ff8dc92d19a1e170`; headers: `(1,5,0x4000,0x400000,0x400000,0x200,0x200,0x4000)`, `(1,6,0x8000,0x404000,0x404000,0x80,0x300,0x4000)`, `(4,4,0x1500,0,0,0,0,8)` (PT_NOTE: size and memory cut to 0, offset = input length), `(0x6474e551,6,0,0,0,0,0,0x10)` |
| V3 | as V1 but the blob is `bytes(range(256))` and the record flags `0x800` | 4352, `0ec58b3db5cf2bd054be594f824036fdd82bd33ff75592b1dad58d42b09f58ad` | error `every load segment looks encrypted: segment 0 at 0x1000 (8.00 bits per byte)` |

All three were run through `tools/unwrap_self.py` of `cb20fd8` (Python 3.14) on 2026-10-22; V3 printed the error above. Entropy: 256 distinct bytes = 8.0 bits; `[0]*100 + [1]*28` = 0.7579.

## Appendix B: scan code to SDL name (D15)

Windows set-1 scan codes (`MapVirtualKeyW(vk, MAPVK_VK_TO_VSC_EX)`: high byte `0xE0` for extended keys). Names are `SDL_GetScancodeName`; the engine resolves `KEY:<name>` with `SDL_GetScancodeFromName`. The position on a US keyboard decides, as SDL reads scancodes (Controls.keyName comment).

```
0x01 Escape  0x02..0x0B 1 2 3 4 5 6 7 8 9 0  0x0C -  0x0D =  0x0E Backspace  0x0F Tab
0x10..0x19 Q W E R T Y U I O P  0x1A [  0x1B ]  0x1C Return  0x1D Left Ctrl
0x1E..0x26 A S D F G H J K L  0x27 ;  0x28 '  0x29 `  0x2A Left Shift  0x2B \
0x2C..0x32 Z X C V B N M  0x33 ,  0x34 .  0x35 /  0x36 Right Shift  0x37 Keypad *  0x38 Left Alt
0x39 Space  0x3A CapsLock  0x3B..0x44 F1..F10  0x45 Numlock  0x46 ScrollLock
0x47 Keypad 7  0x48 Keypad 8  0x49 Keypad 9  0x4A Keypad -  0x4B Keypad 4  0x4C Keypad 5  0x4D Keypad 6  0x4E Keypad +
0x4F Keypad 1  0x50 Keypad 2  0x51 Keypad 3  0x52 Keypad 0  0x53 Keypad .  0x57 F11  0x58 F12
0xE01C Keypad Enter  0xE01D Right Ctrl  0xE035 Keypad /  0xE037 PrintScreen  0xE038 Right Alt
0xE047 Home  0xE048 Up  0xE049 PageUp  0xE04B Left  0xE04D Right  0xE04F End  0xE050 Down  0xE051 PageDown
0xE052 Insert  0xE053 Delete  0xE05B Left GUI  0xE05C Right GUI  0xE05D Application
```

The ARM table (macOS key codes) contains the same set except `Numlock`, `ScrollLock`, `PrintScreen`, `Insert`, `Application` and F13..F24, which no Mac keyboard offers; they are added. `Keypad Clear` (ARM has none) is not added. Names with no entry (media keys, the ISO key `0x56`, `F13`-`F24`) cannot be assigned. The three ARM sentinels (`Escape` cancels, F24 unbound) are as in 6.7.
