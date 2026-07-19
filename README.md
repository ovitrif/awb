# awb

Android Wireless Bridge for macOS: pair, connect, and mirror Android phones over Wi-Fi
without remembering a single `adb` command.

awb wraps Android's wireless debugging flow behind a QR code. Scan it with your phone and
awb handles mDNS discovery, `adb pair`, `adb connect`, and reconnects, then launches
[scrcpy](https://github.com/Genymobile/scrcpy) screen mirroring if you want it. It ships as
two small native binaries: the `awb` CLI and a menu bar app.

## Install

```sh
curl -fsSL https://github.com/ovitrif/awb/releases/latest/download/install.sh | sh
```

Pin a release or choose the install directory:

```sh
curl -fsSL https://github.com/ovitrif/awb/releases/latest/download/install.sh | \
  AWB_INSTALL_TAG=v2.0.3 AWB_INSTALL_DIR="$HOME/.local/bin" sh
```

Requirements: `adb` on your PATH (Android SDK platform-tools). `scrcpy` is optional and
only needed for screen mirroring. The phone needs Android 11+ with developer options
enabled, on the same Wi-Fi network as your Mac.

## Menu bar app

```sh
awb app
```

The awb icon appears in the menu bar. Left-click toggles the popover: connected devices
with one-click mirroring, a Logs tab, scrcpy settings, and QR pairing for new phones.
Right-click offers Show, Pair, Refresh, and Quit.

macOS release archives also contain `AWB.app` for /Applications and login items; it is the
same menu bar app in a bundle.

## CLI

```sh
awb
```

Running `awb` with no arguments checks ADB, shows a QR code, and guides the phone from
`Developer options -> Wireless debugging -> Pair device with QR code` to a connected
device, with live waits, retries, and manual IP:port fallbacks when discovery misbehaves.
Once connected it offers to start scrcpy.

Useful commands and flags:

```sh
awb status --json        # ADB, scrcpy, and device status for scripts and UIs
awb reset-adb            # kill and restart the local ADB server
awb completions zsh      # print zsh completions
awb app                  # launch the menu bar app
awb --connect-only       # pair and connect without the scrcpy menu
awb --background         # start scrcpy detached and exit
awb --foreground         # start scrcpy and wait until it exits
awb --stable             # scrcpy + keepalive watch + reconnects + Wi-Fi diagnostics
awb --watch --wifi-doctor
awb --timeout 120        # wait longer than the 60-second default
awb --device-serial SERIAL --background
awb --window-title "Pixel 10 Pro" --window-width 480 --window-height 1071
awb --plain-window --always-on-top
awb --adb /path/to/adb --scrcpy /path/to/scrcpy
```

By default scrcpy launches borderless at 480x1071 with `--stay-awake` and no audio; pass
`--plain-window` for scrcpy's regular decorated window. The menu bar app stores its scrcpy
options in `~/.config/awb/config.toml`.

## Build from source

```sh
git clone https://github.com/ovitrif/awb.git
cd awb
cargo build --release            # target/release/awb and target/release/awb-app
scripts/bundle-app.sh            # optional: wraps awb-app into target/bundle/AWB.app
```

The workspace has three crates: `awb-core` (ADB, scrcpy, QR, and Bonjour logic), `awb`
(the CLI), and `awb-app` (the menu bar app, built from the design in `DESIGN.pen`).

## License

MIT. Bundled [Inter](https://rsms.me/inter/) font is licensed under the SIL OFL 1.1;
icons are [Phosphor](https://phosphoricons.com/) (MIT).
