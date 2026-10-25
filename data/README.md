# Recommended settings

`recommended-settings.v1.json` lists settings that are known to help (or hurt) a particular game. The launcher shows them for a game as "Recommended for this game": a short list of what would change, and one button to apply it. Nothing is applied without that click. A copy is built into every launcher, and the launcher fetches the current file from this repository once a day.

## Add or change an entry

Open a pull request that adds one entry under the game's title ID. Every entry needs proof:

- `evidence`: at least one https link. A line in upstream's `USAGE.md`, a Game bring-up discussion, an issue or a log that shows the problem and the fix.
- `verified`: the toolkit version you tried it on, the date, and `by`: `docs` (taken from upstream's documentation), `maintainer`, or `community` (you tried it). `os` and `gpuVendor` are optional and worth adding when the result depends on them.
- `why`: one or two plain sentences the player will read. Say what happens without it.

Leave out what you did not check. A wrong recommendation costs a player more than a missing one.

## Kinds

| `kind` | Meaning |
|---|---|
| `required` | The game does not start without it. Applied with the rest, and shown first. |
| `recommended` | A better experience. A value the player set themselves is kept. |
| `avoid` | A known bad value. It only warns, when the game would run with it. |

## What an entry can set

The launcher accepts only what is listed here. A file with anything else is refused as a whole, and a test in CI checks the file in this repository.

- `set.env`: `APS5_PLAYGO_UNLISTED_CHUNKS` (`installed` or `absent`). `APS5_TEXTURE_CACHE_MIB` may appear in an `avoid` entry only. Everything else the launcher owns has a setting of its own: use `set.launcher` for language, vsync, overlay and the shader cache, and never name a path.
- `set.relink`: flag and value pairs for the relinker: `--windows-fixed-base 0x...`, `--module-dir <relative path>`, `--executable <relative path>`. A relinker flag only counts when the game is unlocked again.
- `set.launcher`: `vsync` (true or false), `overlay` (`off`, `tl`, `tr`, `bl`, `br`), `shaderCache` (true or false), `language` (0 to 30), `toolkit` (like `v0.1.2`).
- `set.launcher.toolkit` is not a setting that gets applied: it is the toolkit release the game works best on. The game sheet offers it as "Recommended for This Game" in "Toolkit for This Game", and the player chooses. Use `appliesTo.toolkit` for the toolkits the advice is given to: `min` = `v0.1.3` recommends the release to players on v0.1.3 or newer. It is for `required` and `recommended` entries, not `avoid`. Launchers before the one that knows it refuse a file that holds it and keep their shipped catalogue, so add the first one when you want that.
- `appliesTo`: `contentVersions` (exact strings from the game's `param.json`, or `*`), `toolkit.min` and `toolkit.max` (like `v0.1.3`), and `when.missing`, a list of dump files that must be absent (`sce_sys/playgo-chunk.dat`, `sce_sys/playgo-scenario.json`).

When an upstream release fixes the cause, set `appliesTo.toolkit.max` to the last release that needs the entry instead of deleting it.

## Format

```json
{
  "id": "playgo-installed",
  "kind": "required",
  "appliesTo": { "when": { "missing": ["sce_sys/playgo-chunk.dat"] } },
  "set": { "env": { "APS5_PLAYGO_UNLISTED_CHUNKS": "installed" } },
  "why": "This dump has no PlayGo data. Without this the game cannot load its saves.",
  "evidence": ["https://github.com/boykopovar/AnyPS5/blob/main/docs/user/USAGE.md#playgo-dumps-without-metadata"],
  "verified": { "toolkit": "v0.1.3", "date": "2026-10-22", "by": "docs" }
}
```

Entry ids are lowercase letters, digits and dashes, and unique within a game. Run `cargo test -p anyps5-core recommended` before you open the pull request.
