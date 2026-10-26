# AnyPlay Launcher

<p align="center">
  <img src="docs/media/home-screen.jpg" width="800" alt="The launcher's home screen: a row of game tiles, the selected game's artwork, its name, a Play button and its details">
</p>

The home of your games on PC. The launcher finds the games you dumped from your own console, unlocks them for [AnyPS5](https://github.com/anyplaynet/AnyPS5) and starts them from a console-style home screen. It also connects you with friends for parties, voice and sharing your captures. It runs on Windows, Linux and macOS (Apple silicon), in English, Turkish and German.

The latest release is [0.3.2](docs/releases/v0.3.2.md). [Download](#get-it) · [Discord](https://discord.gg/9zYqf8fEsh) · [anyplaynet.com](https://anyplaynet.com) · [Compatibility list](https://anyplaynet.com/compatibility)

## What it does

**Your library**
- Finds your dumps in one folder and reads title, artwork and music from them. A dump is never changed.
- Shows how each game runs for other players (runs well, runs with issues, doesn't start), with sort, filter and search.
- Unlocks a dump into a copy AnyPS5 can run, starts it, and says why it stopped if it fails.
- Shows the space each unlocked copy uses and removes it; saves are backed up first.
- A Game Hub per game: trophies, play time, captures, friends playing and how it runs.

**Play together**
- Friends with presence, messages, parties with voice and push to talk (also while a game is in front), and a Notification Center.
- Find Players for a game, and join codes for people who are not your friends.
- Captures and clips, which you can send to friends.
- Share your screen with a party, or broadcast to Twitch, YouTube or an RTMP server (Windows).

**Make it yours**
- Controller Center: battery, button tests, vibration, dead zones, and remapping the launcher's buttons.
- Your phone as a controller, through [LudusCtrl](https://github.com/parl0v/ludusctrl).
- Per-game settings and controls, with recommended settings you apply in one click.
- Themes, larger text, high contrast, sounds and volumes.

**Everywhere you play**
- Steam Deck Gaming Mode, and Add to Steam on Windows, Linux and the Mac.
- Self-updating and signed, except where a package manager owns the files.

The online area needs an AnyPlay account on [anyplaynet.com](https://anyplaynet.com). No game plays together over the internet yet; that is the next big step of the [network](https://github.com/anyplaynet/network). What works and what does not: [docs/ONLINE.md](docs/ONLINE.md).

## Screens

<table>
  <tr>
    <td width="50%"><img src="docs/media/home-minecraft.jpg" alt="Home screen with a game selected: artwork, title, Play button, trophy count, size, title ID and version"></td>
    <td width="50%"><img src="docs/media/game-settings.jpg" alt="Game settings panel beside the home screen: display, window size, vertical sync, controls, save data and performance overlay"></td>
  </tr>
  <tr>
    <td><img src="docs/media/settings.jpg" alt="Settings: appearance with theme, accent color, background and animations"></td>
    <td><img src="docs/media/online.jpg" alt="The Online screen before signing in: Sign In, Create Account and the cloud to use"></td>
  </tr>
</table>

## Get it

Download the [latest release](https://github.com/anyplaynet/launcher/releases/latest).

| System | File |
|---|---|
| Windows 10 or 11 | `AnyPlay-Launcher-<version>-Setup.exe` (installer, no administrator rights) or `-x64.exe` (one file) |
| Linux, 64-bit | `AnyPlay-Launcher-<version>-x86_64.AppImage`, `-linux-x86_64.tar.gz` or `-x86_64.flatpak` |
| macOS 11 or newer, Apple silicon | `AnyPlay-Launcher-<version>-macos-arm64.zip` |

You need your own decrypted game dumps. On first start the launcher asks for your games folder and downloads the AnyPS5 toolkit. Requirements, SmartScreen and Gatekeeper prompts, Flatpak, checksums and data folders: [docs/INSTALL.md](docs/INSTALL.md).

### Updates

The launcher checks for a new release when it starts and every 6 hours after. On Windows, in the AppImage and in the Mac app it downloads the update, checks the signature, hash and size, and installs it when you close the launcher. Turn this off in Settings, *Check for Updates*. A Flatpak (and soon an AUR package) leaves updates to the package manager and only tells you when a new version is out. The tar.gz does not update itself. How the signing works: [docs/UPDATES.md](docs/UPDATES.md).

## Build and contribute

Issues and pull requests are welcome; start with [CONTRIBUTING.md](CONTRIBUTING.md). You need Rust 1.95 and a checkout of [anyplaynet/network](https://github.com/anyplaynet/network) at the commit in `network.ref`:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

| Read | For |
|---|---|
| [docs/BUILDING.md](docs/BUILDING.md) | Building on each system, and releases |
| [docs/HOW-IT-WORKS.md](docs/HOW-IT-WORKS.md) | How the crates fit together |
| [docs/PORT.md](docs/PORT.md) | The logic step by step |
| [docs/DESIGN.md](docs/DESIGN.md) | Every screen, size, colour and animation |
| [CONTEXT.md](CONTEXT.md) | The words the code and docs use |
| [docs/releases](docs/releases) | Release notes |

## Community

Questions, bug reports and other players: the [AnyPlay Discord](https://discord.gg/9zYqf8fEsh). The Code of Conduct and security policy are in the [organisation's repository](https://github.com/anyplaynet/.github).

## Credits and licence

The launcher is GPL-2.0-only, see [LICENSE](LICENSE).

- It began as a port of the macOS launcher by [utkuhalis](https://github.com/utkuhalis) in [AnyPS5-ARM](https://github.com/utkuhalis/AnyPS5-ARM) (GPL-2.0). Since then it has its own implementation, Windows, Linux and online parts. The Rust files that began as ports name their Swift source in their first line; the mapping is in [docs/PORT.md](docs/PORT.md).
- AnyPS5, the compatibility layer the launcher runs games on, is by [boykopovar](https://github.com/boykopovar/AnyPS5). [anyplaynet/AnyPS5](https://github.com/anyplaynet/AnyPS5) is AnyPlay's fork that carries the network bridge.
- The LudusCtrl protocol and phone app are by Josip Parlov ([@parl0v](https://github.com/parl0v)), see [parl0v/ludusctrl](https://github.com/parl0v/ludusctrl).
- Bundled fonts and icons are listed in [docs/supply-chain.md](docs/supply-chain.md#fonts-and-icons).

AnyPlay is a community project, not affiliated with the AnyPS5 maintainers or any platform holder. It works with games you own: no game files, keys or firmware are included here, and none will be. AnyPS5 is a compatibility layer, not an emulator.
