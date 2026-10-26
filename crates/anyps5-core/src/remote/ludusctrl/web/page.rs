//! The browser client's page, built into the binary: `assets/ludusctrl-web/index.html`, one file
//! with its script and style inline. The Content-Security-Policy pins each inline block by its
//! SHA-256, taken from these bytes, so nothing else can run in the page.

use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};

pub const INDEX: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/ludusctrl-web/index.html"
));

/// The contents of every inline `<tag>` block (no `src`), in order.
pub fn inline_blocks<'a>(html: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let lower = html.to_ascii_lowercase();
    let mut blocks = Vec::new();
    let mut from = 0;
    while let Some(start) = lower[from..].find(&open).map(|i| i + from) {
        let after = start + open.len();
        // `<scriptx` is another tag.
        if !lower[after..].starts_with(['>', ' ', '\t', '\n', '\r']) {
            from = after;
            continue;
        }
        let Some(head_end) = lower[after..].find('>').map(|i| i + after + 1) else {
            break;
        };
        let Some(end) = lower[head_end..].find(&close).map(|i| i + head_end) else {
            break;
        };
        if !lower[start..head_end].contains("src=") {
            blocks.push(&html[head_end..end]);
        }
        from = end + close.len();
    }
    blocks
}

fn hashes(html: &str, tag: &str) -> String {
    inline_blocks(html, tag)
        .iter()
        .map(|block| {
            format!(
                "'sha256-{}'",
                STANDARD.encode(Sha256::digest(block.as_bytes()))
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The policy parts that depend only on the page: `(script-src, style-src)` sources.
pub fn sources() -> (String, String) {
    let html = String::from_utf8_lossy(INDEX);
    (hashes(&html, "script"), hashes(&html, "style"))
}

/// Whether a `Host` header can go into the policy as it is: a name or an address with a port,
/// IPv6 in brackets (`[A-Za-z0-9.:\[\]-]+`), so nothing that could end a source or a directive.
pub fn is_plain_host(host: &str) -> bool {
    !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b':' | b'[' | b']' | b'-'))
}

/// The page's Content-Security-Policy for a request to `host`. A host that is not plain leaves
/// `connect-src` at `'self'` alone.
pub fn csp(sources: &(String, String), host: &str) -> String {
    let (script, style) = sources;
    let connect = if is_plain_host(host) {
        format!("'self' wss://{host}")
    } else {
        "'self'".to_string()
    };
    format!(
        "default-src 'none'; script-src {script}; style-src {style}; img-src data:; connect-src {connect}; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
    )
}

/// Whether a request path is the page.
pub fn is_page(path: &str) -> bool {
    matches!(path, "/" | "/index.html")
}
