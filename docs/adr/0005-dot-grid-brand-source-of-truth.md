# ADR-0005: The logo is a dot grid that generates every asset

- **Status:** Accepted
- **Date:** 2026-09-23

## Context

The logo must appear as SVG (README, favicon, future admin page) and on the
LED display itself (welcome and boot screens). Hand-maintaining both would
drift.

## Decision

- The mark is a chickadee drawn on a dot grid in `crates/core/assets/mark.txt`
  (`#` solid dot, `o` ring dot, `.` empty); a head-only `favicon.txt` stays
  legible at 16 px.
- `core::logo` renders the grids to SVG (amber, ink and paper variants, plus a
  `currentColor` favicon) and to `LedBitmap` for the display.
- A test compares the files in `media/` with freshly generated output and
  fails on drift; `TICKADEE_BLESS=1 cargo test -p tickadee-core logo`
  regenerates them.

## Consequences

- Editing the logo is a text edit plus one command; every surface stays in sync.
- The logo is constrained to what reads well as dots, which fits the product.
