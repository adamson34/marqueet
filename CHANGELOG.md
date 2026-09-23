# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Changed

- **Renamed from Tickadee to Marqueet** (marquee + parakeet): "Tickadee" is an
  existing time-clock product ([ADR-0008](docs/adr/0008-name-marqueet.md)).
  Crates are now `marqueet-core` / `marqueet-display`, the mascot is a
  parakeet, and the repository moved to `adamson34/marqueet`.

### Added

- **LED display on mock data (Phase 1).** `marqueet-display` is a native
  wgpu/winit app that draws a convincing LED sign: dot grid, glow, subtle
  flicker, stepped or smooth scrolling ([ADR-0004](docs/adr/0004-led-rendering-pipeline.md)).
  The main ticker shows two-line game blocks under league headers (possession,
  red zone, inning and outs, power plays, college ranks, dimmed losers); the
  crawl lists upcoming games; score changes flash the game's block.
- **Normalized sports schema** (`marqueet-core::sports`) covering football,
  basketball, baseball, hockey and soccer, with mock fixtures and ticker
  formatting.
- **Generic ticker segments and alerts** so non-sports sources can plug in
  later ([ADR-0003](docs/adr/0003-generic-segments-and-alerts.md)).
- **Hand-drawn LED font** as editable ASCII art, plus a Scale2x large font;
  tabular digits so scores don't jitter.
- **LED-safe team colors:** dark primaries are brightened keeping their hue.
- **Parakeet logo** drawn on a dot grid; SVGs in `media/` and the display's
  LED welcome screen are generated from one source
  ([ADR-0005](docs/adr/0005-dot-grid-brand-source-of-truth.md)).
- **Headless capture:** `--screenshot` for single frames and `--record` for
  frame sequences (for GIFs and videos).
- **Concept mockup GIF** (`media/mockup.gif`) of the planned design: the real
  LED ticker on top, with a header bar, flat crawl, widget cards (game of the
  day, scores) and a touchdown takeover composited below
  ([ADR-0007](docs/adr/0007-led-ticker-flat-widgets.md)).
- **`--score GAME:home|away:POINTS`** (headless) scripts a scoring alert: the
  game's score updates and its ticker block flashes in team color.
- **Repository setup:** CI (fmt, clippy, tests, ARM64 build, cargo-deny),
  Dependabot, issue and PR templates, CODEOWNERS, contributing and security
  policies, roadmap, ADRs, MIT license.

### Security

- winit's Wayland client-side decorations are disabled, removing the
  unmaintained `ttf-parser` (RUSTSEC-2026-0192) from the dependency tree.
