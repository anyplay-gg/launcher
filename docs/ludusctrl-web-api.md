# LudusCtrl browser client in the launcher: the core API

What `anyps5-core` offers the launcher for LudusCtrl's browser client: a phone that cannot run the LudusCtrl app
opens an HTTPS page that this PC serves on the LAN, and the controller connected to that phone plays through the page's
WebSocket. The wire protocol is LudusCtrl's `docs/PROTOCOL.md`, section "Browser client (HTTPS and WebSocket)"; this file is
the interface the launcher binds to, and the short list of what the launcher does on top of that section. It extends
[ludusctrl-api.md](ludusctrl-api.md) (the app over UDP). Everything lives in `anyps5_core::remote::ludusctrl` and the names
below are re-exported from `anyps5_core::remote`.

**Which phones.** Phones whose browser supports the Gamepad API; on iPhone, iOS 15.2 or newer. Safari before about 15.2
refuses a `wss://` connection to a self-signed certificate even after the player accepted it for the page (WebKit bug
145819). The page has run on a phone against this launcher on Windows 10, but not yet on an iPhone.

## Preference

The browser client has no preference of its own: it runs whenever the LudusCtrl option does, that is while `phone_remote` and
`phone_ludusctrl` are on, and starts and stops with the app's server. It is a method of the running `LudusServer` and feeds the
same pad slot. When the app's UDP port cannot be bound, the launcher starts the server with `LudusServer::start_without_app`
(no UDP socket, no beacon, `port()` 0), so the browser client still runs; when the browser client cannot start, the app's
server keeps running.

## Starting and stopping

```rust
pub const LUDUS_WEB_PORT: u16 = 45991; // TCP: HTTPS page and WebSocket (a busy port falls back to a free one)

pub struct BrowserConfig {
    pub cert_file: PathBuf,   // BrowserConfig::file_in(&paths): <config>/LudusCtrlWeb.json
    pub port: u16,            // LUDUS_WEB_PORT
    pub bind: Option<IpAddr>, // None = every interface (the launcher); tests bind 127.0.0.1
    // (private: the protocol's timers, shortened only by the core's tests)
}
impl BrowserConfig {
    pub fn new(cert_file: PathBuf) -> BrowserConfig; // port 45991, every interface
    pub fn file_in(paths: &AppPaths) -> PathBuf;
}

pub struct BrowserInfo {
    /// One link per LAN address, the most likely first (the same ranking and virtual-adapter filter as the
    /// Remote server's addresses): "https://192.168.1.23:45991/#t=<token>". Empty when this PC has no LAN
    /// address. The first one goes into the QR code; the others are "Other Addresses".
    pub urls: Vec<String>,
    pub port: u16,                 // the TCP port really bound
    pub preferred_port: bool,      // false: 45991 was busy (maybe the LudusCtrl app for PC), the links use `port`
    pub certificate: Certificate,  // what happened to the certificate at this start
    pub fingerprint: String,       // SHA-256 of the certificate, "AB:CD:...:EF" (32 groups), for the curious
}

pub enum Certificate {
    /// The stored certificate covers every address: a phone that accepted it before does not warn again.
    Kept,
    /// The first certificate of this PC (or the file was unreadable): every phone warns once.
    New,
    /// An address was not covered or the certificate was near its end: phones warn once more.
    Reissued,
}

impl LudusServer {
    /// Starts the HTTPS server, or restarts it (a new token) when it runs. Fails only when no TCP port can be
    /// bound or the LudusCtrl server has stopped; a busy 45991 is not a failure (see `preferred_port`).
    pub fn start_browser(&self, config: BrowserConfig) -> Result<BrowserInfo, RemoteError>;
    /// None: not running.
    pub fn browser(&self) -> Option<BrowserInfo>;
    /// New Link: a new token (the addresses are looked at again), every open page is closed with `denied` and
    /// shows that its link is old; the certificate stays. None: not running.
    pub fn new_browser_link(&self) -> Option<BrowserInfo>;
    /// Stops it: open pages are closed, a browser holding the pad lets go of it. Also done by `stop()` and on drop.
    pub fn stop_browser(&self);
}
```

All four return quickly (the stop waits at most for one 40 ms accept tick); `start_browser` reads or mints the certificate,
which takes a few milliseconds. Call them from the UI thread.

The token is new at every start and every New Link: 16 random bytes, URL-safe base64 without padding (22 characters). It
travels only in the URL fragment, so the browser never sends it in a request; no log line contains it. Restarting the launcher
voids every old link.

## Status

The browser's link is reported through the server's existing `LudusEvents::status`, as for the app:

```rust
pub enum LudusVia {
    App { name: String },
    /// A browser holds the pad. `device` is a coarse name taken from the browser's User-Agent at the upgrade:
    /// "iPhone", "iPad", "Android", "Mac", "Windows", "Linux", or None when it says nothing useful.
    Browser { peer: IpAddr, device: Option<String> },
}
```

- **Waiting** (`LudusStatus::Waiting`): nobody holds the pad. With the browser client running and no app linked, this is
  "waiting for a browser".
- **Connected** (`LudusStatus::Linked { via: LudusVia::Browser { peer, device }, layout, controller, pad_live }`): a page
  authenticated and claimed the pad. `controller` is false until the page sees a standard-mapping pad (flags bit 0); `layout`
  is the page's layout byte.
- The status changes within 0.25 s of a claim, a release or the 1.5 s watchdog.
- **Certificate reissued**: `BrowserInfo::certificate == Certificate::Reissued`. **Port busy**: `!preferred_port`.

There is no delay figure on the PC: the page measures the round trip (`ping:`/`pong:`) and shows it itself.

The launcher meets the browser server in two places: `app::ludus_web` (the `BrowserBridge` trait, which every
`LudusReceiver` is, `BrowserState` and `config_for_launcher`) and `impl BrowserBridge for LudusServer` in `app/ludus.rs`, the
one place the core plugs in. In the same file, `event_of` turns `LudusVia::Browser { peer, device }` into the launcher's
`LudusEvent::Browser`, which names the browser in the status row and the top-bar pill.

## Certificate

- ECDSA P-256 with SHA-256 (not the RSA 2048 of LudusCtrl's own PC app: `ring` cannot make RSA keys and the RustCrypto `rsa`
  crate carries an advisory `cargo deny` refuses). Minted by `rcgen` on `ring`.
- Valid 825 days (Apple's ceiling for TLS server certificates), from one day before it is made.
- SANs: every LAN IPv4 address, `127.0.0.1` and `localhost`. CN "AnyPlay Launcher". `CA:false`, EKU serverAuth, key usage
  digitalSignature, a random serial.
- Stored in `<config>/LudusCtrlWeb.json` (temp file and rename, mode 0600 on Linux):
  `{ "cert": "<base64 DER>", "key": "<base64 PKCS#8>", "addresses": ["192.168.1.23", ...], "expires": <unix seconds> }`.
- Reissued only when the file is missing or unreadable, the key does not match the certificate, it ends within 30 days, or a
  current LAN address is not covered. The key is kept across a reissue, and the new address list is the union of the current and
  the old ones (at most 32, the current first), so a PC that moves between Wi-Fi and Ethernet does not alternate between two
  certificates. A config folder that cannot be written still serves a certificate (a new one every start; logged).

## Serving the page

The page is LudusCtrl's own browser page in the launcher's colours, one file with its script and style inline:
`crates/anyps5-core/assets/ludusctrl-web/index.html`, built into the binary with `include_bytes!` and checked out with LF
endings (`.gitattributes`), because its CSP hashes are taken from its bytes.

| Request | Answer |
|---|---|
| `GET /`, `GET /index.html` (a query is ignored) | `200`, the page, `text/html; charset=utf-8` |
| `GET /ws` | the WebSocket (`101`), or `400`/`403` (below) |
| anything else, any other method | `404` |
| a head that does not parse, or over 8 KiB | `400` |

Every answer has `Cache-Control: no-store`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer` and
`Connection: close` (one request per connection). The page has

```
Content-Security-Policy: default-src 'none'; script-src 'sha256-<script>'; style-src 'sha256-<style>'; img-src data:;
  connect-src 'self' wss://<Host>; base-uri 'none'; form-action 'none'; frame-ancestors 'none'
```

The hashes are computed at start from the embedded bytes (one per inline block; blocks with `src=` are skipped) and written
down nowhere, so a new page needs no server change. There is no `'unsafe-inline'`. A test compares the served CSP with hashes of
the served body. For the page: no `on...=` or `style="..."` attributes (the CSP blocks them); `__LUDUSCTRL_VERSION__` stays
as it is.

## The WebSocket, as the launcher answers it

As PROTOCOL.md:

- Upgrade: `Upgrade: websocket` and `Sec-WebSocket-Key` are required, otherwise `400`. An `Origin` header, when present, must
  equal `https://` + `Host` (case-insensitive), otherwise `403`. No `Origin`: on to the token.
- First frame `auth:<token>` (the token from the link's `#t=`) within 5 s, compared in constant time. Wrong, or any other first
  frame: `denied`, close 1008 "bad token". Nothing in 5 s: close 1008 "no token".
- Claim: the app linked over UDP: `busy:app`, close 1000. Another browser sent a packet within 1.5 s: `busy`, close 1000. A
  staler holder is taken over.
- Success: `ready`, then `padkind:x360`.
- Binary, exactly 20 bytes, `D5 01`: applied (`buttons & 0xF7FF`; flags bit 0 clear holds the pad at neutral). Acked with
  `ack:<seq>:<received>:<applied>` at most every 200 ms per connection; `received` counts every packet the bridge applied since
  it started, `applied` is the pad-live bit. Any other binary: ignored, no reply.
- Any rate is accepted (the page sends one packet per animation frame: 234 a second on a 240 Hz screen). Packets that arrive
  together after a stall are coalesced: of a run that is already in the buffer, only the newest is written to the pad.
- `ping:<id>`: `pong:<id>` (the first 59 bytes of the id, cut at a character boundary). A WebSocket ping: a pong with the same
  payload. A close: its code is echoed and the session ends. Other text: ignored.
- `pad:x360`: `padkind:x360`. `pad:ds4`: `paderr:this PC has only the Xbox 360 pad` (the protocol's "why the new pad could not
  be created"; the launcher has one Xbox 360 virtual pad, which stays). Any other kind: `paderr:unknown pad kind`. At most one
  switch every 2 s: `paderr:wait a moment before switching again`. The page's picker offers Xbox 360 only.
- Eviction: a holder with no valid packet for 1.5 s loses the pad (neutral). The connection learns it on its next frame
  (`evicted`, close 1000) or at the 5 s read deadline (`evicted`, close 1001 "idle"). Still holding at the deadline: close 1001
  "idle".
- The frames are parsed by the bridge itself, not by a WebSocket library, so the rules are the protocol's exactly: 1002 for
  reserved bits, a fragment, a continuation, an unknown opcode, a control frame over 125 bytes or an unmasked frame; 1009 for a
  frame over 4096 bytes (known from its header, before the payload arrives).
- After its own close frame the server reads for up to 1 s for the peer's close, then sends TLS close_notify.
- At most 16 connections at a time (page loads included); the 17th is closed before TLS. A TLS handshake has 5 s, a request
  head 10 s.

## What the launcher adds to PROTOCOL.md

Nothing the page can see. On the PC side:

1. **ECDSA P-256 certificate**, not RSA 2048 (see Certificate). Browsers accept both the same way; the iPhone warning flow with
   it still has to be tried on a real device (iOS 15.2 or newer, above).
2. **New Link and stop** are the launcher's: New Link closes every open page with `denied` and 1008 "bad token", which the page
   reads as an old link; stop closes them with 1001 "stopping", and the page's reconnect then finds no server.
3. **Stricter peers**: a connection must come from a private address on a network this PC is plugged into (the launcher's rule
   of DESIGN.md 18: not carrier-grade NAT, not routed, not a VPN). Anything else is dropped before TLS.
4. **`SO_EXCLUSIVEADDRUSE`** on Windows and no `SO_REUSEADDR`, so no other program can bind over the port.

## Testing by hand

`fake-phone browser --url "<link>" --press cross+r1 --seconds 3` (the test helper crate, never shipped) opens the link the
launcher shows, skips certificate verification (it is a test tool), authenticates with the token in the fragment, sends one
packet every 16 ms with those buttons and a ping every second, prints everything the PC sends, and ends with a neutral packet
and a close. Button names: up, down, left, right, options, create, l3, r3, l1, r1, ps, cross, circle, square, triangle.
