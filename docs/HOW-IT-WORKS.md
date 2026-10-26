# How the launcher works

The launcher is a Cargo workspace of six crates. All logic lives apart from the interface.

| Crate | What it holds |
|---|---|
| `crates/anyps5-core` | All logic, no interface: library scan, unlock pipeline, toolkit, launch and diagnosis, controls, saves, trophies, settings, self-update and the online state. The launcher depends on it; it never depends on the launcher. |
| `crates/anyps5-launcher` | The application `anyplay-launcher`: egui and eframe on wgpu, the screens, the sounds, the window handling and the glue between them. |
| `crates/atrac9` | ATRAC9 game-music decoding: builds LibAtrac9 from source and exposes it through a safe Rust wrapper. |
| `crates/fake-relinker` | A stand-in for AnyPS5's relinker, so tests run without the toolkit or a game. |
| `crates/fake-phone` | Test helper standing in for the AnyPS5 Remote phone app and a game on the pad bridge; never shipped. |
| `crates/update-manifest` | The signed self-update manifest format, its verification logic and the `update-sign` signing tool. |

The words the code uses are in [CONTEXT.md](../CONTEXT.md), the logic step by step in [PORT.md](PORT.md) and the interface in [DESIGN.md](DESIGN.md).

## From a dump to a game

1. **Scan.** The launcher looks in the games folder for dumps: a decrypted folder with `eboot.bin`, its modules and `sce_sys` metadata.
2. **Unwrap.** Every SELF container of the executable and the modules becomes a plain ELF. Nothing is decrypted, so an encrypted dump cannot be used.
3. **Relink.** The toolkit's relinker rewrites the ELF files so they run natively. Per-title byte patches, where there are any, are passed to it.
4. **Assemble.** The result goes into an unlocked copy in the hidden `.AnyPS5` store inside the games folder. The toolkit's system libraries are added, and the dump's files are hard-linked in (copied where a link is not possible). A small unlock record says which dump and which toolkit made the copy. The dump itself is never written to.
5. **Play.** The launcher starts the unlocked copy's executable with an environment and an input file built from the game's settings. It keeps the game's output in a log, notices the first window and reports how the process ended and why. Saves and trophies go to the unlocked copy, and an unlock carries them across.

The toolkit is a release of [boykopovar/AnyPS5](https://github.com/boykopovar/AnyPS5) unless you choose another repository. A table in `compat.rs` records what each release can do. The launcher leaves out a setting that an older toolkit would not understand and never refuses a toolkit for it.

## The net service

Online play runs in a program of its own, `anyplay-net` from [anyplaynet/network](https://github.com/anyplaynet/network). The launcher starts it as a child process and talks to it in JSON lines over its standard streams (the control protocol). The launcher shows; the net service acts.

- Nothing goes online until you sign in. The service starts when you open the Online area, where it stays local until you ask for a code, or together with the launcher once you have signed in on this PC.
- A release build carries the service inside the launcher and writes it to `Network\bin` once per version, so every download and every self-update brings the service that matches it. A build without it looks for `anyplay-net` next to the launcher, or where `ANYPLAY_NET` points.
- When you press Play while signed in, the launcher asks the service for the variables of the network bridge and passes them to the game. It waits at most 3 seconds and then starts the game offline.

## Self-update

The launcher asks GitHub for the latest release of this repository at start, then every 6 hours. An update is installed only when the release carries a signed manifest (`latest.json` and `latest.json.sig`) that a key in `data/update-keys.txt` signed (the key "anyplay 2026" since v0.2.0). The manifest has to be unexpired and newer than the running version, and the download has to match its hash and size. If anything is wrong, the update is skipped and the installed launcher stays as it is. The rules, the key ceremony and how a version is retired are in [UPDATES.md](UPDATES.md).

What gets replaced depends on the platform:

- **Windows:** the running exe is renamed to `<name>.old` and the new one takes its place (Windows can rename a running exe but not overwrite it). If the second step fails, the old one moves back. The swap happens when you close the launcher, or at once with *Restart Now* in Settings, About.
- **Linux AppImage:** the new image replaces the old one in a single atomic rename.
- **macOS:** the whole app bundle is exchanged atomically.
- **Linux tar.gz:** the launcher cannot replace itself. About shows that a new version is out and links to the download.
- **Flatpak and AUR:** a marker file written by the package says which package manager owns the files. The launcher then never replaces them; About shows the new version and names the package manager that updates it.

Launchers up to v0.1.3 check only the release's `SHA256SUMS.txt` and take v0.2.0 that way once. Releases are not code-signed. More on what is pinned and how to verify a release: [supply-chain.md](supply-chain.md).
