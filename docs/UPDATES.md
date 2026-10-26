# Signed updates

How the AnyPlay Launcher decides that an update is genuine, and what the operator does to switch that on and to keep it working. The code is `crates/anyps5-core/src/selfupdate.rs` (the client) and `crates/update-manifest` (the manifest format, the checks and the `update-sign` tool). The interface side of updating (the notice, About, Restart Now, the swap on exit) is in [DESIGN.md 9.5](DESIGN.md#95-the-launcher-updates-itself); nothing in it can weaken a rule below.

## Why

Until a key is trusted, the launcher believes the latest GitHub release: it downloads the exe and checks it against `SHA256SUMS.txt` from the same release. Whoever can publish a release (a stolen token, a compromised workflow) can therefore run code on every player's PC. A signature over the exe alone would not be enough either:

- **Rollback.** An attacker serves an old, genuine, signed, vulnerable release as the latest.
- **Freeze.** An attacker keeps serving a stale answer so a fix never arrives.

A signed manifest that carries a version and an expiry, both checked by the client, closes both.

## The manifest

Every release carries two extra assets next to the binaries: `latest.json` and `latest.json.sig`.

```json
{
  "schema": 1,
  "version": "0.2.0",
  "published": "2026-11-01T12:00:00Z",
  "expires": "2026-12-01T12:00:00Z",
  "minimum_version": "0.1.0",
  "min_supported_version": "0.2.0",
  "platforms": {
    "windows-x86_64": {
      "name": "AnyPlay-Launcher-0.2.0-x64.exe",
      "url": "https://github.com/anyplaynet/launcher/releases/download/v0.2.0/AnyPlay-Launcher-0.2.0-x64.exe",
      "sha256": "<64 lower-case hex digits>",
      "size": 12345678
    },
    "linux-x86_64": {
      "name": "AnyPlay-Launcher-0.2.0-x86_64.AppImage",
      "url": "https://github.com/anyplaynet/launcher/releases/download/v0.2.0/AnyPlay-Launcher-0.2.0-x86_64.AppImage",
      "sha256": "<64 lower-case hex digits>",
      "size": 87654321
    }
  }
}
```

| Field | Meaning |
|---|---|
| `schema` | The format. A launcher reads exactly `1` and refuses anything else. |
| `version` | The version this manifest offers, plain semver without `v`. |
| `published`, `expires` | RFC 3339, UTC. After `expires` the manifest is void. The release workflow sets 30 days; the tool refuses more than 90. |
| `minimum_version` | The oldest running version that may update from this manifest. A launcher below it refuses (it needs an intermediate release or the installer). The workflow takes it from the repository variable `UPDATE_MINIMUM_VERSION`, default `0.1.0`. |
| `min_supported_version` | **Optional.** The oldest running version that may keep going online. A launcher below it, which has verified this manifest, treats the update as *required* ([Retiring a version](#retiring-a-version)). Absent means every version may; launchers that do not know the field ignore it. Plain semver, not above `version`. It is a different thing from `minimum_version` (which versions can take *this* update) and has its own name because launchers up to v0.2.x read `minimum_version` as "may not update from here". The workflow takes it from the file `MIN_SUPPORTED_VERSION` of the repository, empty by default. |
| `platforms` | Per platform the download's `name`, `url`, `sha256` and `size`: `windows-x86_64` (the portable exe) and `linux-x86_64` (the AppImage). A manifest without a Linux entry (every one up to v0.2.4) offers Linux nothing, quietly: it is no error. Windows always needs its entry. |

`latest.json.sig` is one line: the Ed25519 signature in hex over `anyplay-launcher-update-manifest-v1\n` followed by the exact bytes of `latest.json`. The prefix means a signature made for this purpose is good for nothing else. The bytes are verified before they are parsed; nothing is canonicalised.

## What the launcher checks

Once a key is trusted ([Migration](#migration)), an update is taken only when all of this holds, and a refusal only writes a line to the log:

1. The release has `latest.json` and `latest.json.sig`. The release's tag, its `SHA256SUMS.txt` and its other asset names are not used.
2. The signature is valid (strict Ed25519) for one of the trusted keys.
3. The manifest is well formed: schema `1`, plain versions, `minimum_version` not above `version`, `expires` after `published`, lower-case hex hash, a size above zero.
4. It has not expired.
5. `version` is newer than the running version (semver, pre-releases below their release). An equal or older version offers nothing: this is the rollback check.
6. The running version is not below `minimum_version`. (A launcher that is below it *and* below `min_supported_version` is told to install the new version by hand from the releases page, on the same "Update required" screen, instead of getting a quiet refusal.)
7. The download's `name` is exactly `AnyPlay-Launcher-<version>-x64.exe` (or the old `AnyPS5-Launcher-<version>-x64.exe`) on Windows and `AnyPlay-Launcher-<version>-x86_64.AppImage` on Linux, and its `url` is https.
8. The download's SHA-256 **and** size are the manifest's. The file gets its final name only after that, and a `.verified` marker holding the hash is written beside it.

A staged exe from an earlier run is offered again only if it has its marker and still hashes to it; what an unsigned launcher left in the updates folder is ignored.

Offline, the check fails with a network error and the launcher runs as it is. A refused update never touches the installed exe: the swap only ever sees a file that passed 8.

### What this does not stop

- A signed manifest stays valid until its `expires`. An attacker who can serve files can offer a **genuine older release that is still newer than what the player runs**, for up to 30 days after that older manifest was made. That is why the expiry is short; for a release that fixes a serious hole, publish it and, if needed, raise `UPDATE_MINIMUM_VERSION` so launchers below the fixed release stop accepting older steps.
- A player's clock set into the past defeats the expiry check on that PC.
- A launcher released before this change (up to the version that carries the first key) does not check any signature. It keeps updating from the unsigned path, which is why every release still carries `SHA256SUMS.txt` and `AnyPS5-Launcher-<v>-x64.exe`. Those launchers stay exposed until their players update once.
- The exe is not code-signed (Windows SmartScreen).

## Linux: the AppImage

The same signed manifest updates the AppImage. Every rule above (signature, expiry, rollback, `minimum_version`, name, https, SHA-256 and size, the repeated check of a staged file) applies unchanged, and **Linux has no unsigned flow at all**: with an empty key list nothing is offered, and a staged file is never offered without a manifest that vouches for it. What differs is which file is replaced and how (`selfupdate::install_path`, `Staged::swap_in`):

- **Which file.** The runtime of an AppImage sets `APPIMAGE` (the path of the `.AppImage`) and `APPDIR` (its mount). Child processes inherit both, so they count only when the running program lies inside `APPDIR` (a `.tar.gz` launcher started from some other AppImage's terminal must never overwrite that one), the path is absolute without `..`, ends in `.AppImage` and is a regular file, not a link. A folder that refuses new files is reported before anything is downloaded (About says it is read-only).
- **The swap.** The staged file is copied to a sibling `<name>.new`, hashed against the manifest (a mismatch removes the copy and refuses), set to mode 0755, synced and renamed over the original in one atomic step. Linux allows that while the old image runs: the running process keeps its open file. The original is untouched until the rename, and a failure at any step leaves it as it was. A restart starts the new file with the same arguments.
- **No AppImage, no swap.** A launcher from the `.tar.gz` (no `APPIMAGE`) cannot replace itself. It still checks the signed manifest, and About shows "<v> available" with *Download from GitHub*; nothing is downloaded.
- **The download** must be a 64-bit ELF with the AppImage type 2 magic after its header, on top of the hash and the size.

## Retiring a version

Retiring means: a launcher below some version must update before it goes online. It can still play offline (DESIGN.md [9.5.1](DESIGN.md#951-a-required-update)). Two independent switches do it, alone or together. Both are **off** by default, and nothing in the repositories sets them.

**Switch 1: the signed manifest** (the launcher's own, every platform).

1. Release the version everyone should be on, say 0.3.0, as usual, and wait until it is the latest release (the release workflow signs its `latest.json`).
2. Open a pull request that puts the version on a line of `MIN_SUPPORTED_VERSION` (the file at the root of anyplaynet/launcher; `#` lines are comments), for example `0.3.0`, and merge it. The file holds one plain version or nothing. A typo, `v0.3.0`, or a version above the source's own fails `cargo test -p anyplay-update-manifest` on the pull request, and would fail the release and the renewal too; it is never read as "off".
3. Apply it now instead of on Monday: run the workflow *resign* by hand (Actions, resign, Run workflow). It takes the file from the default branch and signs a new `latest.json` of the latest release with `min_supported_version`. From then on, every launcher below 0.3.0 that checks for updates shows *Update required*. The weekly renewal and every later release keep copying the file, so it stays until the file changes. The version must not be above the latest release (the renewal fails loudly if it is): release first, then raise the file.
4. Check: `gh release download --repo anyplaynet/launcher --pattern latest.json -O - | grep min_supported_version`.

To switch it **off**, leave only comments in the file, merge, and run *resign*. To retire further, put a higher version in.

The field also reaches a launcher that is too old to update itself (below `UPDATE_MINIMUM_VERSION`, the "can take this update" floor): it shows the same screen with *Download from GitHub*. Launchers that predate this mechanism (v0.1.x, v0.2.x) do not know the field and keep their optional update.

A player who turned *Check for Updates* off does not read the manifest; the cloud (switch 2) still reaches them when they go online.

**Switch 2: the cloud** (online services only). On the cloud, set `min_launcher_version` (environment `ANYPLAY_CLOUD_MIN_LAUNCHER_VERSION`, see the cloud's docs/configuration.md) to the version and redeploy. A launcher below it, which sends `X-AnyPlay-Client: launcher/<version>` through the net service, gets `client_too_old` and shows the same screen. Launchers that send no header, the web and other clients are not affected. To switch it off, empty the variable and redeploy.

Order for a retirement: release, set the file (switch 1), and only some days later the cloud floor (switch 2), so players who update on their own are not caught by the cloud first.

## Provenance of a release

The signed manifest answers "may this launcher take this update?". A second, independent check answers "was this file built by our release workflow, from which commit?": every file of a release has a GitHub artifact attestation, made by the `release` job of [release.yml](../.github/workflows/release.yml) when a tag is pushed. A player or an operator checks it with the GitHub CLI:

```
gh attestation verify AnyPlay-Launcher-0.2.0-x64.exe --repo anyplaynet/launcher \
  --signer-workflow anyplaynet/launcher/.github/workflows/release.yml \
  --source-ref refs/tags/v0.2.0
```

`latest.json` and `latest.json.sig` are renewed every week by `resign.yml` ([below](#renewing-the-manifest)), so for those two files the signer workflow is that one and there is no tag ref.

The launcher does not check attestations itself; the manifest is what it trusts. The SBOMs, the dependency list inside each binary and what is not covered yet (reproducible builds, code signing of the exe) are in [supply-chain.md](supply-chain.md).

## Migration

`data/update-keys.txt` lists the trusted public keys, one per line as `ed25519 <64 hex digits> [label]`; blank lines and `#` comments are ignored. It is embedded into the launcher at build time.

| The file holds | The launcher |
|---|---|
| No key (the launchers before 0.2.0) | Updates exactly as before: latest release, `SHA256SUMS.txt`. Signing is not switched on. |
| One key or more | Requires a valid signed manifest. **There is no fallback to the unsigned path**, not for a release without a manifest, not for a failing signature, not ever. |
| A line that is neither a comment nor a valid key | Fails the build (a test parses the file), and a launcher that somehow carries such a file refuses to update. A damaged list is never read as "no key". |

The rule lives in `selfupdate::signing_required` and `selfupdate::check_with`. The release workflow enforces the other half: the key list is first read by the launcher's own parser (`cargo test -p anyps5-core the_key_list`), so a damaged list fails the release before anything is signed; then, with a trusted key but no `UPDATE_SIGNING_KEY` secret, the release **fails** instead of publishing something no launcher would accept; with neither it publishes as before.

The first release that carries a key is the one that makes its own players safe. Players on older launchers update to it through the unsigned path one last time.

## The key ceremony (operator)

The real key is made on the operator's machine, never in CI. The operator's agent may run the steps, as long as nothing reads or prints the secret: the tool writes it to a file, `gh secret set` reads it from that file, and no command shows its contents.

The first key, labelled "anyplay 2026", was made this way on 2026-10-24.

### 1. Make the key

Build the tool once (`cargo build --release -p anyplay-update-manifest --bin update-sign`), then:

```
update-sign keygen --secret-out update-signing.key --public-out update-keys.line --label "anyplay 2026"
```

- The secret (32 bytes in hex) goes to `update-signing.key`. It is never printed and an existing file is never overwritten.
- The public line goes to `update-keys.line` and to the terminal; that one is meant to be shared.

### 2. Store the secret

- Keep a backup of `update-signing.key` with its label and date: in a password manager, or in a folder on the operator's machine that only the operator's account can read. Losing it means a rotation ([below](#rotate-the-key)), not a disaster.
- Add the same text as the secret **`UPDATE_SIGNING_KEY`** of **two Environments**, not as a repository secret (anyplaynet/launcher, Settings, Environments; [Protecting the key](#protecting-the-key) says how to set them up). A repository secret would be readable by any workflow run on any branch.
- Delete every other copy of the file.

With the `gh` CLI, as an admin of the repository, the secret goes into both environments without being shown:

```
gh secret set UPDATE_SIGNING_KEY --env release-signing --repo anyplaynet/launcher < update-signing.key
gh secret set UPDATE_SIGNING_KEY --env release-resigning --repo anyplaynet/launcher < update-signing.key
```

### 3. Publish the public key

Open a pull request that adds the line from `update-keys.line` to `data/update-keys.txt`. Do not merge it before the secret from step 2 is set: after the merge, any release built from `main` needs the secret.

### 4. Release

Tag the release as usual. The job `sign` waits for a reviewer to approve the environment `release-signing`, signs the manifest with the secret, checks the signature against `data/update-keys.txt` (a mismatch fails the release) and hands `latest.json` and `latest.json.sig` to the job that publishes. Check that both are among the release's assets. Test once with a launcher built from the commit that carries the key, against that release, before announcing.

To try the signing without publishing, run the release workflow by hand (it keeps all files as workflow artifacts and publishes nothing). A run by hand never reaches the real key: it signs with a key made in the job and thrown away, and checks the result with its public half, so `latest.json` and `latest.json.sig` among the artifacts are only a test. Locally:

```
update-sign manifest --version 0.2.0 --exe AnyPlay-Launcher-0.2.0-x64.exe --out latest.json
update-sign sign --manifest latest.json --out latest.json.sig --key-file update-signing.key
update-sign verify --manifest latest.json --sig latest.json.sig --keys data/update-keys.txt --current 0.1.3
```

### Rotate the key

A key must be replaced when it may have leaked, or on a schedule. Keys are rotated without a gap because the list holds several:

1. Make the new key (step 1) and store it (step 2) under a **new** secret name, for instance `UPDATE_SIGNING_KEY_NEXT`; do not replace the old one yet.
2. Open a pull request that **adds** the new line to `data/update-keys.txt` and keeps the old one. Release. Now launchers carry both keys and accept either.
3. Wait until the players have taken that release (watch the download counts; a launcher takes the update on its next start).
4. Set `UPDATE_SIGNING_KEY` to the new secret, release again, check that it verifies.
5. Open a pull request that **removes** the old line. Release. Launchers that carry the new list refuse anything the old key signs.
6. Delete the old secret from the password manager.

If the old key **leaked**, shorten the wait: anyone holding it can sign a manifest for any launcher that still trusts it until step 5 has been taken. Skip step 3 as far as you can, say so in the release notes, and consider raising `UPDATE_MINIMUM_VERSION` so older launchers must take the rotated release before anything else.

If the secret is **lost** (not leaked), nothing can be signed with the old key any more and every launcher still trusts only that key. Those players need the installer from the releases page. Keep the backup.

## Protecting the key

The release workflow signs in one job, `sign`, which runs only for a pushed tag `v*` and uses the Environment **`release-signing`**. The signing tool is built in an earlier job that has no secret; only the one step that runs `update-sign sign` has the secret in its environment. All of this is worthless if anyone with write access can still reach the secret, so the operator sets up, once, in Settings of anyplaynet/launcher:

1. **Environment `release-signing`.** Deployment branches and tags: *Selected branches and tags*, add the tag rule `v*` and nothing else. Required reviewers: the admins (today AlxcBTW and TideOPL). *Prevent self-review* is off, because only admins can push a `v*` tag (step 3): the review is the last look before signing, not a second key holder. Turn it on once the project has enough maintainers that the person who tags is never the only one around. The secret `UPDATE_SIGNING_KEY` goes here.
2. **Environment `release-resigning`** for the weekly renewal below: deployment branches *Selected branches*, only `main`; no reviewers (a weekly run must not wait for a person). The same secret goes here. Because it is bound to `main`, protect `main` too: a ruleset that requires a pull request with one approval and blocks force pushes, so nothing reaches `main` that was not reviewed.
3. **Protect the tags.** Settings, Rules, Rulesets, new tag ruleset for `v*`: restrict creations, updates and deletions to the maintainers, block force pushes. Whoever can push a `v*` tag can start a release.
4. **Delete the repository-level secret** `UPDATE_SIGNING_KEY` if one exists, and every other copy outside the two environments.

Until step 1 is done, the environment does not exist yet and GitHub creates it unprotected on the first run: do it before the first signed release.

A workflow edited on a branch cannot get the key: a run from a branch or started by hand is refused by the environment's rules, and the dry run uses a throwaway key. What remains is anyone who can merge to `main` (hence the ruleset and its required approval) and the admins, who can both push a tag and approve the signing.

## Renewing the manifest

A manifest expires after 30 days, and a launcher refuses an expired one, so a month without a release would end the update channel. The workflow [resign.yml](../.github/workflows/resign.yml) runs every Monday (and by hand): it takes the latest release, checks that its exe still has the hash in `SHA256SUMS.txt`, makes a new manifest with a fresh `published` and `expires` for the same version, signs it in the environment `release-resigning` and replaces `latest.json` and `latest.json.sig` of that release. Nothing else about the release changes. It does nothing while the latest release has no signed manifest. If the secret is missing there, the run fails and GitHub mails the maintainers: fix it within the weeks the old manifest still has left.

The new manifest has its own attestation, made by `resign.yml`; `gh attestation verify` on `latest.json` therefore names that workflow, not `release.yml` (see [Provenance of a release](#provenance-of-a-release)). What launchers trust is the Ed25519 signature, not the attestation.

## The tool

`update-sign` (crate `anyplay-update-manifest`) does the four jobs of the ceremony and of the workflow:

| Command | Does |
|---|---|
| `keygen --secret-out <file> [--public-out <file>] [--label <text>]` | Makes a key pair. Prints only the public line. |
| `manifest --version <v> --exe <file> --out <file> [--appimage <file>] [--minimum <v>] [--supported-file <MIN_SUPPORTED_VERSION>] [--days 30] [--base-url <url>]` | Hashes the exe (and the AppImage, when given) and writes the manifest. The exe must be named `AnyPlay-Launcher-<v>-x64.exe`, the AppImage `AnyPlay-Launcher-<v>-x86_64.AppImage`. Without `--appimage` the manifest is Windows-only. `--supported-file` reads the repository's `MIN_SUPPORTED_VERSION` into `min_supported_version`; a file with only comments leaves the field out, and anything but one plain version (not above `--version`) is an error. `release.yml` always passes it; `resign.yml` only when the release it renews has an AppImage. |
| `sign --manifest <file> --out <file> (--key-env <NAME> \| --key-file <file>)` | Signs, after checking that the manifest is well formed, and verifies the new signature before writing it. Never prints the key. |
| `verify --manifest <file> --sig <file> --keys <file> [--current <v>]` | What the launcher does, for a release candidate. |

Tests use throwaway keys made inside the test. The repository holds no private key; `data/update-keys.txt` holds the public key "anyplay 2026".

## Client-side hardening

Rules of the client that go beyond the list under [What the launcher checks](#what-the-launcher-checks):

- **Size, time and redirects.** A signed download stops as soon as it is longer than the manifest's `size`; a connection that cannot be opened in 15 seconds or a transfer that makes no progress for 30 seconds is dropped. Downloads follow at most three redirects and only over https. The manifest and its signature must come from `github.com` or `*.githubusercontent.com` (also after redirects); the exe may be on any https host because its hash is signed. An address with a user name in it (`https://github.com@evil.example/`) is refused. Plain http to this machine exists only in the test build.
- **Staged updates are verified again.** The manifest and signature are kept beside the staged exe (`<exe>.manifest`, `<exe>.manifest.sig`). At the next start [`pending`] checks signature, expiry, version and the exe's size and SHA-256 before it offers the update again; a file or marker alone is never trusted, and a staged update whose manifest has expired is not offered (the next check fetches a fresh one). The swap hashes its own copy against the manifest's hash before renaming it into place.
- **Validity window.** `expires` may be at most 90 days after `published`, checked by `Manifest::validate` (so `update-sign sign` refuses a longer one too), and the client refuses a `published` more than a day ahead of its clock.
- **Order of refusals.** A launcher that already runs the offered version or a newer one gets "nothing to update" even when the manifest has expired; only a manifest that would offer an update is refused as expired.
- **The key list** may start with a UTF-8 byte order mark. The build of `anyps5-core` parses `data/update-keys.txt` with the launcher's own parser (`build.rs`) and fails on an unreadable list.
- **`update-sign keygen` on Windows.** The secret file is created with the access rules of the folder it is written to (on Unix, mode 0600). Write it into a private folder, not a shared or synced one, and move it to the password manager at once.

## When the launcher checks

- **At the start, then every 6 hours** while the launcher stays open; after a check that could not reach GitHub, again after 30 minutes. Only with the *Check for Updates* preference on, except for the player's own *Check Now*.
- **The release data is asked for with an ETag** (`Cache/Feeds/launcher-latest.json` and its `.etag`), so a check that finds nothing new is one small `304` that GitHub does not count against the rate limit. The manifest and its signature (two small files on github.com) are fetched on every check that sees a release, and checked from scratch every time: the cache decides what is asked for, never what is trusted. If the network fails, the stored copy is **not** used as an answer; the check fails and the launcher says "Not reachable" instead of "Up to date".
- **The release notes** shown in "What's New" come from the `body` of the same release data. They are unsigned display text: cut to 32 KiB, control characters removed, rendered as plain lines. Nothing is decided by them, and no address in them is opened.
- **A refusal is quiet.** A signature, expiry, hash or size that fails is written to the log. The player sees it only on the About page ("Update failed", with *Download from GitHub*); no notice, no toast.
