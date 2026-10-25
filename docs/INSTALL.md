# Install and first start

Get the latest version from the [releases page](https://github.com/anyplaynet/launcher/releases/latest). You need your own decrypted game dumps; the launcher includes no game files, keys or firmware.

## Windows

64-bit Windows 10 or 11, nothing else to install first.

- `AnyPlay-Launcher-<version>-Setup.exe` installs the launcher for your user account, without administrator rights. It adds a Start menu entry, an optional desktop shortcut and an uninstaller.
- `AnyPlay-Launcher-<version>-x64.exe` is the same launcher as a single file. Put it anywhere and run it.

The files are not code-signed yet, so SmartScreen may say "Windows protected your PC". Click **More info**, then **Run anyway**; it asks once per file. Each release lists SHA-256 checksums in `SHA256SUMS.txt`: run `Get-FileHash <file>` in PowerShell and compare.

Releases up to v0.1.3 were made under the old name and call their files `AnyPS5-Launcher-<version>-…`. Each release also carries `AnyPS5-Launcher-<version>-x64.exe`, the same portable exe under the old name, so copies of v0.1.2 and v0.1.3 can update themselves. You do not need it.

## Linux

64-bit x86-64 with glibc 2.35 or newer (Ubuntu 22.04, Debian 12 or Fedora 36 and later), a Vulkan driver, and the ALSA and udev libraries every desktop has.

- `AnyPlay-Launcher-<version>-x86_64.AppImage`: make it executable and run it. Without FUSE 2, add `--appimage-extract-and-run`.

  ```sh
  chmod +x AnyPlay-Launcher-*-x86_64.AppImage
  ./AnyPlay-Launcher-*-x86_64.AppImage
  ```

- `AnyPlay-Launcher-<version>-linux-x86_64.tar.gz` holds the executable, a `.desktop` file and the icon. Unpack it anywhere; copy the `.desktop` file to `~/.local/share/applications` and the icon to `~/.local/share/icons` for a menu entry. Coming from AnyPS5 Launcher: replace `anyps5-launcher.desktop` and `anyps5-launcher.png` with the new files and pin the launcher again.

The Linux build does not update itself yet: download a new version from the releases page. Starting a game with the network bridge is untested on Linux.

What games need from your system is up to AnyPS5:

- a CPU with AVX2, FMA, F16C, BMI1, BMI2, LZCNT and MOVBE (Zen 2, Intel Haswell or newer);
- a Vulkan 1.1 GPU. With Mesa drivers (AMD, Intel) games import memory through `/dev/udmabuf`: join the `kvm` group (`sudo usermod -aG kvm $USER`) and log in again. The launcher tells you when a game fails for this reason;
- on Wayland a game's window cannot be found or placed from outside, so the launcher counts a game as shown after a few seconds, and fullscreen and size follow the game's own settings (F11 in the game).

## First start

1. Choose a language and a look.
2. Pick your games folder, the one that holds your decrypted dumps. The launcher lists what it finds there.
3. The launcher downloads the AnyPS5 toolkit and tells you when it is ready. Without internet, open Settings later and choose *Download the Toolkit*.
4. Select a game, unlock it and press Play.

## Updates

On Windows the launcher looks for a new release when it starts, downloads and checks it in the background, and installs it when you close the launcher (or at once with *Restart Now* in Settings, About). Turn *Check for Updates* off in Settings to stop that. How updates are signed and checked is in [How it works](HOW-IT-WORKS.md#self-update).

## Where your data lives

Settings live in `%APPDATA%\AnyPS5` and the toolkit, logs and caches in `%LOCALAPPDATA%\AnyPS5`. On Linux they are in `~/.config/anyps5` and `~/.local/share/anyps5`, or where `XDG_CONFIG_HOME` and `XDG_DATA_HOME` point. On macOS they are in `~/Library/Application Support/AnyPlay Launcher`. Uninstalling leaves them in place. The Windows and Linux folders keep the old name on purpose, so an upgrade finds your data.
