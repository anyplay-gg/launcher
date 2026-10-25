#!/bin/bash
# Builds a Finder-launchable app and zip from a compiled macOS launcher.
# Usage: package.sh <version> <launcher binary> <third-party licences> <output folder>
set -euo pipefail

version=${1:?version}
binary=$(cd "$(dirname "${2:?launcher binary}")" && pwd)/$(basename "$2")
notices=$(cd "$(dirname "${3:?third-party licences}")" && pwd)/$(basename "$3")
mkdir -p "${4:?output folder}"
out=$(cd "$4" && pwd)
root=$(cd "$(dirname "$0")/../.." && pwd)

case "$version" in
  ''|*[!0-9.]*) echo "version must contain only digits and dots" >&2; exit 1 ;;
esac
[[ $(uname -s) == Darwin ]] || { echo "macOS packaging requires macOS" >&2; exit 1; }
[[ -x "$binary" && -f "$notices" ]] || { echo "binary or licences missing" >&2; exit 1; }
file "$binary" | grep -q 'Mach-O' || { echo "launcher must be a Mach-O executable" >&2; exit 1; }
architectures=$(lipo -archs "$binary")
case "$architectures" in
  arm64) arch=arm64 ;;
  x86_64) arch=x86_64 ;;
  'x86_64 arm64'|'arm64 x86_64') arch=universal ;;
  *) echo "unsupported launcher architectures: $architectures" >&2; exit 1 ;;
esac

app="$out/AnyPlay Launcher.app"
archive="$out/AnyPlay-Launcher-$version-macos-$arch.zip"
[[ ! -e "$app" && ! -e "$archive" ]] || { echo "packaging destination already exists" >&2; exit 1; }
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
bundle="$work/AnyPlay Launcher.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
install -m 755 "$binary" "$bundle/Contents/MacOS/anyplay-launcher"
install -m 644 "$root/LICENSE" "$notices" "$bundle/Contents/Resources/"

# The same icon as Linux, converted with the macOS SDK tools.
iconset="$work/AnyPlay.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$root/tools/linux/anyplay-launcher.png" \
    --out "$iconset/icon_${size}x${size}.png" >/dev/null
  retina=$((size * 2))
  sips -z "$retina" "$retina" "$root/tools/linux/anyplay-launcher.png" \
    --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$bundle/Contents/Resources/AnyPlay.icns"
cat > "$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>AnyPlay Launcher</string>
  <key>CFBundleDisplayName</key><string>AnyPlay Launcher</string>
  <key>CFBundleIdentifier</key><string>gg.anyplay.launcher</string>
  <key>CFBundleExecutable</key><string>anyplay-launcher</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleIconFile</key><string>AnyPlay</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.games</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>Use your microphone for party voice when you enable it.</string>
</dict></plist>
PLIST
plutil -lint "$bundle/Contents/Info.plist"
codesign --force --sign - "$bundle"
codesign --verify --strict "$bundle"
ditto "$bundle" "$app"
ditto -c -k --norsrc --noextattr --keepParent "$app" "$archive"
echo "$app"
echo "$archive"
