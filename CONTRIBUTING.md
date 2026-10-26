# Contributing to the AnyPlay Launcher

Issues and pull requests from anyone are welcome here. The rules that hold in every AnyPlay repository (how to contribute, the line the project keeps, and no game data) are in the [organisation's CONTRIBUTING.md](https://github.com/anyplaynet/.github/blob/main/CONTRIBUTING.md), and the [Code of Conduct](https://github.com/anyplaynet/.github/blob/main/CODE_OF_CONDUCT.md) applies. This file adds what is specific to the launcher.

## Before you start

- **A small fix** (a typo, a wrong link, a clear bug): open a pull request.
- **Anything bigger:** open an issue first. Say what you want to change and why, and wait for an answer. Interface changes, new settings and new dependencies are always bigger.
- **Looking for something to do:** the issues labelled [good first issue](https://github.com/anyplaynet/launcher/labels/good%20first%20issue) are small and say where in the code to look.
- **A game does not work:** open a "Game report" issue. Do not put a fix for one title into the code. The launcher works the same for every game; what is known per title lives in `data/`.
- **Never attach game data** (dumps, keys, firmware, saves, or logs that contain game memory) to an issue or a pull request.

## Build and test

You need Rust 1.95 or newer and a checkout of [anyplaynet/network](https://github.com/anyplaynet/network) next to this repository, at the commit named in `network.ref`:

```sh
git clone https://github.com/anyplaynet/network ../network
git -C ../network checkout "$(cat network.ref)"
```

A pull request that needs newer network code changes `network.ref` in the same pull request. The details, including the net service and releases, are in [docs/BUILDING.md](docs/BUILDING.md).

These are the commands CI runs on Windows and on Linux. A pull request needs all of them green:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

Tests need no GPU, no game and no toolkit: `fake-relinker` stands in for the relinker.

### Windows

Windows 10 or 11 with the Rust MSVC toolchain (`x86_64-pc-windows-msvc`) and the Visual Studio Build Tools. CI and the releases use it. The GNU toolchain works for local development without Visual Studio; BUILDING.md lists the linker flags. Do not build in a folder whose path contains an apostrophe.

### Linux

Install the development packages first (Debian and Ubuntu names):

```sh
sudo apt-get install -y --no-install-recommends \
  pkg-config libasound2-dev libudev-dev libxkbcommon-dev libwayland-dev \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

## Interface changes

The interface follows [docs/DESIGN.md](docs/DESIGN.md). Read the section for the screen you change and keep to its sizes, spacing, colours, motion and focus rules. If your change needs a new rule, change DESIGN.md in the same pull request.

The launcher has a hidden gallery mode that renders a scene with made-up games and no personal data. Use it to check your change and to show it in the pull request:

```sh
cargo run --release -p anyps5-launcher -- --gallery=home --screenshot=home.png --size=1600x900
```

- `--gallery=<scene>` picks the scene. The scenes are listed in the module comment of `crates/anyps5-launcher/src/app/preview.rs`. `components`, `panels` and `themes` show the building blocks.
- `--screenshot=<file>` writes a PNG and exits. `--size=WxH` sets the window size, `--tr` renders in Turkish and `--pad` shows gamepad buttons in the hint bars.
- On Linux without a display, CI runs the release binary in a virtual X server with a software Vulkan driver: `xvfb-run -a target/release/anyplay-launcher --gallery=home --screenshot=home.png` (packages `xvfb mesa-vulkan-drivers libvulkan1`).

For a visible change, attach the screenshots of every scene you changed, before and after, and in Turkish too when text changes. A new state gets its own scene.

## Texts

Every text a player can read exists in English and Turkish. A new string goes into the strings file of its screen (`crates/anyps5-launcher/src/screens/*_strings.rs`, or `ui/i18n_table.rs` for the shared ones) with both languages. If you do not speak Turkish, write the English text and say so in the pull request; a maintainer will help with the translation.

## Where things are specified

| File | What it holds |
|---|---|
| [CONTEXT.md](CONTEXT.md) | The words the code and docs use. Use them in names, issues and commits. |
| [docs/PORT.md](docs/PORT.md) | The logic, step by step: library, unlock, launch, settings, trophies, saves. |
| [docs/DESIGN.md](docs/DESIGN.md) | The interface: every screen, size, colour, animation and sound. |
| [docs/HOW-IT-WORKS.md](docs/HOW-IT-WORKS.md) | How the pieces fit together. |
| [docs/ONLINE.md](docs/ONLINE.md) | The online area and how it uses the net service. |
| [data/README.md](data/README.md) | The data files (recommended settings, LAN titles) and how to add an entry with its evidence. |

If the code and a specification disagree, say so in the issue. One of them is wrong.

## Pull requests

- One topic per pull request, small commits, English commit messages, and a description that says what changed and why.
- Fill in the pull request template.
- A pull request needs one review and green CI. CI runs on Windows, Linux and macOS, and also checks the links in the Markdown files, the licences of dependencies and security advisories.
- Dependencies must suit GPL-2.0-only. `cargo-deny` checks this with `deny.toml`, so a crate offered only under Apache-2.0 does not fit.

## Licence

The launcher is GPL-2.0-only. By contributing you release your work under the same licence ([LICENSE](LICENSE)). There is no other agreement to sign.
