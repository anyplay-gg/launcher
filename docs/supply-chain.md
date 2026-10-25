# Supply chain

How a launcher release can be traced from the source to the binary, and what makes it harder to slip a bad dependency or a bad action into one. The signed update manifest, which is what the launcher itself checks before it updates, is described in [UPDATES.md](UPDATES.md); this page is about everything around it.

## What is pinned

| What | How |
|---|---|
| GitHub Actions | Every third-party action is pinned by full commit SHA with the release it was taken from in a `# vX.Y.Z` comment. The SHA is looked up from the release tag with `gh api repos/<owner>/<action>/git/ref/tags/<tag>`, never copied from a blog post. |
| Workflow permissions | `contents: read` for every workflow. Only the `release` job of `release.yml` can write: `contents` (the release), `id-token` and `attestations` (the provenance). The signing key is in an Environment that only a pushed `v*` tag can reach, with required reviewers, and only the step that signs sees it ([UPDATES.md](UPDATES.md#protecting-the-key)). |
| Checkouts | `persist-credentials: false` everywhere. Nothing in these workflows pushes. |
| Crates | `Cargo.lock` is committed and every CI and release build uses `--locked`. The build tools of a release (`cargo-auditable`, `rust-audit-info`, `cargo-cyclonedx`) are installed with `--locked` at an exact version, set in the `env` of `release.yml`. |
| Downloads | Inno Setup, `appimagetool`, the AppImage runtime and `cargo-about` are checked against a SHA-256 in the repository before they run. |
| Release builds | No build cache, so a poisoned cache cannot put code into a binary that no commit holds. The Rust compiler is an exact version (`RUST_TOOLCHAIN` in `release.yml`). The network commit the net service is built from is a full SHA, the one in `network.ref` (overridable by the repository variable `NETWORK_REF` or by an input of a run by hand), and is used by every job; bump the file by a pull request. CI reads the same file, so the commit a release carries is the one the checks passed against. |
| Licences, advisories, sources | `cargo deny` with [deny.toml](../deny.toml) on every pull request (the shared `rules` workflow) and every Monday (`advisories.yml`). Unknown registries and git sources are refused. |

The shared `rules` workflow comes from `anyplaynet/.github@main`, which is the organisation's own code and is not pinned to a commit.

## What the share worker brings in

The share worker (also the recorder of Clips) is built from the network repository at the commit in `network.ref`. Its notable dependencies and their licences: `windows` and `windows-capture` (Windows capture, Media Foundation; MIT or Apache-2.0 and MIT), `minifb` (the viewer's window; MIT or Apache-2.0) and `mp4` 0.14 (writes the clips' MP4 files; MIT, pure Rust, 15 million downloads, last release 2023). On Linux: `ashpd` (the desktop portal's window picker; MIT), `pipewire` (the window's pictures; MIT; it links the system's `libpipewire-0.3`) and `openh264` with `openh264-sys2` (BSD-2-Clause), which load Cisco's OpenH264 binary at run time and only a build whose SHA-256 they know. No OpenH264 code is built into the worker or the launcher: Cisco's binary is downloaded from Cisco to the player's PC when the player turns the codec on in Settings, checked by the SHA-256 of the archive and of the library (`anyps5-core/src/openh264.rs`, `bzip2` to unpack it), and removed when they turn it off. Cisco's licence for that binary is added to `THIRD-PARTY-LICENSES.txt`. MIT is compatible with this repository's GPL-2.0-only; Apache-2.0 alone would not be, which is why the MP4 writer is `mp4` and not `shiguredo_mp4`. The network repository's `cargo deny` run holds the policy; this repository's `deny.toml` and the SBOM of the share worker cover what the launcher ships.

## How updates arrive

[Dependabot](../.github/dependabot.yml) opens one pull request a week for the actions (all in one) and one for the crates (minor and patch updates in one; a major update by itself). It waits a week after a release before it proposes it, so a release that is pulled or found malicious in its first days never reaches us. Security updates ignore that wait.

The launcher and the net service are built from `Cargo.toml` files that name `../network` by path. Dependabot sees only this repository, so it may not be able to resolve the crates and report an error instead of a pull request. If it does, update the crates by hand (`cargo update`, then the checks CI runs) until the dependency on the network is a version instead of a path.

## What a release carries

Next to the binaries, installer, `THIRD-PARTY-LICENSES.txt` and the signed manifest, every release has:

- `SHA256SUMS.txt`, which now also covers the files below;
- six SBOMs in CycloneDX 1.5: `AnyPlay-Launcher-<v>-windows-x86_64.sbom.cdx.json` and `...-linux-x86_64.sbom.cdx.json` for the launcher, and `...net-service.sbom.cdx.json` and `...share-worker.sbom.cdx.json` for the same platforms for the net service and the share worker inside it. They list the crates that go into the binary, not the build tools, and are made from the `Cargo.lock` files the build used;
- `AnyPlay-Launcher-<v>-BUILD-INFO.txt`: the launcher commit, the network commit and the workflow run;
- an attestation for every one of these files (see below).

The binaries themselves are built with `cargo auditable`: the list of every crate is inside the exe, the AppImage's executable, the net service and the share worker. The release fails if a build lost it.

## Verify a release

You need the [GitHub CLI](https://cli.github.com/) (2.49 or later). Download the file, then:

```
gh attestation verify AnyPlay-Launcher-0.2.0-x64.exe --repo anyplaynet/launcher
```

That checks a signature made by GitHub's attestation service over the file's hash, and prints which workflow, on which commit and tag, built it. To insist that it was this repository's release workflow on that tag:

```
gh attestation verify AnyPlay-Launcher-0.2.0-x64.exe --repo anyplaynet/launcher \
  --signer-workflow anyplaynet/launcher/.github/workflows/release.yml \
  --source-ref refs/tags/v0.2.0
```

The same works for the installer, the AppImage, the tarball, the SBOMs and the checksum file. A file that was changed after the build, or that the workflow never built, fails.

To read what is inside a binary without trusting the release page, take the dependency list out of it (`cargo install rust-audit-info`, then `rust-audit-info AnyPlay-Launcher-0.2.0-x64.exe`), or feed the SBOM to a scanner such as Grype or Trivy.

The attestation is made by the `release` job of a tag build. A release workflow started by hand is a dry run and attests nothing.

## What is not covered yet

- **Reproducible builds.** Two builds of one commit do not give the same bytes (timestamps in the PE file, the moving `stable` toolchain, Inno Setup). The attestation says who built a file, not that anyone else gets the same file.
- **Code signing of the Windows exe.** The exe and the installer are not Authenticode-signed, so SmartScreen warns. The update manifest has its own Ed25519 signature ([UPDATES.md](UPDATES.md)); that protects the launcher's self-update, not a first download.
- **The Rust toolchain** of CI (not of releases) is `stable`, and the runner images and the apt packages of the Linux build are whatever GitHub provides that day.
- **The shared rules workflow** is referenced by branch, see above.
- **Vulnerabilities in a built binary** are not scanned after the build; `cargo deny` looks at the lockfile.

## Fonts and icons

Licence texts are in `crates/anyps5-launcher/assets/fonts/`. Bundled: [Inter](https://rsms.me/inter/) (SIL OFL 1.1), [JetBrains Mono](https://www.jetbrains.com/lp/mono/) (SIL OFL 1.1), [Noto Sans](https://github.com/notofonts) in Latin, Mono, Thai and Arabic, which also stand in for the games' system fonts (SIL OFL 1.1; sources and checksums in `crates/anyps5-launcher/assets/fonts/noto/README.md`), and [Phosphor Icons](https://phosphoricons.com) (MIT). The Japanese, Chinese and Korean Noto Sans CJK fonts are not bundled: the launcher downloads them (pinned release, SHA-256 checked) on request or at the first game start.
