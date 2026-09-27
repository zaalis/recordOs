# Vision-Pro — Design System

This file is the visual source of truth for the iPad PWA and the macOS application. Page-specific files in `pages/` may refine layout, but must not override accessibility, naming, or semantic-color rules.

## Product principles

1. The remote content is primary. Chrome recedes and can disappear completely.
2. Every common action feels immediate; security state stays explicit and calm.
3. Advanced controls use progressive disclosure. Networking and codec jargon never blocks automatic mode.
4. iPad and Mac share materials, typography, spacing, iconography, and language without pretending to be an Apple system app.

## Visual direction

- Monochrome, minimal, professional and content-first.
- Deep black canvas, neutral white/gray surfaces, controlled transparency and light blur.
- System typography only: SF Pro (SF Mono for codes), system sans-serif fallback.
- No logo exists yet. Render the development name exactly as `Vision-Pro`; do not invent a brand mark.
- Use the custom 24 px line SVG icon set. Never use emoji as interface icons.
- Reserve color for semantic state: green connected/success, red destructive/error, orange warning, blue actionable information.
- Avoid gold accents, editorial serif typography, heavy gradients, ornamental glass, and marketing-page scroll effects.

## Core tokens

The implemented source of truth is `packages/design/src/styles.css` under the `--vp-*` namespace.

- Touch target: 44 px minimum; 8 px minimum gap between adjacent targets.
- Radii: 6, 10, 14, 20, 28 px, plus full pill.
- Spacing: 4, 8, 12, 16, 20, 24, 32, 40, 56 px.
- Text: 12 px metadata minimum, 15 px body, 17 px section title; line height at least 1.4.
- Focus: visible 2 px ring with 2 px offset.
- Contrast: 4.5:1 for body text and 3:1 for large text and meaningful controls.

## Motion and materials

- Use 140–380 ms transitions with critically damped spring/ease-out curves.
- Animate opacity and transforms; avoid layout-affecting width/height animation.
- Motion explains state, spatial continuity, and successful connection. It is never decorative noise.
- Respect `prefers-reduced-motion`; the final readable state must remain complete.
- Blur is limited to floating chrome and sheets. Always provide an opaque-enough fallback and preserve text contrast.

## Responsive behavior

- iPad portrait: vertical content flow, bottom floating session toolbar, sheets within safe areas.
- iPad landscape: maximize the remote stage; side panels remain under 420 px unless shortcut/quality content needs 640 px.
- Large iPad/desktop: increase breathing room and control size slightly; never stretch cards or lines indefinitely.
- Support safe-area insets, dynamic viewport units, zoom, hardware keyboard, touch, Pointer Events and Pencil.
- No horizontal page scrolling and no control hidden behind browser/system chrome.

## Interaction and accessibility

- Every icon-only control has an accessible label and visible focus state.
- Do not rely on hover; pressed, selected, loading, denied and reconnecting states must be visible.
- Pairing always distinguishes discovery from authorization and shows confirmation on the Mac.
- Destructive actions are named precisely and require confirmation when they revoke identity or trust.
- Auto-hidden controls must remain recoverable with one obvious tap.

## Pre-delivery checks

- Verify 375, 768, 1024 and 1440 px widths plus iPad portrait/landscape safe areas.
- Verify keyboard-only navigation, screen-reader labels, reduced motion, dark/light contrast and 200% zoom.
- Verify no emoji icons, no missing labels, no target below 44 px, and no hover-only action.
- Verify connection, pairing, denial, reconnect, view-only, stream fallback and disconnect states.
