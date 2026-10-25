# Building and releasing

You need Rust 1.95 or newer ([rustup](https://rustup.rs)), because the egui/eframe 0.37 dependencies require it, and a clone of [anyplaynet/network](https://github.com/anyplaynet/network) next to this repository for the online crates, checked out at the commit in [`network.ref`](../network.ref). The repository does not override your selected toolchain; CI uses stable, and the release workflow selects its own compiler.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

Every pull request runs the first three commands on Windows and on Linux, and builds the release exe.

The executable is `target/release/anyplay-launcher.exe` (Linux: `target/release/anyplay-launcher`).

## Which network revision

The launcher builds against one network commit everywhere: the full SHA in [`network.ref`](../network.ref). CI, the advisory check, the release and the nightly jobs all read that file, so a merge on the network's `main` cannot turn this repository red by itself. Run `git -C ../network checkout $(cat network.ref)` to build as CI does. A pull request that needs new network code (a changed event or command, a new crate) changes `network.ref` in the same pull request, so CI proves the pair. The workflow `network-drift` (nightly and on demand) builds `main` against the network's `main` and goes red when that no longer works; it reports and blocks nothing.

## The net service

For online play the launcher looks for the net service `anyplay-net` next to itself, or where `ANYPLAY_NET` points. Build it in the network repository with `cargo build --release -p anyplay-net`. Setting `ANYPLAY_NET_EMBED` to a built service at build time carries it inside the launcher, as releases do. A build names AnyPlay's own cloud with `ANYPLAY_DEFAULT_CLOUD`; without it the player enters a cloud.

## Windows

Two toolchains are used:

- **MSVC** (`x86_64-pc-windows-msvc`, needs the Visual Studio Build Tools) is what CI builds and releases ship. `.cargo/config.toml` links the C runtime statically, so the exe needs no Visual C++ redistributable.
- **GNU** (`x86_64-pc-windows-gnu`) works for local development without Visual Studio. If the linker fails, build with `RUSTFLAGS="-C link-self-contained=yes -C linker=rust-lld -C linker-flavor=ld.lld"` and make `libshlwapi.a` (from any MinGW-w64) available through an extra `-L native=<dir>`. Do not build in a path that contains an apostrophe: `windres` cannot embed the icon there.

## Linux

Install the development packages first (Debian and Ubuntu names: `pkg-config libasound2-dev libudev-dev libxkbcommon-dev libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev`). `tools/linux/package.sh <version> <binary> <folder>` makes the AppImage and the tarball.

## macOS

Build with the commands above and run `target/release/anyplay-launcher`. The interface uses wgpu's Metal backend. Games use the separate [AnyPS5-ARM toolkit](https://github.com/utkuhalis/AnyPS5-ARM), with Vulkan through MoltenVK; Apple silicon needs Rosetta 2 for the x86-64 runtime. The launcher reports a missing Rosetta installation and does not install it automatically.

On macOS the toolkit picker defaults to `utkuhalis/AnyPS5-ARM`. It downloads `AnyPS5-toolkit-macos-<version>.zip`, checks the release's SHA-256 and every file in its manifest, and installs the relinker, libraries, Vulkan runtime and patch catalogue together. A local toolkit must contain `relinker`, `libs/*.prx`, and `vulkan/{libvulkan.1.dylib,libMoltenVK.dylib,MoltenVK_icd.json}`. Linux executables are refused. Windows and Linux keep their existing upstream repository and separate relinker and library downloads.

Games are converted to Mach-O, with `--to-rosetta` on Apple silicon, and kept in the library's `.AnyPS5/Games` folder. APFS clones isolate the unlocked copy from the original dump; other filesystems fall back to copying. The launcher supplies the bundled MoltenVK driver manifest, applies the toolkit's checked Mach-O patches, watches real game windows and captures process output. Game compatibility is determined by the selected runtime; a successful conversion does not guarantee that every game plays.

Preferences and launcher data live in `~/Library/Application Support/AnyPlay Launcher`. On first start, an existing XDG preference file is read as a fallback, preserving the profile, library and game settings. The old non-Windows default toolkit repository is changed to the macOS source; custom repositories are retained. The old preference file and previously downloaded Linux toolkit are left in place.

To make a Finder-launchable app and a zip:

```sh
tools/notices.sh /tmp/THIRD-PARTY-LICENSES.txt
tools/macos/package.sh 0.1.3 target/release/anyplay-launcher /tmp/THIRD-PARTY-LICENSES.txt dist
```

The app is signed ad hoc for local use. Publishing a notarized release needs a Developer ID signing workflow. macOS CI checks formatting, Clippy and tests, builds and packages the launcher, and renders Metal screenshots alongside the Windows and Linux jobs.

The Mac package does not yet embed `anyplay-net`, so online integration needs a follow-up. Mac release assets and the Mac entry in the signed self-update manifest are also still to do.

## Releases

A release is made by pushing a tag `v<version>` that matches the version in `Cargo.toml`. The [release workflow](../.github/workflows/release.yml) builds the net service from [anyplaynet/network](https://github.com/anyplaynet/network) at the commit in `network.ref` and embeds it, then builds the portable exe and the installer (`tools/installer/anyplay-launcher.iss`, Inno Setup), the Linux AppImage and tarball, and a licence file for every third-party crate. It publishes all of them with their checksums in `SHA256SUMS.txt`. Started by hand it only builds them and keeps them as workflow artifacts. Release notes live in [docs/releases](releases). Before the tag, refresh the README screenshots in `docs/media` from the release build, taken with a separate profile and no personal data.

## More

- [data/README.md](../data/README.md): how to add a recommended setting. Community key layouts are added under `community/layouts`.
- [CONTRIBUTING.md](../CONTRIBUTING.md): how to contribute.
