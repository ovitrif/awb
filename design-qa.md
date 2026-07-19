# Design QA — AWB 2.0.3

## Source visual truth

- Main screen: `/Users/ovitrif/Library/CloudStorage/GoogleDrive-masivotech.be@gmail.com/My Drive/Captures/Obsidian 2026-07-19 007660.png`
- Settings controls: `/Users/ovitrif/Library/CloudStorage/GoogleDrive-masivotech.be@gmail.com/My Drive/Captures/Obsidian 2026-07-19 007662.png`
- Persistent scrollbar: `/Users/ovitrif/Library/CloudStorage/GoogleDrive-masivotech.be@gmail.com/My Drive/Captures/Google Chrome 2026-07-19 007664.png`
- Screen-change baseline: `/Users/ovitrif/Library/CloudStorage/GoogleDrive-masivotech.be@gmail.com/My Drive/Captures/CleanShot 2026-07-19 007670.mp4`
- White-top correction baseline: `/Volumes/ssd/r/github/ovitrif/awb/.ai/logs/2.0.3-patch/white-top-baseline.png`
- Appearance layout baseline: `/Volumes/ssd/r/github/ovitrif/awb/.ai/logs/2.0.3-patch/appearance-layout-baseline.png`
- Scroll-edge baseline: `/Volumes/ssd/r/github/ovitrif/awb/.ai/logs/2.0.3-patch/scroll-edge-baseline.png`

## Native comparison

- Viewport: native 380 × 349-point popover, captured at 2× where noted.
- Main comparison, reference left and implementation right: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/main-comparison-2.0.3.png`
- Settings comparison, reference left and implementation right: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/settings-comparison-2.0.3.png`
- Focused input state: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/settings-focus-day-2.0.3.png`
- Settled scrolled state: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/settings-scrolled-settled-2.0.3.png`
- Wi-Fi pairing state: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/pair-day-2.0.3.png`
- White-top native implementation: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/main-day-white-top-2.0.3.png`
- White-top full-view comparison, baseline left and corrected implementation right: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/white-top-comparison-2.0.3.png`
- A separate focused crop was unnecessary because the gradient spans the full shell; centerline pixel samples provide the focused color evidence.
- Compact appearance native implementation: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/settings-compact-appearance-2.0.3.png`
- Appearance focused comparison, baseline left and corrected implementation right: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/appearance-layout-comparison-2.0.3.png`
- Scroll-polish full-view comparison, baseline left and implementation right: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/scroll-polish-comparison-full-2.0.3.png`
- Focused scrollbar comparison: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/scroll-polish-comparison-scrollbar-2.0.3.png`
- Day scroll state with the indicator visible: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/settings-scroll-fades-indicator-2.0.3.png`
- Day scroll state after the indicator settles: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/settings-scroll-fades-hidden-2.0.3.png`
- Night scroll state: `/Volumes/ssd/r/github/ovitrif/awb/target/design-qa/settings-scroll-fades-night-2.0.3.png`

## Findings

- The Day shell now holds pure white through the header before transitioning to clearly visible cool-blue depth in the lower half. The centered beak, top-edge sheen, and existing shell geometry remain intact, with no banding or transparency artifacts.
- The supplied baseline sampled at RGB `243/245/252` near the top and `235/239/248` near the bottom. The corrected native capture samples at RGB `255/255/255` across the top/header and `227/235/248` near the bottom, restoring a clear white-to-blue range.
- The appearance selector now uses a fixed 172-point width instead of expanding across the row. Its 28-point row vertically centers the `Appearance` label and selector, while the selector's right edge aligns with the settings content edge.
- Enabled text fields now read as interactive through darker resting outlines and small elevation; hover and keyboard-focus states strengthen the outline further, with a blue focus treatment.
- Checkboxes use darker, slightly thicker outlines with elevation and pointer feedback. The appearance selector has a clearer track, elevated selected segment, and selected-segment border.
- Scrolled content now dissolves through 22-point top and bottom masks instead of ending on a hard crop. Each mask appears only when more content exists beyond that edge and eases in over the first 22 points of scroll travel.
- The built-in proportional scrollbar is replaced by a fixed 44 × 2-point pill. It uses a semi-transparent theme token, tracks normalized scroll progress, remains fully visible while scrolling, and fades over the final 100 ms of the 250 ms settle delay.
- Native Day and Night captures confirm that the masks blend into their respective shell gradients. The settled Day capture confirms that the indicator disappears without leaving a track or edge artifact.
- Main, Settings, and Pair use a 220 ms cubic eased horizontal push/pull with a restrained opacity blend. Pairing cancellation is deferred until the back transition completes so outgoing content does not disappear mid-motion.
- The native journey was exercised through Main → Settings → Main and Main → Pair → Main. With Wi-Fi enabled, the QR payload rendered successfully, the countdown advanced, and Back returned to Main while cancelling pairing.
- The compact selector was exercised in `Auto` and `Day` states; selection, appearance persistence, and the selected-segment treatment remained functional.
- Typography, spacing rhythm, interaction targets, corner geometry, and the existing AWB color language remain consistent with the current design system.
- Fidelity surfaces: typography, colors, copy, iconography, and image quality are unchanged; the scoped spacing and alignment regression is corrected without introducing clipping or density changes elsewhere.

## Comparison history

- Pass 1 — P1: the blue beak glow tinted the nominally white gradient stop, so the entire shell read as blue-gray and the gradient lacked a visible white endpoint.
- Fix: replaced the Day beak tint with a neutral white glow, held the base at pure white through 28% of the shell, and strengthened the lower blue stop.
- Pass 2: the full-view comparison and native pixel samples show a pure-white header and an unmistakably blue lower shell. No actionable P0/P1/P2 differences remain.
- Pass 3 — P1: the appearance segmented control expanded to the full remaining row width and sat out of alignment with its label, making three short choices look like a large form field.
- Fix: constrained the control to 172 × 28 points, vertically centered the row, kept 56-point segments, and aligned the control to the settings content edge.
- Pass 4: the normalized focused comparison shows a compact selector with aligned centers and deliberate edge alignment. No actionable P0/P1/P2 differences remain.
- Pass 5 — P1: scrolling exposed a hard content cut under the fixed header and an oversized, opaque proportional thumb spanning most of the viewport.
- Fix: added conditional 22-point edge masks and replaced the platform scrollbar with a short, translucent position indicator that fades after scrolling settles.
- Pass 6: normalized full-view and focused comparisons show a clean content dissolve, a compact indicator, and no persistent scrollbar in both Day and Night states. No actionable P0/P1/P2 differences remain.

## Implementation checklist

- [x] Strengthen the Day background gradient.
- [x] Keep the Day header genuinely white rather than blue-gray.
- [x] Make the appearance selector compact and align it with its row and content edge.
- [x] Improve resting, hover, focus, and selected control affordances.
- [x] Auto-hide the settings scrollbar 250 ms after scrolling settles.
- [x] Fade overflowing settings content beneath the fixed header and footer edges.
- [x] Use a short, semi-transparent scrollbar indicator that preserves position feedback.
- [x] Add directional eased transitions between screens.
- [x] Preserve pairing content until its exit transition completes.
- [x] Compare supplied references and native renders at the same viewport.
- [x] Exercise Settings and Wi-Fi pairing in the native app.
- [x] Remove screenshot-only QA scaffolding from release source.

final result: passed
