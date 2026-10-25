# Online play

The online area is built into the launcher. It needs an AnyPlay account and a cloud; release builds use AnyPlay's own at `https://anyplaynet.com`.

## What works

Select your profile at the top of the home screen to open **Online**.

- **Account.** Sign in or create an account: both open the cloud's page in your browser, where you can use a passkey or an e-mail code, and the launcher takes you in when you are done.
- **Friends.** Add friends by their name, see what they play, accept requests and invitations, and join a friend's game when it is open to friends.
- **Parties and calls.** Start a party, invite friends, send messages and talk with party voice. Call an online friend from their page. Voice runs on AnyPlay or on Discord; on Discord the launcher never opens your microphone. On Windows you can share your screen with the party.
- **Profile and feed.** Your profile shows your picture, a line about you, up to three favourite games, trophies, most played and linked accounts. The feed lists what you and your friends did lately, and you answer with Nice, GG, Wow or Same. There are no comments.
- **Join codes and Discord.** A join code lets someone who is not your friend ask to join your game. With *Show Game on Discord* on, Discord friends get a **Join on AnyPlay** button; it only asks you and never joins by itself.
- **Trophies.** They go to the cloud as records: number, grade and time of each, while names and pictures stay on your PC. *Who Sees Your Trophies* on the Account page decides who sees them (friends by default).
- **Hide your address.** *Hide Your IP Address* keeps every tunnel on a relay, so other players never learn your address.
- **Restrictions.** When the cloud restricts an account, the launcher shows what, until when and how to appeal.
- **Run reports.** After you play a game while signed in, the game's page asks once how it ran. The answers make up the public list at [anyplaynet.com/compatibility](https://anyplaynet.com/compatibility).

The launcher, the net service and the cloud are tested together, including two net services with fake games that exchange packets through a tunnel, directly and through a relay (the end-to-end tests in [anyplaynet/cloud](https://github.com/anyplaynet/cloud)).

## What does not work yet

- No real game plays online yet. The bridge that connects a game to the network lives in an AnyPS5 build, and it is still under review. Until a toolkit carries it, games start as before, offline. The [known gaps](https://github.com/anyplaynet/network/blob/main/docs/gaps.md) list what is missing.
- Starting a game with the bridge is untested on Linux.

## Use another cloud

Under **Online → Cloud** you can use a cloud of your own or of friends instead. [anyplaynet/cloud](https://github.com/anyplaynet/cloud) explains how to host one with `docker compose up`.

## What to expect later

A game plays online only with a build that carries the bridge, and only when its online part uses the platform's own services. A game whose online play runs on a publisher's live servers stays offline: the network never stands in for a service that still runs. The [network's design](https://github.com/anyplaynet/network/blob/main/docs/DESIGN.md) has the details and the roadmap.

## Privacy

- The launcher contacts the network only after you sign in. Until then, nothing goes online.
- *Record Network Calls* (Settings, Advanced) makes a game write what it asks the network for into the Logs folder. It is off by default and nothing is uploaded.
- The net service writes its log to `Logs\network.log`.
- A run report sends only the game's title ID and version, the toolkit version, your answer and your system. Nothing about your PC or your games' files.
