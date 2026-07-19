# Design QA — AWB 2.0.3

## Source visual truth

- Main screen: `/Users/ovitrif/Library/CloudStorage/GoogleDrive-masivotech.be@gmail.com/My Drive/Captures/Obsidian 2026-07-19 007660.png`
- Settings controls: `/Users/ovitrif/Library/CloudStorage/GoogleDrive-masivotech.be@gmail.com/My Drive/Captures/Obsidian 2026-07-19 007662.png`
- Persistent scrollbar: `/Users/ovitrif/Library/CloudStorage/GoogleDrive-masivotech.be@gmail.com/My Drive/Captures/Google Chrome 2026-07-19 007664.png`
- Screen-change baseline: `/Users/ovitrif/Library/CloudStorage/GoogleDrive-masivotech.be@gmail.com/My Drive/Captures/CleanShot 2026-07-19 007670.mp4`

## Native comparison

- Viewport: native 380 × 349-point popover, captured at 2× where noted.
- Main comparison, reference left and implementation right: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/main-comparison-2.0.3.png`
- Settings comparison, reference left and implementation right: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/settings-comparison-2.0.3.png`
- Focused input state: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/settings-focus-day-2.0.3.png`
- Settled scrolled state: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/settings-scrolled-settled-2.0.3.png`
- Wi-Fi pairing state: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/pair-day-2.0.3.png`

## Findings

- The Day shell now has visible cool-blue depth through the lower half while preserving the near-white top, centered beak, top-edge sheen, and existing shell geometry. No banding or transparency artifacts are visible.
- Enabled text fields now read as interactive through darker resting outlines and small elevation; hover and keyboard-focus states strengthen the outline further, with a blue focus treatment.
- Checkboxes use darker, slightly thicker outlines with elevation and pointer feedback. The appearance selector has a clearer track, elevated selected segment, and selected-segment border.
- The settings scrollbar appears while the scroll position is moving and hides after the 250 ms settle delay. The scrolled settled capture shows the full dependency rows without a lingering scrollbar.
- Main, Settings, and Pair use a 220 ms cubic eased horizontal push/pull with a restrained opacity blend. Pairing cancellation is deferred until the back transition completes so outgoing content does not disappear mid-motion.
- The native journey was exercised through Main → Settings → Main and Main → Pair → Main. With Wi-Fi enabled, the QR payload rendered successfully, the countdown advanced, and Back returned to Main while cancelling pairing.
- Typography, spacing rhythm, interaction targets, corner geometry, and the existing AWB color language remain consistent with the current design system.

## Implementation checklist

- [x] Strengthen the Day background gradient.
- [x] Improve resting, hover, focus, and selected control affordances.
- [x] Auto-hide the settings scrollbar 250 ms after scrolling settles.
- [x] Add directional eased transitions between screens.
- [x] Preserve pairing content until its exit transition completes.
- [x] Compare supplied references and native renders at the same viewport.
- [x] Exercise Settings and Wi-Fi pairing in the native app.
- [x] Remove screenshot-only QA scaffolding from release source.

final result: passed
