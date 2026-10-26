# LudusCtrl browser client in the launcher: the core API

What `anyps5-core` offers the launcher for LudusCtrl's browser client: a phone that cannot run the LudusCtrl app (an iPhone)
opens an HTTPS page that this PC serves on the LAN, and the controller connected to that phone plays through the page's
WebSocket. The wire protocol is LudusCtrl's `docs/PROTOCOL.md`, section "Browser client (HTTPS and WebSocket)"; this file is
the interface the launcher and the page bind to, and the list of the places where the launcher differs from that section.
It extends [ludusctrl-api.md](ludusctrl-api.md) (the app over UDP). Everything lives in `anyps5_core::remote::ludusctrl` and the
names below are re-exported from `anyps5_core::remote`.

## Preference

The browser client is an option of the LudusCtrl option: it runs only while `phone_remote`, `phone_ludusctrl` and its own
preference (the UI branch's, for example `phone_ludusctrl_browser`, serde default `false`) are on. It is a method of the running
`LudusServer`, so it cannot run without it, and it feeds the same pad slot.

## Starting and stopping

```rust
pub const LUDUS_WEB_PORT: u16 = 45991; // TCP: HTTPS page and WebSocket (a busy port falls back to a free one)

pub struct BrowserConfig {
    pub cert_file: PathBuf,   // BrowserConfig::file_in(&paths): <config>/LudusCtrlWeb.json
    pub port: u16,            // LUDUS_WEB_PORT
    pub language: String,     // the page's language: "en" or "tr" (anything else is served as "en")
    pub bind: Option<IpAddr>, // None = every interface (the launcher); tests bind 127.0.0.1
}
impl BrowserConfig {
    pub fn new(cert_file: PathBuf) -> BrowserConfig; // port 45991, "en", every interface
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

All four return at once (the stop waits at most for one 40 ms accept tick). Call them from the UI thread.

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

The launcher-side test `the_core_status_becomes_the_launchers_event` (in `app/ludus.rs`) builds `LudusVia::Browser` and gains
the `device: None` field on this branch; nothing else of the launcher crate is touched.

## Certificate

- ECDSA P-256 with SHA-256 (not the RSA 2048 of LudusCtrl's own PC app: `ring` cannot make RSA keys and the RustCrypto `rsa`
  crate carries an advisory `cargo deny` refuses). Minted by `rcgen` on `ring`.
- Valid 825 days (Apple's ceiling for TLS server certificates), from one day before it is made.
- SANs: every LAN IPv4 address, `127.0.0.1` and `localhost`. CN "AnyPlay Launcher". `CA:false`, EKU serverAuth, key usage
  digitalSignature, a random serial.
- Stored in `<config>/LudusCtrlWeb.json` (temp file and rename, mode 0600 on Linux):
  `{ "cert": "<base64 DER>", "key": "<base64 PKCS#8>", "addresses": ["192.168.1.23", ...], "expires": <unix seconds> }`.
- Reissued only when the file is missing or unreadable, the key does not match the certificate, it ends within 30 days, or a
  current LAN address is not covered. The key is kept across a reissue, and the new address list is the union of the old and the
  current ones (at most 32, the newest first), so a PC that moves between Wi-Fi and Ethernet does not alternate between two
  certificates. A config folder that cannot be written still serves a certificate (a new one every start; logged).

## Serving the page

The page is the files of `crates/anyps5-core/assets/ludusctrl-web/`, built into the binary with `include_bytes!`:

| Path | File | Content-Type |
|---|---|---|
| `GET /`, `GET /index.html` | `index.html` | `text/html; charset=utf-8` |
| `GET /app.js` | `app.js` | `text/javascript; charset=utf-8` |
| `GET /app.css` | `app.css` | `text/css; charset=utf-8` |
| `GET /ws` with `Upgrade: websocket` | the WebSocket | `101` |
| any other path | | `404` |
| any other method | | `405` |

Every response has `Cache-Control: no-store`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer` and
`Connection: close` (one request per connection). The page has

```
Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:;
  connect-src 'self' wss://<Host>; base-uri 'none'; form-action 'none'; frame-ancestors 'none'
```

**What the page must do to work under that** (for the page branch, `feat/ludusctrl-web-page`):

- Load its script and style as `<script src="app.js"></script>` (or `defer`) and `<link rel="stylesheet" href="app.css">`.
  No inline `<script>` or `<style>`, no `on...=` attributes, no `style="..."` attributes in the markup and no
  `setAttribute("style", ...)`. Setting `element.style.foo` from the script is allowed. Images only as files of the folder
  (not served, so in practice none) or `data:` URLs. No fonts, no other origins.
- Write the root element exactly as `<html lang="en">`: the server replaces `en` there with the launcher's language (`tr`), so
  the script can pick its strings from `document.documentElement.lang`.
- Connect to `wss://${location.host}/ws` (the same host and port as the page).
- Only these three files are served; anything else in the folder is ignored.

Until the page branch lands, the folder holds a placeholder page; the page branch replaces the three files wholesale.

## The WebSocket, as the launcher answers it

As PROTOCOL.md, with the launcher's values:

- Upgrade: `Upgrade: websocket` and `Sec-WebSocket-Key` are required, otherwise `400`. An `Origin` header, when present, must
  equal `https://` + `Host` (case-insensitive), otherwise `403`.
- First frame `auth:<token>` within 5 s, compared in constant time. Wrong: `denied`, close 1008 "bad token". Nothing in 5 s:
  close 1008 "no token".
- Claim: the app linked over UDP: `busy:app`, close 1000. Another browser sent a packet within 1.5 s: `busy`, close 1000. A
  staler holder is taken over.
- Success: `ready`, then `padkind:x360`.
- Binary, exactly 20 bytes, `D5 01`: applied (`buttons & 0xF7FF`; flags bit 0 clear holds the pad at neutral). Acked with
  `ack:<seq>:<received>:<applied>` at most every 200 ms per connection; `received` counts every packet the bridge applied since
  it started, `applied` is the pad-live bit. Any other binary: ignored.
- `ping:<id>`: `pong:<id>` (the first 59 bytes of the id, cut at a character boundary). WebSocket pings are answered by the
  library.
- `pad:x360`: `padkind:x360`. `pad:<anything else>`: `paderr:unknown pad kind`. The launcher's virtual pad is Xbox 360 only,
  so the page should not offer a switch.
- Eviction: a holder with no valid packet for 1.5 s loses the pad (neutral). The connection learns it on its next frame
  (`evicted`, close 1000) or after 5 s without any frame (`evicted`, close 1001 "idle").
- Errors from the WebSocket layer: 1002 for a protocol error (reserved bits, a continuation without a start, an unknown opcode,
  a fragmented or over-125-byte control frame, an unmasked frame), 1009 for a frame or message over 4096 bytes.
- After its own close frame the server reads for up to 1 s for the peer's close, then sends TLS close_notify.
- At most 16 connections at a time (page loads included); the 17th is closed before TLS. A TLS handshake has 5 s, a request
  head 10 s (and at most 8 KiB).

## Where the launcher differs from PROTOCOL.md

1. **ECDSA P-256 certificate**, not RSA 2048 (see Certificate).
2. **Three files, not one**: the page's script and style are served as `/app.js` and `/app.css` and the CSP allows `'self'`
   for them, instead of one page with inline script and style pinned by hash. Same origin over the same pinned TLS connection,
   so it allows nothing the hashes would not.
3. **Fragmented messages are reassembled** (up to 4096 bytes in all) by tungstenite instead of being closed with 1002. Browsers
   never fragment messages this small, and the page sends none.
4. **Invalid UTF-8 in a text frame** closes with 1007 (the RFC's code; PROTOCOL.md does not name one).
5. **No pad switch**: only `x360` exists, so `pad:ds4` gets `paderr:unknown pad kind` and there is no 2 s switch interval.
6. **New Link and stop** are the launcher's: New Link closes every open page with `denied` and 1008 "bad token" (the page
   reads it as an old link); stop closes them with 1001 "stopping".
7. **Stricter peers**: a connection must come from a private address on a network this PC is plugged into (the launcher's rule
   of DESIGN.md 18: not carrier-grade NAT, not routed, not a VPN). The `Host` of every request must be an address of this PC or
   `localhost`, otherwise `403` (DNS rebinding).
8. **`SO_EXCLUSIVEADDRUSE`** on Windows and no `SO_REUSEADDR`, so no other program can bind over the port.

## Testing by hand

`fake-phone browser <url>` (the test helper crate, never shipped) opens the link the launcher shows, skips certificate
verification (it is a test tool), authenticates with the token in the fragment and sends one packet every 16 ms for a few
seconds with the buttons it is given, printing everything the PC sends.
