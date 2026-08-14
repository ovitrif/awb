# Repository Instructions

## Shell
- Prefix shell commands with `rtk`.

## Workflow
- After every separable unit of work, commit the completed changes directly.
- Keep commits scoped to the completed unit of work.
- Before committing Rust changes, run:
  - `rtk cargo fmt --all -- --check`
  - `rtk cargo test`
  - `rtk cargo clippy --all-targets -- -D warnings`
  - `rtk cargo build --release`

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
- `rtk cargo build --release`
- `rtk proxy install -m 755 target/release/awb /Users/ovitrif/.local/bin/awb`
- `rtk proxy install -m 755 target/release/awb-app /Users/ovitrif/.local/bin/awb-app`
- Rebundle the app: `rtk proxy scripts/bundle-app.sh target/release/awb-app target/bundle`, then refresh `/Applications/Android Wifi Bridge.app` from `target/bundle/Android Wifi Bridge.app`.
- Replace the running instance (overwriting the binary does not update an already-running process): `rtk proxy pkill -9 -f awb-app`, then relaunch `awb app`.
- Verify: `rtk proxy /Users/ovitrif/.local/bin/awb --version`.

## Layout
- `crates/awb-core`: shared ADB/scrcpy/QR/mDNS logic (lib).
- `crates/awb`: the `awb` CLI.
- `crates/awb-app`: the macOS menu bar app (`awb-app`), design source in `DESIGN.pen`.

## CI / GitHub Actions
- GitHub Action workflow file changes only take effect on PRs opened after the merge of the PR that modifies them. Always note "(after merge)" in test plan items about verifying workflow behavior.
