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

pub struct BeaconTarget {
    pub to: SocketAddrV4,             // 255.255.255.255:45990 by default
    pub from: Option<Vec<Ipv4Addr>>,  // None = every LAN adapter (lan_addresses), looked at every 15 s
}

pub struct LudusServer;
impl LudusServer {
    pub fn start(config: LudusConfig, events: Arc<dyn LudusEvents>) -> Result<LudusServer, RemoteError>;
    pub fn port(&self) -> u16;                 // the UDP port really bound (the beacon advertises it)
    pub fn status(&self) -> LudusStatus;
    pub fn respond(&self, request: u64, allow: bool); // the answer to a pairing question
    pub fn paired(&self) -> Vec<LudusPhone>;
    pub fn forget(&self, id: &str);            // removes the phone; if it holds the pad the pad is released
    pub fn set_pad(&self, pad: Option<Arc<dyn PadTarget>>); // the slot's pad; None = nowhere (pad live 0)
    pub fn pad_slot(&self) -> Arc<PadSlot>;    // for the browser bridge of step 2 (see "The pad slot")
    pub fn stop(&self);                        // also on drop; unplugs the pad
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
    /// Nobody has fed the pad for 1.5 s; the beacon is on.
    Waiting,
    /// A phone or a browser holds the pad.
    Linked {
        /// Who holds it.
        via: LudusVia,
        /// The layout byte of its last packet: what the controller on the phone is.
        layout: PadLayout,         // Unknown, PlayStation, Xbox, Nintendo
        /// Flags bit 0: a controller is connected to the phone. False: the pad is held at neutral.
        controller: bool,
        /// A pad target exists and its last write worked (the ack's pad-live bit).
        pad_live: bool,
    },
}

pub enum LudusVia {
    /// A phone of the LudusCtrl app, by the name it paired with.
    App { name: String },
    /// A browser client (step 2; nothing claims the pad this way yet).
    Browser { peer: IpAddr },
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
translation (`buttons & 0xF7FF`, GUIDE `0x0400` included; `xbutton::GUIDE` is new and the uinput backend now maps it to
`BTN_MODE`). Unlike the AnyPS5 Remote pad, which only plugs in while a game runs, the LudusCtrl pad is the player's only
controller and also drives the launcher: hand the server a `VirtualPad` as soon as it starts. The slot writes neutral when the
phone has no controller or the holder times out, and calls `release` (unplug) after 10 s without a holder and on stop.

## The pad slot

LudusCtrl has exactly one virtual pad, owned by one `PadSlot` inside the server. The UDP receiver (the app) and, in step 2,
the browser bridge both feed it through the slot; neither holds a `PadTarget` of its own, so Windows never shows two controllers
for LudusCtrl.

```rust
pub enum Owner { App([u8; 8]), Browser(u64) }  // the phone id, or the browser connection id
pub enum Refusal { BusyApp, Busy }             // the browser's `busy:app` and `busy`
pub struct Holding { pub owner: Owner, pub source: SocketAddr, pub layout: PadLayout, pub controller: bool, pub pad_live: bool }

impl PadSlot {
    pub fn set_target(&self, pad: Option<Arc<dyn PadTarget>>); // what LudusServer::set_pad calls
    pub fn holding(&self) -> Option<Holding>;
    pub fn claim_browser(&self, connection: u64, peer: SocketAddr, now: Instant) -> Result<(), Refusal>;
    pub fn browser_input(&self, connection: u64, state: &PadState, layout: PadLayout, controller: bool, now: Instant)
        -> Option<bool>;                        // Some(pad_live); None = not the holder any more (`evicted`)
    pub fn release(&self, owner: Owner, now: Instant) -> bool; // a closed connection, a removed phone
    pub fn tick(&self, now: Instant) -> bool;  // the server calls it every 0.25 s
    // app_input is the UDP receiver's
}
```

The rules are the protocol's: one holder; a holder silent for 1.5 s is dropped and the pad goes to neutral; between phones a new
session takes over at once and a continued one after 0.3 s of the holder's silence; a browser claim is refused with `BusyApp`
while the app is linked and with `Busy` while another browser sent within 1.5 s (a staler one is taken over); while a browser holds
the pad, the app's packets are authenticated and acked with pad-live 0 but not applied. The browser bridge itself (HTTPS,
WebSocket, the token) is step 2 and not built.

**Two servers, one pad.** The AnyPS5 Remote server opens its own `VirtualPad` while a game runs (`app::phone::attach_pad`). If
both features are on, `app::phone` must keep **one** `Arc<VirtualPad>` and hand it to LudusCtrl (`set_pad`) and to the Remote
server (`set_pad_sink`), or Windows shows two controllers. `VirtualPad` is both a `PadSink` and a `PadTarget`, so one value serves
both; whichever feeds it last wins, and only one phone plays at a time in practice. Not built yet; the glue is the UI branch's.

## Pairing question flow in the launcher

The same as the AnyPS5 Remote server's: on `pairing`, the settings open on the Phone as Controller page when nothing else is
open and no game starts or runs, otherwise a toast. The question shows `device_name` and `code`; Pair Phone calls
`respond(id, true)`, Deny `respond(id, false)`. Answer only the request id that is shown: an answer for a closed question
does nothing.

## Ownership while T55 is split

The core branch (`feat/ludusctrl`) owns `crates/anyps5-core/src/remote/ludusctrl/`, the `PadTarget` impl and `GUIDE` in
`remote/vpad`, the re-exports in `remote/mod.rs`, the preference in `settings.rs` and this file. The launcher crate (glue in `app::phone`, the
rows, strings, About, README and DESIGN.md) is the UI branch's.
