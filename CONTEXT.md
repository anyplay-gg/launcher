# AnyPlay Launcher

A launcher for Windows and Linux that finds a player's own PS5 game dumps, unlocks them for AnyPS5 and starts them. This file is the glossary; the specification of the logic is in [docs/PORT.md](docs/PORT.md).

## Language

**AnyPlay Launcher**:
This program. Up to v0.1.3 it was AnyPS5 Launcher; data folders, the store, the `anyps5://` scheme and the `ANYPS5_*` variables keep that name.
_Avoid_: AnyPS5 Launcher, AnyPS5 (that is the engine)

**AnyPS5**:
The compatibility layer of boykopovar/AnyPS5 that the launcher unlocks games for and starts them on. Never an emulator.
_Avoid_: Emulator

### Games and files

**Game**:
A PS5 title the player owns, found as a dump folder inside the games folder. The launcher identifies it by its folder.
_Avoid_: App, title (for the folder), ROM

**Dump**:
The decrypted folder of a game: an executable (`eboot.bin`), its modules, `sce_sys` metadata and the game's data. The launcher only reads it and never changes it.
_Avoid_: Image, rip, backup

**Games folder**:
The one folder the player points the launcher at. Games are looked for inside it, a few levels deep.
_Avoid_: Library folder, root

**Home button**:
The PS or Guide button of a controller, and the PS button of the phone controller. While a game runs it brings the launcher in front of the game, with the game's quick panel, and back (docs/DESIGN.md 6.6).
_Avoid_: PS button in visible text

**Title ID**:
The console's identifier of a game, such as `PPSA02100`, read from the dump's metadata.
_Avoid_: Game ID, product code

**Executable kind**:
What a dump's `eboot.bin` is: a plain ELF, a SELF container, or neither (encrypted or unknown).
_Avoid_: Format, type

**SELF container**:
The console's wrapper around an ELF. A decrypted one can be turned into the plain ELF the relinker reads; an encrypted one cannot be used.
_Avoid_: Signed file, fSELF (names one variant only)

**Module**:
A `.prx` or `.sprx` file of a dump that the game loads at run time.
_Avoid_: Plugin, library (a library is the toolkit's)

### Unlocking

**Unlock**:
Turn a dump into something AnyPS5 can run: unwrap its executable and modules, relink them, and assemble the unlocked copy. One word for the whole job, in the interface and in the code.
_Avoid_: Convert, install, port, jailbreak

**Unwrap**:
The first step of an unlock: turn every SELF container into a plain ELF.
_Avoid_: Unpack, decrypt (nothing is decrypted)

**Relink**:
The second step: the relinker rewrites the ELF files into native Windows programs and guest modules.
_Avoid_: Compile, recompile, emulate

**Unlocked copy**:
The folder the launcher makes for one game: the relinked executable, the toolkit's libraries, and the dump's files linked in. The game runs from it and writes its saves and trophies there.
_Avoid_: Bundle, install, build, cache

**Store**:
The hidden `.AnyPS5` folder inside the games folder, on the same volume as the dumps, where unlocked copies and work folders live.
_Avoid_: Cache, output folder

**Unlock record**:
The small file inside an unlocked copy that says which dump and which toolkit made it, and when.
_Avoid_: Manifest, receipt

**Outdated**:
An unlocked copy made by a different toolkit version than the current one. The player is offered to unlock it again.
_Avoid_: Stale, old

**Unlock queue**:
The games waiting to be unlocked. Only one is unlocked at a time; a game the player asked for goes before a game that is only being prepared.
_Avoid_: Job list

**Prepare**:
Unlock a game the player has not asked for yet: the launcher does it by itself for a game that appeared in the games folder (or that a toolkit update left outdated), one at a time, at the lowest priority, and only while the PC is idle. The tile says "Preparing". The word the player sees for the work they ask for stays Unlock.
_Avoid_: Install, download, pre-install

**Idle gate**:
The check before and during a preparation: no game starts or runs, the PC is on mains power, enough memory is free and the processors are not busy. When it closes, a preparation in progress stands still (it keeps what it did) and goes on when it opens.
_Avoid_: Scheduler, throttle

**Kept relinker output**:
When an unlock again finds nothing the relinker reads changed (the dump's executable and modules, the toolkit release, the flags, the patches), it lays the converted files of the copy it replaces into the new copy instead of relinking. The record's relink key says whether that is so.
_Avoid_: Cache

### Toolkit

**Toolkit**:
The set of AnyPS5 tools the launcher uses: the relinker, the system libraries and the per-title patches, with one version string.
_Avoid_: AnyPS5 build, SDK, backend

**Relinker**:
AnyPS5's command line program that turns a PS5 executable into a native one.
_Avoid_: Converter, compiler

**System libraries**:
The `.prx` files AnyPS5 provides in place of the console's system software. Every unlocked copy carries a copy of them.
_Avoid_: Libs, DLLs, drivers

**Toolkit version**:
The release tag plus a short fingerprint of the toolkit's files. It changes whenever a file does.
_Avoid_: Build number

**AnyPlay engine**:
The toolkit channel that follows the nightly build of anyplaynet/AnyPS5, the fork with the network bridge. It installs into its own folder, is checked against the release's checksum list, and is offered a new build when the archive's checksum changes, not the tag. Its version reads `anyplay-<commit>`.
_Avoid_: Nightly (for the player), fork build, bridge build

**Glibc shortfall** (Linux):
The system's glibc is older than the newest version the toolkit's libraries ask for. The launcher says so (Settings, the Unlock and Play buttons) and does not stop anything.
_Avoid_: Incompatible, unsupported

**Game toolkit**:
The toolkit one game is unlocked and started with: the global one, or the release the player let that game stay on ("pinned"). A pinned release that is not installed is never replaced by the global one; the player decides.
_Avoid_: Per-game version

### Launcher updates

**Update manifest**:
The file `latest.json` of a release, signed by `latest.json.sig`: the version, the address, SHA-256 and size of the exe per platform, an expiry and the oldest version that may update from it. Once the launcher trusts a key, it updates only through one.
_Avoid_: Release notes, version file

**Trusted update key**:
A public key in `data/update-keys.txt` whose signature the launcher accepts on an update manifest. The file may hold several, so the next key ships before the current one retires. With none, signing is not switched on and the launcher updates unsigned as before.
_Avoid_: Certificate, code-signing key (the exe itself is not code-signed)

### Playing

**Play**:
Start an unlocked game. The period from the press of Play until the game's first window shows is the **start**.
_Avoid_: Run, launch (as the player's word), boot

**Splash**:
The launcher's own screen shown during the start, until the game's first window appears.
_Avoid_: Loading screen

**Game settings**:
The player's choices for one game that become environment variables and an input file when the game starts: window, sync, overlay, language, controls, tuning, shader cache.
_Avoid_: Config, options

**Controls**:
The keyboard and mouse layout of one game: the launcher's default layout with the keys the player moved.
_Avoid_: Keybinds, mapping (the file format is the engine's)

**Pad action**:
A console controller input a key or mouse button can press, named as the engine names it (`Cross`, `LeftStickUp`, ...).
_Avoid_: Button (sticks and wheels are not buttons)

**Tuning**:
The numeric input settings of a game: stick deadzones, mouse-as-right-stick response, keyboard ramps.
_Avoid_: Sensitivity (one of its values)

**Session**:
One run of a game as the launcher saw it: from the moment the game's first window showed to its exit. A game that exits before a window shows has no session.
_Avoid_: Run, round

**Playtime**:
The total time of a game's sessions and how many there were, kept per game in the preferences. It is added to while the game runs, so a crash or a closed launcher keeps what was seen. The home screen shows it as "Played".
_Avoid_: Time played (in code), hours, uptime

**Shared settings**:
A game's settings exported to a file so another player can import them.
_Avoid_: Preset, profile

**Profile**:
The button in the top bar that opens the online area: the account's handle and avatar when signed in, "Sign In" when not. The PC's user name never shows on it. It has nothing to do with input files.
_Avoid_: Account (that is the network's), user

**Input profile**:
An input file the engine reads for one title ID, beside the shared input file. Not used by the launcher yet.
_Avoid_: Controls (the launcher's own layout)

### Save data

**Save data**:
What a game keeps of the player's progress in its unlocked copy: its saves in `_sd` and `_sd_mem`, and its trophies in `_trophy`. An unlock again carries it over; removing the copy deletes it.
_Avoid_: Saves (alone, for the folders), profile

**Backup**:
A zip of a game's save data under the launcher's data folder, named by when and why it was made. The newest ten of a game are kept, those made after playing going first. A backup is made by the player, before an unlock again, before a removal, before a restore and, at most once an hour, after a game ends well.
_Avoid_: Snapshot, save game

**Restore**:
Put a backup back in place of the game's save data. It refuses while the game runs, checks the zip before it changes anything, and backs up the current state first, so it can be undone.
_Avoid_: Load, roll back, import

### Clips

**Clip**:
The last seconds of the running game, kept in memory while Clips is on and saved as an MP4 file beside the screenshots when the player asks (hotkey, the controller's Create button or the phone's). Video only for now; it never leaves the PC.
_Avoid_: Recording, replay, capture

### Trophies

**Trophy**:
An achievement of a game, with a grade, a name, a description and, once earned, the time it was earned.
_Avoid_: Achievement

**Grade**:
A trophy's class: platinum, gold, silver or bronze. Each has a weight in the percentage.
_Avoid_: Tier, rank

**Trophy set**:
All trophies of a game as the console shows them, with the percentage earned.
_Avoid_: Trophy list (the screen)

**Trophy record**:
What the engine writes into an unlocked copy when the game earns a trophy.
_Avoid_: Save

**Run report**:
A signed-in player's answer to "How did it run?" after a session: doesn't start, menus, in game or playable, with the game version and the toolkit version it ran with. The launcher asks once per game version and toolkit version, in one quiet line on the game page, and sends the title ID, the two versions, the verdict and the system, nothing else. The game page shows the community verdict as the Runs fact once three players reported.
_Avoid_: Rating, review, compatibility report (the Report a Problem flow is another thing)

### Patches

**Patch**:
A byte change applied to one title's executable, for a value the host cannot meet. It only applies when the bytes it replaces are the expected ones.
_Avoid_: Mod, hack, crack

**Resolution patch**:
A patch that makes a title's output size a choice. Only some titles have one.
_Avoid_: Resolution setting (the player's choice of one of its values)

### Failures

**Diagnosis**:
The reason in plain words why a game stopped, found in its log and in how its process ended. It is one of: missing module, unsupported function, GPU lost, out of memory, engine error, crash, or unsupported CPU.
_Avoid_: Error, crash (one kind)

**Log**:
The text a game wrote while it ran, kept per title ID. A new start adds a marker line, so only the last start is read.
_Avoid_: Output

**Report**:
A text file with the PC, the versions, the game's settings, the diagnosis and the log of the last start, for the player to attach to a bug report.
_Avoid_: Dump (that is the game)

### Links

**Deep link**:
An `anyps5://` address that opens a place in the launcher: a game, a game's unlock, a page of the settings or a dump folder. It only shows the place; the player presses the button that does something. The interface calls them AnyPlay links; the scheme keeps its old name so links already out there work.
_Avoid_: Command, URL scheme (that is how Windows finds the launcher), protocol

**Link handler**:
The launcher that Windows starts for deep links and for the "Open with AnyPlay Launcher" entry of folders. Only one copy is the handler at a time.
_Avoid_: Default app, association

**Single instance**:
Only one launcher runs per user. A second launch hands its link to the running launcher, which comes to the front, and exits.
_Avoid_: Lock, mutex (how it is done)

### Online

The network's own words (account, handle, friend, presence, session, invitation, join intent, version guard, cloud, party, party leader, conversation, message, message number, read mark, message request) are defined in [anyplaynet/network](https://github.com/anyplaynet/network/blob/main/CONTEXT.md). The launcher adds these:

**Friends button**:
The top bar's button between the library and the settings. It opens the Friends screen on the friends list and shows how many friends are online (a dot and the number; nothing at none or while signed out).

**Friends screen**:
The full-screen page the friends button opens (and the profile screen's Friends button; the profile button opens it on sign-in while signed out): a header with who you are and how the connection is, a rail of sections (**Friends**, **Party**, **Messages**, **Activity**, **Settings**) with Your Profile below them, and the page of the section beside it. Signing in, the friends and their presence, requests, invitations, parties, messages, the feed and the online settings all live in it; Back returns to where it was opened from (home, or the profile). It replaced the drawer the online area used to be.
_Avoid_: Online area, online panel, drawer, Social, community (that is the layout gallery), friends list (one page of it)

**Section**:
One entry of the Friends screen's rail with a first page of its own. Every page belongs to one section; Back leaves the screen from the first page of a section and goes up one page elsewhere.

**Quick actions**:
The buttons at the end of a friend's row (Join, Message, Invite to Party, View Profile, Options). They rest hidden and come up with the focus or the pointer; select presses the chosen one and a click on the row's body opens Options, the friend's own page.

**Play Together**:
Starting a game that finds others on a local network so that friends can join it. The player presses Play Together on the game's page or in the party; a friend presses Join on an invitation, a friend's page or the prompt card. Only games in the LAN title table have the button. Defined in the network's glossary; the launcher only adds what it shows.
_Avoid_: Multiplayer (alone), hosting a server, LAN party

**LAN title table**:
The small list in `data/lan-titles.v1.json` of the games that can be played together, with the most players each allows. Built into every launcher; the cloud has no copy.
_Avoid_: Compatibility list, whitelist

**Join check**:
What the launcher compares before it starts a game in a friend's session, in a fixed order: signed in and online, online play on, owned, version, toolkit, place, not busy, still there. It stops at the first that does not hold and names exactly what is missing.
_Avoid_: Validation, handshake

**Prompt card**:
The glass card at the top centre that asks "Mira is playing Minecraft. Join?" and, in the same place, says why a join did not go ahead.
_Avoid_: Popup, banner, toast (a toast says what happened; the card asks or explains)


**Profile screen**:
The full-screen page of a player's profile, the player's own or another's, opened from the profile button, a friend's page or a card of the feed; Back returns to where it was opened from. A header (avatar, handle, how you know each other, a line about them), a few figures (games, hours, trophies by grade) and up to three quiet lists: games, recent trophies, recent activity. Everything is only what the player may see; a part the cloud leaves out leaves no gap and no placeholder for another player. The player's own profile reads games and trophies from this PC and adds Edit Profile (opens the account web) and Friends (the Friends screen). The launcher shows games by name and art from the player's own library only; the cloud only knows title IDs.
_Avoid_: Profile page (the side panel's old page), banner

**Profile wash**:
The soft colour in the top-left of the profile screen, taken from the avatar's pixels (`banner_colour`) or, without a picture, from its monogram. It is the profile's only colour of its own; the accent stays the UI's.
_Avoid_: Banner (alone: that is the trophy banner over a game), cover

**Feed page**:
The Feed of the network as the launcher shows it: a card for each thing the player or a friend did, newest first, each with the four reactions Nice, GG, Wow and Same. A trophy is named only when the player has the game and can read its trophy file.
_Avoid_: Timeline, wall, activity page

**Unread mark**:
The count on the profile button, and the same number on the Messages row and page: every message the player has not read, those of message requests included. It is an upper bound, since a block can hide some message numbers. Opening a conversation reads it.
_Avoid_: Badge (the code's word), notification count

**Draft**:
The text the player typed in a conversation and has not sent. The launcher keeps it per conversation until the cloud took the message, so a refusal or a closed panel loses nothing.
_Avoid_: Unsent message

**Plain text**:
How the launcher shows every message: as the characters the sender wrote, minus control and direction-changing characters. Nothing in it is interpreted: no link opens, no markup applies, nothing loads.
_Avoid_: Sanitised, escaped (those say how, not what the player sees)

**Net service**:
The program `anyplay-net` that the launcher starts as its child once the player signs in on a PC, and talks to over its standard streams. A release build of the launcher carries it inside and writes it to `Network\bin` on first use.
_Avoid_: Daemon, background service, client

**Bridge variables**:
`ANYPS5_NET_ENDPOINT` and `ANYPS5_NET_TOKEN`, which the net service hands out for one game start and the launcher adds to the game's environment. Without them the game plays offline.
_Avoid_: Network config, token (alone)

**Recording network calls**:
The setting that has a game write its observation log, the record of what it asks the network for, to `Logs\<TITLEID>-network.jsonl`. Nothing of it leaves the PC.
_Avoid_: Network log, trace, capture


### Phone as controller

**Phone server**:
The part of the launcher that a phone app talks to over the home network, started only when the player turns Phone as Controller on. It lists the library, starts games and relays the pad. It is not part of the online network and never touches the internet.
_Avoid_: Remote, remote play (that streams a picture; this does not)

**Paired phone**:
A phone the player allowed once by comparing a four-digit code on both screens. The launcher keeps its name and a secret key, and welcomes it without a question afterwards. At most eight; the player can remove each.
_Avoid_: Trusted device, registered phone

**Virtual controller**:
The controller the operating system sees while a paired phone plays: ViGEmBus on Windows, uinput on Linux. Games read it like a plugged-in pad. It exists only while a game starts or runs.
_Avoid_: Fake pad, emulated controller

**ViGEmBus**:
The Windows driver that makes a virtual controller possible. The launcher explains it and links its official download; it never installs it.
_Avoid_: Driver (alone, there are others)
