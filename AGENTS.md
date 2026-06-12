# Repository Instructions

## Shell
- Prefix shell commands with `rtk`.

## Workflow
- After every separable unit of work, commit the completed changes directly.
- After every separable unit of work, install or reinstall the local `awb` binary so the checked-out tool is immediately testable.
- Keep commits scoped to the completed unit of work.
- Before committing Rust changes, run:
  - `rtk cargo fmt --all -- --check`
  - `rtk cargo test`
  - `rtk cargo clippy --all-targets -- -D warnings`
  - `rtk cargo build --release`
- Reinstall locally with:
  - `rtk proxy install -m 755 target/release/awb /Users/ovitrif/.local/bin/awb`
  - `rtk proxy install -m 755 target/release/awb-tray /Users/ovitrif/.local/bin/awb-tray`
- Verify the reinstall with:
  - `rtk proxy /Users/ovitrif/.local/bin/awb --version`

## Layout
- `crates/awb-core`: shared ADB/scrcpy/QR/mDNS logic (lib).
- `crates/awb`: the `awb` CLI.
- `crates/awb-tray`: the macOS menu bar app (`awb-tray`), design source in `DESIGN.pen`.

## CI / GitHub Actions
- GitHub Action workflow file changes only take effect on PRs opened after the merge of the PR that modifies them. Always note "(after merge)" in test plan items about verifying workflow behavior.
