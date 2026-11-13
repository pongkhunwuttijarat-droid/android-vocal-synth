---
version: alpha
name: "Rusted UTAU Studio"
description: "A focused dark music editor that keeps the piano roll, track context, and playback controls in view."
colors:
  primary: "#A98CFF"
  secondary: "#55D7DE"
  tertiary: "#FFC16D"
  neutral: "#111217"
  surface: "#181A22"
  raised: "#20232D"
  text: "#EEF0F7"
  muted: "#8E94A7"
  border: "#303441"
  success: "#7BE3A8"
  track-vocal: "#FF83B8"
typography:
  body:
    fontFamily: "Noto Sans Thai, Noto Sans, sans-serif"
    fontSize: 13px
    fontWeight: 400
    lineHeight: 1.35
  label:
    fontFamily: "Noto Sans Thai, Noto Sans, sans-serif"
    fontSize: 10px
    fontWeight: 600
    letterSpacing: "0.06em"
  data:
    fontFamily: "SFMono-Regular, Consolas, monospace"
    fontSize: 10px
    fontWeight: 400
rounded:
  sm: 5px
  md: 7px
  lg: 10px
spacing:
  xs: 4px
  sm: 8px
  md: 12px
  lg: 18px
  xl: 24px
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "#17131F"
    rounded: "{rounded.sm}"
    height: 31px
    padding: 11px
  button-primary-hover:
    backgroundColor: "#BEAAFF"
    textColor: "#17131F"
    rounded: "{rounded.sm}"
  panel:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    rounded: "{rounded.md}"
  editor-canvas:
    backgroundColor: "{colors.neutral}"
    textColor: "{colors.text}"
    rounded: "{rounded.sm}"
  raised-surface:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.text}"
    rounded: "{rounded.sm}"
  secondary-label:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.muted}"
    rounded: "{rounded.sm}"
  panel-border:
    backgroundColor: "{colors.border}"
    textColor: "{colors.text}"
    rounded: "{rounded.sm}"
  status-success:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.success}"
    rounded: "{rounded.sm}"
  note-selected:
    backgroundColor: "{colors.track-vocal}"
    textColor: "#17131F"
    rounded: "{rounded.sm}"
---

## Overview

Rusted UTAU Studio is an editor for making and shaping vocal performances. The main editor is an **Operate** surface: keep the piano roll dominant, make track and note context easy to inspect, and keep transport actions reachable. Use the existing Rusted UTAU dark palette rather than introducing a new brand direction.

## Colors

- **Primary (#A98CFF):** active tools, selection, and the main export action.
- **Secondary (#55D7DE):** secondary track or expression lane distinctions.
- **Tertiary (#FFC16D):** pitch curve and playhead accents.
- **Neutral/surface (#111217 / #181A22):** quiet background layers; use borders to define working regions instead of shadows.
- **Text (#EEF0F7) and muted (#8E94A7):** maintain clear hierarchy; never use muted text for essential values on dark surfaces.
- **Success (#7BE3A8):** saved and ready states only.

## Typography

Use a compact sans-serif for controls and labels. Use monospace only for time, BPM, note values, and technical readouts. Reserve uppercase, letter-spaced labels for small section headers, not long content.

## Layout

At landscape tablet/desktop widths, use a narrow global navigation rail, project/track column, central editor, and contextual inspector. Give the piano roll the largest area. Keep the arrangement overview above it, expression lanes below, and transport fixed at the bottom. Collapse project navigation and inspector before compressing the roll on narrow screens.

## Elevation & Depth

Prefer flat surfaces separated by one-pixel borders. Use a restrained shadow only for transient overlays; avoid glass effects and decorative glow. Selection outlines must remain visible against every track color.

## Shapes

Use compact 5–7px radii for controls and panels. Avoid pill shapes except for small status tags. Piano-roll notes should be rectangular with small corner radii so duration and alignment remain visually precise.

## Components

- **Primary action:** reserve the lavender filled button for export or the single highest-value action.
- **Toolbar:** compact, persistent tool selection with a clear active state; keep snap and zoom controls adjacent to the editor.
- **Track list:** show track color, name, voicebank, and mute/menu affordances without large cards.
- **Piano roll:** grid, pitch labels, notes, phoneme hints, curves, and playhead must remain visually separable.
- **Inspector:** show fields for the current selection; label units and make editable values obvious.
- **Transport:** persistent play/stop and time display; status text should not compete with playback.

## Do's and Don'ts

- Do preserve the established palette in `app/lib/theme.dart`.
- Do keep essential controls keyboard-focusable and provide visible focus rings.
- Do respect reduced-motion preferences.
- Don't encode track identity by color alone; retain track names and labels.
- Don't use gradients, heavy shadows, or decorative cards to simulate hierarchy.
- Don't put essential editing controls behind hover-only interactions.
