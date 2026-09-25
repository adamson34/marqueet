# ADR-0004: LED rendering: CPU rasterizes once, GPU scrolls and glows

- **Status:** Accepted
- **Date:** 2026-09-23

## Context

A convincing LED sign needs visible dots, glow, subtle flicker and flashes,
at 60 fps on weak GPUs, without per-frame CPU work that grows with content.

## Decision

1. **Rasterize once.** `core::ticker::Rasterizer` draws segments into an
   `LedBitmap` with one texel per LED, using the hand-drawn 5x8 font (5x7 capitals plus a descender row)
   (`fonts/led5x8.txt`) or its Scale2x double-size version. Scores use
   tabular digits so content width doesn't jitter.
2. **Upload once.** The strip is folded into 2048-wide tiles to respect GLES 3.0
   texture limits and uploaded when content changes. Flashes re-upload only
   the flashing segment's columns.
3. **Per frame, three tiny passes plus one composite:** gather the visible LEDs
   (scroll offset, flicker) into a grid texture about 150x20 texels; blur it
   twice for glow; then, per screen pixel, draw a round dot (lit color or dim
   unlit color) and add the glow.
4. Scrolling is stepped one LED column at a time by default, like a real sign;
   `--smooth` cross-fades between columns.

## Consequences

- Per-frame CPU cost is a few uniforms; GPU cost is dominated by one cheap
  full-band fragment pass.
- Anything shown on the LED bands must be expressible as an LED bitmap
  (text, the logo, simple icons).
