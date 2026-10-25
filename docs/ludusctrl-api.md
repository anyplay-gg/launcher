# LudusCtrl in the launcher: the core API

What `anyps5-core` offers the launcher for LudusCtrl (the Android app that turns a controller connected to the phone into a
controller of this PC). The wire protocol is LudusCtrl's `docs/PROTOCOL.md` (DSLINK v2); this file is only the interface the
launcher binds to. Everything lives in `anyps5_core::remote::ludusctrl` and the names below are re-exported from
`anyps5_core::remote`.

## Preference

`Preferences::phone_ludusctrl: bool` (serde default `false`). The server runs only while `phone_remote` **and**
`phone_ludusctrl` are on: LudusCtrl is an option of Phone as Controller.

## Starting and stopping

```rust
pub const LUDUS_PORT: u16 = 45989;        // UDP: pairing, input, acks (a busy port falls back to a free one)
pub const LUDUS_BEACON_PORT: u16 = 45990; // UDP broadcast: the beacon

pub struct LudusConfig {
    pub store_file: PathBuf,          // LudusStore::file_in(&paths): <config>/LudusCtrl.json
    pub port: u16,                    // LUDUS_PORT
    pub beacon: Option<BeaconTarget>, // Some(BeaconTarget::default()) = broadcast to 255.255.255.255:45990; None = no beacon
    pub allow_all: bool,              // pair without asking (tests only)
}
impl LudusConfig { pub fn new(store_file: PathBuf) -> LudusConfig }

pub struct LudusServer;
impl LudusServer {
    pub fn start(config: LudusConfig, events: Arc<dyn LudusEvents>) -> Result<LudusServer, RemoteError>;
    pub fn port(&self) -> u16;                 // the UDP port really bound (the beacon advertises it)
    pub fn status(&self) -> LudusStatus;
    pub fn respond(&self, request: u64, allow: bool); // the answer to a pairing question
    pub fn paired(&self) -> Vec<LudusPhone>;
    pub fn forget(&self, id: &str);            // removes the phone; if it holds the pad the pad is released
    pub fn set_pad(&self, pad: Option<Arc<dyn PadTarget>>); // where input goes; None = nowhere (pad live 0)
    pub fn stop(&self);                        // also on drop; releases the pad
}
```

`LudusStore::file_in(paths: &AppPaths) -> PathBuf` names the file. It is the launcher's own format (not LudusCtrl's
`pairing.json`): `{ "pc_id": "<16 hex>", "phones": [{ "added", "id", "key", "name", "highest_session" }] }`, written through a
temp file and a rename, mode 0600 on Linux, like `RemoteDevices.json`.

## Events

Called from the server's thread: enqueue and wake the UI, never block (the same rule as `RemoteEvents`).

```rust
pub trait LudusEvents: Send + Sync {
    /// A phone revealed its nonce: ask the player. `request.code` is "260 329" (two groups of three digits),
    /// `request.device_name` the sanitised name ("Unnamed phone" when it sent none). The same
    /// `PairingRequest` type as the AnyPS5 Remote server.
    fn pairing(&self, request: PairingRequest);
    /// The question is obsolete: answered, 60 s without an answer, or the phone left.
    fn pairing_ended(&self, id: u64);
    /// The link changed (see LudusStatus).
    fn status(&self, status: LudusStatus);
}

pub enum LudusStatus {
    /// No phone has sent an accepted packet for 1.5 s; the beacon is on.
    Waiting,
    /// A paired phone holds the pad.
    Linked {
        name: String,
        /// The layout byte of its last packet: what the controller on the phone is.
        layout: PadLayout,         // Unknown, PlayStation, Xbox, Nintendo
        /// Flags bit 0: a controller is connected to the phone. False: the pad is held at neutral.
        controller: bool,
        /// A pad target exists and its last write worked (the ack's pad-live bit).
        pad_live: bool,
    },
}

pub struct LudusPhone {
    pub id: String,     // 16 lowercase hex digits (the 8-byte phone_id)
    pub name: String,
    pub added: String,  // RFC 3339, UTC, like PairedDevice::added
}
```

There is no delay figure: the protocol has no message that carries one to the PC (the phone measures the round trip and
keeps it). The settings show none for LudusCtrl.

At most `MAX_DEVICES` (8) phones. An Allow while eight others are paired finishes the attempt as denied; the launcher shows
its "Eight phones are paired" toast as for the other server.

## The virtual pad

```rust
pub trait PadTarget: Send + Sync {
    /// One complete XInput state; false when the controller could not take it.
    fn write(&self, state: &PadState) -> bool;
    /// Unplug: nothing more comes for a while.
    fn release(&self);
}
impl PadTarget for VirtualPad
```

LudusCtrl's buttons are XInput's `wButtons` and its sticks are signed with up positive, so `PadState` is filled without
translation (`buttons & 0xF7FF`, GUIDE `0x0400` included). Unlike the AnyPS5 Remote pad, which only plugs in while a game
runs, the LudusCtrl pad is the player's only controller and also drives the launcher: hand the server a `VirtualPad` as soon as
it starts. The server writes neutral when a phone has no controller or the link times out, and calls `release` (unplug) after
10 s without a linked phone and on stop.

## Pairing question flow in the launcher

The same as the AnyPS5 Remote server's: on `pairing`, the settings open on the Phone as Controller page when nothing else is
open and no game starts or runs, otherwise a toast. The question shows `device_name` and `code`; Pair Phone calls
`respond(id, true)`, Deny `respond(id, false)`. Answer only the request id that is shown: an answer for a closed question
does nothing.

## Ownership while T55 is split

The core branch (`feat/ludusctrl`) owns `crates/anyps5-core/src/remote/ludusctrl/`, the `PadTarget` impl in `remote/vpad`, the
re-exports in `remote/mod.rs`, the preference in `settings.rs` and this file. The launcher crate (glue in `app::phone`, the
rows, strings, About, README and DESIGN.md) is the UI branch's.
