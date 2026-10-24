# AnyPlay Launcher

The home of your games on PC. The launcher finds the games you dumped from your own console, unlocks them for [AnyPS5](https://github.com/boykopovar/AnyPS5) and starts them from a console-style home screen, on Windows and Linux. It is written in Rust with egui and keeps all its logic apart from the interface.

It is part of **[AnyPlay](https://github.com/anyplay-gg)**, which builds everything around AnyPS5 that players need:

| | |
|---|---|
| **Launcher** (this repository) | Your library: unlock, play, controls, trophies and saves |
| **[Network](https://github.com/anyplay-gg/network)** | Online play with friends over the internet, in development: the net service this launcher starts, the bridge protocol and the specifications |
| **[Cloud](https://github.com/anyplay-gg/cloud)** | The services behind the network, run by AnyPlay or by you. No public cloud runs yet |

News, help and other players: the [AnyPlay Discord](https://discord.gg/nSHbHBRdS).

<p align="center">
  <img src="docs/media/preview.webp" width="800" alt="The launcher fades in, then moves between Minecraft Dungeons II, Sword of the Sea and Hollow Knight: Silksong">
</p>

<p align="center"><a href="https://github.com/anyplay-gg/launcher/releases/download/v0.1.0/AnyPS5-Launcher-Update.mp4"><b>Watch the update video with sound</b></a></p>

| | |
|---|---|
| ![The home screen with Minecraft Dungeons II selected](docs/media/home.jpg) | ![The controls of Minecraft Dungeons II, read from the game's own files](docs/media/controls-from-the-game.jpg) |
| ![Sword of the Sea selected on the home screen](docs/media/sword-of-the-sea.jpg) | ![Hollow Knight: Silksong, not unlocked yet](docs/media/silksong.jpg) |

<sub>Game artwork belongs to its publishers and is shown only to illustrate the launcher.</sub>

## What it does

| | |
|---|---|
| **Home screen** | Each game shows its own artwork and music. Soft system sounds, fullscreen like a console (F11 for a window), navigation by keyboard, mouse or controller, themes and a choice of language. |
| **Library** | Finds dumps in one games folder, a few levels deep, and watches the folder for changes. It reads each game's title, ID, artwork and media from the dump and never changes the dump. |
| **Unlock and play** | Turns a dump into an unlocked copy that AnyPS5 can run. A queue unlocks one game at a time. Play starts the game and shows a splash until its first window opens. |
| **Toolkit** | Downloads the AnyPS5 toolkit on the first start, checks it and offers new releases with their notes. A game unlocked with an older toolkit is marked *outdated* and can be unlocked again. |
| **Controls** | Reads a game's controls from its own files (Unreal Engine, including Enhanced Input), so every button shows what it does. One choice gives you the game's own PC keys. Ready-made mouse and keyboard layouts, a gallery of community layouts and tuning for sticks and mouse are there too. |
| **Trophies, saves, playtime** | Trophies are read from the dump and pop up while you play. Save data is backed up as a zip before anything risky, and the home screen shows how long you played. |
| **Per-game settings** | Window, sync, overlay, language, controls, tuning and shader cache for each game. A catalogue of recommended settings, each entry with its evidence, is applied only when you click. |
| **When something fails** | Warns early if the CPU lacks the extensions AnyPS5 needs, reads from the log why a game stopped, writes a report you can attach to a bug report and opens a pre-filled issue for AnyPS5. |
| **Online with friends** | Sign in to the AnyPlay network, see what your friends play and join their game. In development; see [Online play](#online-play). |
| **Links and Discord** | `anyps5://game/<title ID>` links open a game from a website or a message, and folders get "Open with AnyPlay Launcher" ([docs/DESIGN.md](docs/DESIGN.md) 9.6). If Discord runs, it shows "Playing" with the game's name. |

## How it works

The launcher is a Cargo workspace of three crates:

| Crate | What it holds |
|---|---|
| `crates/anyps5-core` | All logic, no interface: library scan, unlock pipeline, toolkit, launch and diagnosis, controls, saves, trophies, settings, self-update and the online state. The launcher depends on it; it never depends on the launcher. |
| `crates/anyps5-launcher` | The application `anyplay-launcher`: egui and eframe on wgpu, the screens, the sounds, the window handling and the glue between them. |
| `crates/fake-relinker` | A stand-in for AnyPS5's relinker, so tests run without the toolkit or a game. |

### From a dump to a game

1. **Scan.** The launcher looks in the games folder for dumps: a decrypted folder with `eboot.bin`, its modules and `sce_sys` metadata.
2. **Unwrap.** Every SELF container of the executable and the modules becomes a plain ELF. Nothing is decrypted, so an encrypted dump cannot be used.
3. **Relink.** The toolkit's relinker rewrites the ELF files so they run natively. Per-title byte patches, where there are any, are passed to it.
4. **Assemble.** The result goes into an unlocked copy in the hidden `.AnyPS5` store inside the games folder. The toolkit's system libraries are added, and the dump's files are hard-linked in (copied where a link is not possible). A small unlock record says which dump and which toolkit made the copy. The dump itself is never written to.
5. **Play.** The launcher starts the unlocked copy's executable with an environment and an input file built from the game's settings. It keeps the game's output in a log, notices the first window and reports how the process ended and why. Saves and trophies go to the unlocked copy, and an unlock carries them across.

The toolkit is a release of [boykopovar/AnyPS5](https://github.com/boykopovar/AnyPS5) unless you choose another repository. A table in `compat.rs` records what each release can do. The launcher leaves out a setting that an older toolkit would not understand and never refuses a toolkit for it.

### The net service

Online play runs in a program of its own, `anyplay-net` from [anyplay-gg/network](https://github.com/anyplay-gg/network). The launcher starts it as a child process and talks to it in JSON lines over its standard streams (the control protocol). The launcher shows; the net service acts.

- Nothing goes online until you sign in. The service starts when you open the Online area, where it stays local until you ask for a code, or together with the launcher once you have signed in on this PC.
- A release build carries the service inside the launcher and writes it to `Network\bin` once per version, so every download and every self-update brings the service that matches it. A build without it looks for `anyplay-net` next to the launcher, or where `ANYPLAY_NET` points.
- When you press Play while signed in, the launcher asks the service for the variables of the network bridge and passes them to the game. It waits at most 3 seconds and then starts the game offline.

### Self-update

At start, in the background, the launcher asks GitHub for the latest release of this repository. If it is newer, the launcher downloads the portable exe and compares it with its line in the release's `SHA256SUMS.txt`. A missing or wrong checksum refuses the update and leaves nothing behind. Windows can rename a running exe but not overwrite it, so the swap moves the running exe to `<name>.old` and puts the new one in its place; if the second step fails, the old one moves back. The next start removes the `.old` file. The swap happens when you close the launcher. It works for the installed exe and for the portable exe in any folder you can write to.

The checksum list comes from the same release as the exe. That guards against a damaged or substituted download, not against a compromised release. The launcher can therefore trust a signed manifest instead ([docs/UPDATES.md](docs/UPDATES.md)). From the first release whose launcher carries a key in `data/update-keys.txt`, an update is installed only if it is signed by one of those keys, has not expired, is newer than the version you run, and matches the signed hash and size. If anything is wrong, the update is skipped and the installed launcher stays as it is. The key list is still empty, so releases use the checksum path for now. Releases are not code-signed yet. Self-update exists on Windows only.

## Download

Get the latest version from the [releases page](https://github.com/anyplay-gg/launcher/releases/latest). The files below carry the launcher's current name. The newest release at the time of writing, v0.1.3, was made before the rename and calls them `AnyPS5-Launcher-<version>-…`; the online area described here is on `main` and not in v0.1.3 yet. Each release also carries `AnyPS5-Launcher-<version>-x64.exe`, the same portable exe under the old name, so copies of v0.1.2 and v0.1.3 can update themselves; you do not need it.

**Windows** (64-bit Windows 10 or 11, nothing else to install first):

- **`AnyPlay-Launcher-<version>-Setup.exe`** installs the launcher for your user account, without administrator rights. It adds a Start menu entry, an optional desktop shortcut and an uninstaller.
- **`AnyPlay-Launcher-<version>-x64.exe`** is the same launcher as a single file. Put it anywhere and run it.

The files are not code-signed yet, so SmartScreen may say "Windows protected your PC". Click **More info**, then **Run anyway**; it asks once per file. Each release lists SHA-256 checksums in `SHA256SUMS.txt`: run `Get-FileHash <file>` in PowerShell and compare.

**Linux** (64-bit x86-64 with glibc 2.35 or newer, such as Ubuntu 22.04, Debian 12 or Fedora 36, a Vulkan driver, and the ALSA and udev libraries every desktop has):

- **`AnyPlay-Launcher-<version>-x86_64.AppImage`**: make it executable and run it. Without FUSE 2, add `--appimage-extract-and-run`.

  ```sh
  chmod +x AnyPlay-Launcher-*-x86_64.AppImage
  ./AnyPlay-Launcher-*-x86_64.AppImage
  ```

- **`AnyPlay-Launcher-<version>-linux-x86_64.tar.gz`** holds the executable, a `.desktop` file and the icon. Unpack it anywhere; copy the `.desktop` file to `~/.local/share/applications` and the icon to `~/.local/share/icons` for a menu entry. Coming from AnyPS5 Launcher: replace `anyps5-launcher.desktop` and `anyps5-launcher.png` with the new files and pin the launcher again.

The Linux build is new, and it does not update itself yet: download a new version from the releases page. What games need from your system is up to AnyPS5:

- a CPU with AVX2, FMA, F16C, BMI1, BMI2, LZCNT and MOVBE (Zen 2, Intel Haswell or newer);
- a Vulkan 1.1 GPU. With Mesa drivers (AMD, Intel) games import memory through `/dev/udmabuf`: join the `kvm` group (`sudo usermod -aG kvm $USER`) and log in again. The launcher tells you when a game fails for this reason;
- on Wayland a game's window cannot be found or placed from outside, so the launcher counts a game as shown after a few seconds, and fullscreen and size follow the game's own settings (F11 in the game).

## First start

1. Choose a language and a look.
2. Pick your games folder, the one that holds your decrypted dumps. The launcher lists what it finds there.
3. The launcher downloads the AnyPS5 toolkit and tells you when it is ready. Without internet, open Settings later and choose *Download the Toolkit*.
4. Select a game, unlock it and press Play.

On Windows the launcher looks for a new release when it starts, downloads and checks it in the background, and installs it when you close the launcher (or at once with *Restart Now* in Settings, About). Turn *Check for Updates* off in Settings to stop that.

Settings live in `%APPDATA%\AnyPS5` and the toolkit, logs and caches in `%LOCALAPPDATA%\AnyPS5` (Linux: `~/.config/anyps5` and `~/.local/share/anyps5`, or where `XDG_CONFIG_HOME` and `XDG_DATA_HOME` point). Uninstalling leaves them in place. The folders keep the old name on purpose, so an upgrade finds your data.

## Online play

The online part is in development. This is what works today and what does not.

**What works.** Select your profile at the top of the home screen to open **Online**. Sign in with your e-mail address: there is no password, each PC gets a code by e-mail. Then add friends by their name, see what they play, accept requests and invitations, and join a friend's game when it is open to friends. Your trophies go to the cloud as records: number, grade and time of each, while names and pictures stay on your PC. A friend's page shows their trophy count by grade, and *Who Sees Your Trophies* on the Account page decides who sees yours (friends by default). *Hide Your IP Address* keeps every tunnel on a relay, so other players never learn your address. The launcher, the net service and the cloud are tested together, including two net services with fake games that exchange packets through a tunnel, directly and through a relay (the end-to-end tests in [anyplay-gg/cloud](https://github.com/anyplay-gg/cloud)).

**What does not work yet.**

- No public cloud runs. Release builds name none until one does, so you enter the address of a cloud: under **Online → Cloud** you can use one of your own or of friends; [anyplay-gg/cloud](https://github.com/anyplay-gg/cloud) explains how to host it with `docker compose up`.
- No real game plays online yet. The bridge that connects a game to the network lives in an AnyPS5 build, and it is still under review. Until a toolkit carries it, games start as before, offline. The [known gaps](https://github.com/anyplay-gg/network/blob/main/docs/gaps.md) list what is missing.
- On Linux, starting a game with the bridge is untested.

**What to expect later.** A game plays online only with a build that carries the bridge, and only when its online part uses the platform's own services. Games whose online play runs on a publisher's live servers stay offline on the PlayStation side: the network never stands in for a service that still runs. The [network's design](https://github.com/anyplay-gg/network/blob/main/docs/DESIGN.md) has the details and the roadmap.

**Privacy.**

- The launcher contacts the network only after you sign in. Until then, nothing goes online.
- *Record Network Calls* (Settings, Advanced) makes a game write what it asks the network for into the Logs folder. It is off by default and nothing is uploaded.
- The net service writes its log to `Logs\network.log`.

## Building

You need a stable Rust toolchain, 1.88 or newer ([rustup](https://rustup.rs)), and a clone of [anyplay-gg/network](https://github.com/anyplay-gg/network) next to this repository for the online crates.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

The executable is `target/release/anyplay-launcher.exe` (Linux: `target/release/anyplay-launcher`). For online play it looks for the net service `anyplay-net` next to itself, or where `ANYPLAY_NET` points; build it in the network repository with `cargo build --release -p anyplay-net`. Setting `ANYPLAY_NET_EMBED` to a built service at build time carries it inside the launcher, as releases do. A build names AnyPlay's own cloud with `ANYPLAY_DEFAULT_CLOUD`; without it the player enters a cloud.

On Linux, install the development packages first (Debian and Ubuntu names: `pkg-config libasound2-dev libudev-dev libxkbcommon-dev libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev`). `tools/linux/package.sh <version> <binary> <folder>` makes the AppImage and the tarball.

Two toolchains are used on Windows:

- **MSVC** (`x86_64-pc-windows-msvc`, needs the Visual Studio Build Tools) is what CI builds and releases ship. `.cargo/config.toml` links the C runtime statically, so the exe needs no Visual C++ redistributable.
- **GNU** (`x86_64-pc-windows-gnu`) works for local development without Visual Studio. If the linker fails, build with `RUSTFLAGS="-C link-self-contained=yes -C linker=rust-lld -C linker-flavor=ld.lld"` and make `libshlwapi.a` (from any MinGW-w64) available through an extra `-L native=<dir>`. Do not build in a path that contains an apostrophe: `windres` cannot embed the icon there.

Every pull request runs the first three commands on Windows and on Linux, and builds the release exe.

### Releases

A release is made by pushing a tag `v<version>` that matches the version in `Cargo.toml`. The [release workflow](.github/workflows/release.yml) builds the net service from [anyplay-gg/network](https://github.com/anyplay-gg/network) and embeds it, then builds the portable exe and the installer (`tools/installer/anyplay-launcher.iss`, Inno Setup), the Linux AppImage and tarball, and a licence file for every third-party crate. It publishes all of them with their checksums in `SHA256SUMS.txt`. Started by hand it only builds them and keeps them as workflow artifacts.

### Where to read more

- [CONTEXT.md](CONTEXT.md): the words the launcher and its code use.
- [docs/PORT.md](docs/PORT.md): the logic, step by step.
- [docs/DESIGN.md](docs/DESIGN.md): the interface, from tokens and motion to every screen.
- [docs/supply-chain.md](docs/supply-chain.md): what is pinned, how updates arrive and how to verify a release.
- [data/README.md](data/README.md): how to add a recommended setting. Community key layouts are added under `community/layouts`.
- [CONTRIBUTING.md](CONTRIBUTING.md): how to contribute.

## Credits and licence

Licensed under GPL-2.0 (see [LICENSE](LICENSE)).

The launcher began as a port of the macOS launcher by [utkuhalis](https://github.com/utkuhalis) in [AnyPS5-ARM](https://github.com/utkuhalis/AnyPS5-ARM) (GPL-2.0), whose design and logic it builds on. Since then it has its own implementation, Windows and Linux integration, the online area and much more.

Bundled fonts and icons (licence texts in `crates/anyps5-launcher/assets/fonts/`): [Inter](https://rsms.me/inter/) (SIL OFL 1.1), [JetBrains Mono](https://www.jetbrains.com/lp/mono/) (SIL OFL 1.1) and [Phosphor Icons](https://phosphoricons.com) (MIT).

## Legal note

- You need your own decrypted game dumps. No game files, keys or firmware are included in this repository, and none will be.
- AnyPS5 is not an emulator: it ports executables so they run natively. The launcher manages and starts them.
- AnyPlay is a community project. It is not affiliated with Sony Interactive Entertainment or with the AnyPS5 maintainers. "PlayStation" and "PS5" are trademarks of Sony Interactive Entertainment.
