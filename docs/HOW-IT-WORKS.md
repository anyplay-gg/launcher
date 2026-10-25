# How the launcher works

The launcher is a Cargo workspace of three crates. All logic lives apart from the interface.

| Crate | What it holds |
|---|---|
| `crates/anyps5-core` | All logic, no interface: library scan, unlock pipeline, toolkit, launch and diagnosis, controls, saves, trophies, settings, self-update and the online state. The launcher depends on it; it never depends on the launcher. |
| `crates/anyps5-launcher` | The application `anyplay-launcher`: egui and eframe on wgpu, the screens, the sounds, the window handling and the glue between them. |
| `crates/fake-relinker` | A stand-in for AnyPS5's relinker, so tests run without the toolkit or a game. |

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

At start, in the background, the launcher asks GitHub for the latest release of this repository. If it is newer, the launcher downloads the portable exe and compares it with its line in the release's `SHA256SUMS.txt`. A missing or wrong checksum refuses the update and leaves nothing behind. Windows can rename a running exe but not overwrite it, so the swap moves the running exe to `<name>.old` and puts the new one in its place; if the second step fails, the old one moves back. The next start removes the `.old` file. The swap happens when you close the launcher. It works for the installed exe and for the portable exe in any folder you can write to.

The checksum list comes from the same release as the exe. That guards against a damaged or substituted download, not against a compromised release. The launcher therefore trusts a signed manifest instead ([UPDATES.md](UPDATES.md)). Since v0.2.0 the launcher carries the key "anyplay 2026" in `data/update-keys.txt`, and an update is installed only if it is signed by a key on that list, has not expired, is newer than the version you run, and matches the signed hash and size. If anything is wrong, the update is skipped and the installed launcher stays as it is. Older launchers (up to v0.1.3) check only the checksum file and take v0.2.0 that way once. Releases are not code-signed yet. Self-update exists on Windows only.

More on what is pinned and how to verify a release: [supply-chain.md](supply-chain.md).
