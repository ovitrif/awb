# awb v2 Roadmap

Rebuild this repo as **awb** (Android Wireless Bridge): a fast Rust CLI plus a native macOS
menu bar app implementing `DESIGN.pen`, ready to publish and launch as a public tool.

The terminal command is `awb`. The tray app binary is `awb-tray`, launchable via `awb tray`.

## Why Rust everywhere

The current desktop app is Compose Desktop on the JVM: slow to start, heavyweight to ship
(bundled runtime), and it doesn't match the design. The CLI is already Rust and owns all the
ADB/scrcpy/QR/mDNS logic. Porting the tray app to Rust (`eframe`/`egui` + `tray-icon`) gives
one toolchain, instant startup, a single small binary per artifact, and direct reuse of the
core logic without shelling out for status polling.

## Target architecture

```
Cargo.toml                workspace root
crates/awb-core/          lib: adb, scrcpy, qr, dnssd, command_path (all existing logic + tests)
crates/awb/               bin "awb": CLI (main.rs, ui.rs)
crates/awb-tray/          bin "awb-tray": macOS menu bar app (egui popover per DESIGN.pen)
install.sh                installs awb + awb-tray from GitHub releases
.github/workflows/        ci.yml (fmt, clippy, test), release.yml (tag -> binaries + checksums)
```

## Design reference (extracted from DESIGN.pen)

Popover window: 380x340, fill `#1F2025`, corner radius 14, 1px inner stroke `#FFFFFF14`,
padding 14/16, vertical gap 12. Font: Inter.

- Header: logo glyph 26px (bugdroid dome `#3DDC84`, two Wi-Fi waves `#4D9FF5`), title "awb"
  15/600 `#F2F3F7`, tagline "Android Wireless Bridge" 11 `#838791`, right-aligned icon
  buttons 14px `#9094A0`: refresh-cw, settings, qr-code.
- Tabs: "Devices" (active 12.5/600 `#F2F3F7`, 2px underline `#3DDC84`) and "Logs"
  (inactive `#9094A0`), hairline divider `#FFFFFF0D`.
- List row (devices + dependencies): icon 14px `#7C8190`, name 12.5/500 `#ECEEF4`, detail 11
  `#70747F`, trailing 22x22 action button fill `#FFFFFF0D` radius 6 with 10px icon (play/stop).
- Settings screen: nav header (chevron-left 16px + title 15/600); "scrcpy" section with
  Title/W/H inputs (28px tall, fill `#00000033`, stroke `#FFFFFF12`, radius 6, value 12
  `#ECEEF4`, label 10.5 `#7C8190`), checkboxes 15px (checked fill `#3DDC84` radius 4, check
  icon `#0A2A1B`; unchecked fill `#00000033` stroke `#FFFFFF26`) labeled "Top" and "Plain";
  "Dependencies" section with ADB and scrcpy rows ("Ready" state 11 `#9094A0`).
- Pair, QR state: white card 168x168 radius 12 with 144px QR (modules `#17181C`), label
  "Scan with your phone" 13/600, hint "Developer options → Wireless debugging → Pair device
  with QR code" 11 `#838791` centered.
- Pair, connecting state: spinner 28px `#3DDC84`, "Connecting to <device>…", hint "Keep both
  devices on the same Wi-Fi network".
- Pair, failed state: circle-alert 28px `#F87171`, "Pairing failed", hint, Retry button
  (fill `#3DDC84` radius 7, text/icon `#0A2A1B`) + Cancel button (fill `#FFFFFF0D`,
  text `#C9CCD6`), both 28px tall.
- Tray icon: monochrome template glyph (dome with punched eyes + two Wi-Fi arcs, geometry in
  DESIGN.pen `Tray Icon / Android-WiFi`), rendered for light/dark menu bars.

## Milestones

### M1: Rust workspace and rename to awb

- [x] Remove the Compose desktop app and all Gradle files (`desktop-app/`, `gradlew*`,
      `*.gradle.kts`, `gradle/`, `gradle.properties`, `.gradle/`, `.kotlin/`, `build/`).
- [x] Restructure into the workspace above; move `src/*.rs` logic into `awb-core` (lib) and
      the CLI into `crates/awb`.
- [x] Rename everything `airadb` -> `awb`: binary name, UI strings, env vars
      (`AWB_INSTALL_*`), repo metadata. Version becomes 2.0.0.
- [x] Drop the `aw` alias machinery (`install-shell` symlink logic); keep `awb completions`.
- [x] Add `awb tray` subcommand that launches `awb-tray` detached (sibling binary or PATH).
- [x] Update `.gitignore`, `AGENTS.md` for the new layout and names.

Verify: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`,
`cargo build --release`; run `awb --help`, `awb status --json`; install to `~/.local/bin/awb`
and run it.

### M2: macOS tray app implementing DESIGN.pen

- [x] Skeleton: `awb-tray` with tray-icon (template glyph rasterized from the design
      geometry), accessory activation policy (no Dock icon), borderless transparent
      always-on-top egui popover toggled by left-click on the tray icon, positioned under it,
      hidden on focus loss; right-click menu (Show/Hide, Pair new device, Refresh, Quit).
- [x] Theme: embed Inter font + Phosphor icon font; design tokens as constants.
- [x] Main screen: header, Devices/Logs tabs, device list rows backed by `awb-core` status
      polling on a background thread, mirror (scrcpy) start/stop per device.
- [x] Logs tab: timestamped action/output lines, monospaced, scrollable.
- [x] Settings screen: scrcpy options (Title, W, H, Top, Plain) persisted to
      `~/.config/awb/config.toml`; Dependencies section with ADB/scrcpy status rows.
- [x] Pair flow: QR -> Connecting -> Failed/Connected states driven by the same pairing
      logic the CLI uses (QR generation, mDNS watch, pair, connect) on a worker thread,
      with Retry/Cancel.

Verify: build and launch `awb-tray`; screenshot the popover and each state (devices, logs,
settings, pair QR/connecting/failed) and compare against DESIGN.pen; pair and mirror a real
phone end to end; confirm no Dock icon, focus-loss dismiss, and light/dark menu bar icon.

### M3: Packaging and release readiness

- [x] Rewrite `install.sh` for `awb`: installs both binaries from the release archive,
      `AWB_INSTALL_*` overrides, checksum verification, zsh completions.
- [x] Update `ci.yml` (workspace fmt/clippy/test on macOS) and `release.yml` (tag-driven:
      macOS aarch64/x86_64 archives with both binaries, Linux musl CLI-only archives,
      checksums, install.sh asset). (Verify workflow runs after merge.)
- [x] `scripts/bundle-app.sh`: wrap `awb-tray` into `AWB.app` (LSUIElement) for release
      archives so the tray app can live in /Applications and login items.

Verify: run install.sh against a local file server or `gh release` dry run; `tar tzf` the
archives; launch the bundled `AWB.app`.

### M4: Docs and launch

- [ ] Rewrite README for awb: what it is, install one-liner, CLI usage, tray app, screenshots
      from M2 verification, requirements (macOS, adb, scrcpy optional), build from source.
- [ ] Refresh AGENTS.md workflow (workspace commands, awb install path).
- [ ] Remove stale `PLAN.md`; commit `DESIGN.pen` and this roadmap.
- [ ] Final pass: `git status` clean, CI green, tag `v2.0.0` when the user is ready.

Verify: fresh-clone build, README commands copy-paste correctly, repo has no leftover
airadb/Gradle references (`grep -ri airadb` returns only intentional history notes).

## Out of scope for v2.0 (future)

- Homebrew tap (`brew install ovitrif/tap/awb`).
- Login item / launch-at-login toggle in the tray app.
- CLI reading `~/.config/awb/config.toml` for default scrcpy options.
- Windows/Linux tray support.
