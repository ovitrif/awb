# Repository Instructions

## Workflow
- After every separable unit of work, commit the completed changes directly.
- Keep commits scoped to the completed unit of work.
- Before committing Rust changes, run:
  - `cargo fmt --all -- --check`
  - `cargo test`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo build --release`

## Patch Release Shipping
When Ovi asks to ship a patch release:
1. Create a dedicated `ovi/` release branch and pull request containing the complete patch diff, version bump, install documentation, and release notes context.
2. Keep the pull request in review until required local/product validation and CI pass, actionable feedback is fixed, addressed threads are resolved, and the unchanged final head receives two clean fresh-context reviews.
3. Immediately before merging, record and reverify the exact reviewed pull-request head, passing checks, and resolved feedback. Merge with a merge commit, then verify that the resulting merge commit contains that reviewed head as its pull-request parent. Do not use squash or rebase merge.
4. Create and push the annotated `vX.Y.Z` tag at the exact merge commit, verify the remote tag resolves to that commit, then wait for the tag-triggered Release workflow. Verify the GitHub release, all expected platform archives, `install.sh`, and `checksums.sha256`, and confirm GitHub associates the stable tag with the merged pull request. Curate the release notes to the established `This release:` bullet format when generated notes do not match it.

## Reinstall after every CLI/app change (required)
Any change to `crates/awb` (the `awb` CLI) or `crates/awb-app` (the menu bar
app) MUST be followed by a rebuild, reinstall, and restart so the checked-out
tools match the source and the running menu bar app is not left stale:
- `cargo build --release`
- `install -m 755 target/release/awb /Users/ovitrif/.local/bin/awb`
- `install -m 755 target/release/awb-app /Users/ovitrif/.local/bin/awb-app`
- Rebundle the app: `scripts/bundle-app.sh target/release/awb-app target/bundle`, then replace `/Applications/Android Wifi Bridge.app` by moving `target/bundle/Android Wifi Bridge.app` there (move, not copy: a bundle left in `target/` shows up in Spotlight as a second app).
- Replace the running instance (overwriting the binary does not update an already-running process): `pkill -9 -f awb-app`, then relaunch `awb app`.
- Verify: `/Users/ovitrif/.local/bin/awb --version`.

## Verify the app UI
Check every UI change yourself before calling it done, without the user's
screen, mouse or keyboard: drive the real popover headlessly.

- Build and start the drive server (renders offscreen; no window, no menu bar
  item, no focus stealing):
  `cargo build --release -p awb-app --features drive`, then
  `target/release/awb-app --drive /tmp/awbd.sock &`. Keep the socket path
  short (Unix sockets cap it near 100 bytes) and end the server with
  `target/release/awb-app drive /tmp/awbd.sock quit` when done (the installed
  `awb-app` lacks the drive feature).
- Send commands with `target/release/awb-app drive /tmp/awbd.sock <command>`,
  or pipe one per line with `-`. Each reply is one `ok ...` line carrying the
  current `screen`, `tab`, `transition`, `theme`, `gradients` and `pairing`
  phase. Commands: `state`, `click X Y`, `hover X Y`, `leave`, `scroll DY`,
  `key NAME` (egui names, `shift+Tab`), `wait MS`, `shot PATH`,
  `record DIR` / `stop` (every frame plus `times.txt`), `theme auto|day|night`,
  `gradients on|off`, `quit`. Coordinates are window points: 380 wide, 0 at
  the top of the beak; the header buttons sit at y 36, x 287 (Refresh), 319
  (Settings) and 351 (Pair). The full reference is in `crates/awb-app/src/drive.rs`.
- The drive server isolates settings only: it uses a throwaway config folder
  (seed it with `AWB_DRIVE_CONFIG=<config.toml>`) and never touches the login
  item. Everything else is real: adb, the emulator, `avdmanager` and scrcpy,
  so confirming a delete removes a real AVD and Play starts a real emulator or
  mirror. Run it with demo AVDs (below) unless real ones are the point.
- Light and dark mode: `theme day` / `theme night` / `theme auto` over the
  socket. In the running app, use the Appearance control in Settings or set
  `theme = "day"`, `"night"` or `"auto"` in `~/.config/awb/config.toml`
  (honors `XDG_CONFIG_HOME`), then restart it. Check both themes, and both
  `gradients on` and `off`, for every visual change.
- Pairing without a phone (drive builds only): `AWB_MOCK_PAIRING=success` or `failure`, with an
  optional `:seconds` before the simulated scan (`success:1.5`), runs the real
  pairing screens with a simulated scan; success lists `Pixel 9 Pro (mock)`.
- Emulators without touching real AVDs: point `ANDROID_AVD_HOME` at a folder
  of `<Name>.ini` files whose `path=` names an existing `<Name>.avd` folder.
- Before publishing a capture, keep private names out of it: use demo AVDs,
  and put `scutil` and `hostname` stubs that print `awb-demo` first on `PATH`,
  since the pairing QR embeds the machine name.
- Cover every screen and state a change can reach: Devices (checking, empty,
  phones and emulators, a scrolled list with its edge fades and scrollbar),
  Logs, Settings, Pair (QR, connecting, failed, paired), the delete-emulator
  dialog, Tab focus highlights, ← / → page navigation, and the slide of every
  transition. Moving forward (Main → Settings → Pair) slides left, moving back
  slides right, and both pages stay fully drawn throughout; `record` and read
  the frames to confirm.

## Layout
- `crates/awb-core`: shared ADB/scrcpy/QR/mDNS logic (lib).
- `crates/awb`: the `awb` CLI.
- `crates/awb-app`: the macOS menu bar app (`awb-app`), design source in `DESIGN.pen`.

## CI / GitHub Actions
- GitHub Action workflow file changes only take effect on PRs opened after the merge of the PR that modifies them. Always note "(after merge)" in test plan items about verifying workflow behavior.
