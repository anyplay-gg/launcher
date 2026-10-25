//! Embeds the application icon and the version information into the Windows executable (the Windows
//! host builds it; embed-resource is a Windows-only build dependency).

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    embed_net_service();
    embed_share_worker();
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
        let source = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").expect("manifest"))
            .join("assets/icon.ico");
        // Next to the script, so a source path with an apostrophe cannot break windres.
        let icon = out.join("icon.ico");
        std::fs::copy(&source, &icon).expect("icon must be copyable");
        let script = resource_script(&icon.to_string_lossy().replace('\\', "/"));
        let rc = out.join("launcher.rc");
        std::fs::write(&rc, script).expect("resource script must be writable");
        embed_resource::compile(&rc, embed_resource::NONE)
            .manifest_optional()
            .expect("icon resource must compile");
    }
}

/// A release build carries the net service (`anyplay-net` from anyplay-gg/network) inside the
/// launcher, so every download and every self-update brings the service that matches it.
/// `ANYPLAY_NET_EMBED` names the built service; without it the launcher looks for one beside
/// itself.
fn embed_net_service() {
    println!("cargo::rustc-check-cfg=cfg(embedded_net)");
    println!("cargo:rerun-if-env-changed=ANYPLAY_NET_EMBED");
    let Some(source) = std::env::var_os("ANYPLAY_NET_EMBED") else {
        return;
    };
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    std::fs::copy(&source, out.join("anyplay-net.bin"))
        .expect("ANYPLAY_NET_EMBED must be readable");
    println!(
        "cargo:rerun-if-changed={}",
        std::path::Path::new(&source).display()
    );
    println!("cargo:rustc-cfg=embedded_net");
}

/// A release build for Windows also carries the share worker (`anyplay-share` from
/// anyplay-gg/network), which streams and shows a game's window for share screen.
/// `ANYPLAY_SHARE_EMBED` names the built worker; without it the launcher looks for one beside
/// itself, and share screen stays off where there is none.
fn embed_share_worker() {
    println!("cargo::rustc-check-cfg=cfg(embedded_share)");
    println!("cargo:rerun-if-env-changed=ANYPLAY_SHARE_EMBED");
    let Some(source) = std::env::var_os("ANYPLAY_SHARE_EMBED") else {
        return;
    };
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    std::fs::copy(&source, out.join("anyplay-share.bin"))
        .expect("ANYPLAY_SHARE_EMBED must be readable");
    println!(
        "cargo:rerun-if-changed={}",
        std::path::Path::new(&source).display()
    );
    println!("cargo:rustc-cfg=embedded_share");
}

#[cfg(windows)]
/// The icon plus a VERSIONINFO block (Explorer's Details tab, the installer's checks).
fn resource_script(icon: &str) -> String {
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let mut parts = version
        .split(['.', '-', '+'])
        .map(|p| p.parse::<u16>().unwrap_or(0));
    let numbers: Vec<u16> = (0..4).map(|_| parts.next().unwrap_or(0)).collect();
    let comma = format!(
        "{},{},{},{}",
        numbers[0], numbers[1], numbers[2], numbers[3]
    );
    format!(
        r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {comma}
PRODUCTVERSION {comma}
FILEOS 0x40004
FILETYPE 0x1
{{
    BLOCK "StringFileInfo"
    {{
        BLOCK "040904B0"
        {{
            VALUE "CompanyName", "AnyPlay Launcher contributors"
            VALUE "FileDescription", "AnyPlay Launcher"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "anyplay-launcher"
            VALUE "LegalCopyright", "GPL-2.0-only"
            VALUE "OriginalFilename", "anyplay-launcher.exe"
            VALUE "ProductName", "AnyPlay Launcher"
            VALUE "ProductVersion", "{version}"
        }}
    }}
    BLOCK "VarFileInfo"
    {{
        VALUE "Translation", 0x409, 1200
    }}
}}
"#
    )
}
