#!/usr/bin/env bash
# Writes the licence file of a release: the licences of every third-party crate in the launcher
# and in the net service and share worker it carries (about.toml here and in anyplaynet/network),
# then Cisco's licence for the OpenH264 binary the launcher downloads on Linux.
#
#   notices.sh <output file>
#
# Run it from a launcher checkout with anyplaynet/network beside it. cargo-about is a pinned
# download, checked against its SHA-256; to update it, change both lines together. CARGO_ABOUT
# names a cargo-about binary to use instead, for a machine that is not x86-64 Linux.
set -euo pipefail

CARGO_ABOUT_VERSION=0.9.2
mkdir -p "$(dirname "${1:?output file}")"
out=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
network=$(cd "$root/../network" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

about=${CARGO_ABOUT:-}
if [ -z "$about" ]; then
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64)
      target=x86_64-unknown-linux-musl
      checksum=9099a59e820c38a68b9d65f300662a567d56562f9a10f6aa4c7e86c17c2566af ;;
    Darwin-arm64)
      target=aarch64-apple-darwin
      checksum=ae72f0df0c399a1e96336f696fa55b1b28679fd725632eba8cf8e4568467cc3e ;;
    *) echo "Set CARGO_ABOUT to a native cargo-about $CARGO_ABOUT_VERSION binary" >&2; exit 1 ;;
  esac
  name="cargo-about-$CARGO_ABOUT_VERSION-$target"
  curl -fsSL --retry 3 -o "$work/about.tar.gz" \
    "https://github.com/EmbarkStudios/cargo-about/releases/download/$CARGO_ABOUT_VERSION/$name.tar.gz"
  if command -v sha256sum >/dev/null; then
    echo "$checksum  $work/about.tar.gz" | sha256sum -c -
  else
    echo "$checksum  $work/about.tar.gz" | shasum -a 256 -c -
  fi
  tar -C "$work" -xzf "$work/about.tar.gz"
  about="$work/$name/cargo-about"
fi

# --fail stops on a crate whose licence cannot be read: a release never ships an incomplete file.
"$about" generate --format json --locked --fail --workspace \
  -c "$root/about.toml" -m "$root/Cargo.toml" -o "$work/launcher.json"
"$about" generate --format json --locked --fail \
  -c "$network/about.toml" -m "$network/crates/net/Cargo.toml" -o "$work/net.json"
"$about" generate --format json --locked --fail \
  -c "$network/about.toml" -m "$network/crates/share-worker/Cargo.toml" -o "$work/share.json"
mkdir -p "$(dirname "$out")"
node "$here/notices.mjs" "$out" "$work/launcher.json" "$work/net.json" "$work/share.json"
# Not part of the release: Cisco's own binary, downloaded to the player's PC when they turn the
# codec on. Cisco's licence asks for its full text where licences are shown.
{
  printf '\n%s\n%s\n\n' "Cisco's OpenH264 binary (Linux, downloaded from Cisco when the player turns" \
    "Share Screen Video Codec on in Settings, Advanced):"
  cat "$here/openh264/BINARY_LICENSE.txt"
} >> "$out"
