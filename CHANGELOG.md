# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Fixed

- A two-point conversion that arrives in the poll after its touchdown (+6,
  then +2) is now reported as a two-point conversion instead of a safety.

### Changed

- **Renamed from Tickadee to Marqueet** (marquee + parakeet): "Tickadee" is an
  existing time-clock product ([ADR-0008](docs/adr/0008-name-marqueet.md)).
  Crates are now `marqueet-core` / `marqueet-display`, the mascot is a
  parakeet, and the repository moved to `adamson34/marqueet`.

### Added

- **UI layer and header bar** (Phase 4): a CPU canvas with anti-aliased
  rounded rectangles, vector text (swash, bundled Barlow Condensed under the
  SIL OFL) and LED-block text, uploaded to the GPU only when it changes. The
  new header bar above the ticker shows a LIVE badge (grey when nothing is
  live), the league filter and an LED clock. `header_ratio` sizes it (0 hides
  it).
- **Takeovers** (Phase 3): touchdowns, home runs, grand slams and goals take
  over the widget area for 10 s: drifting team-color stripes with a dot
  texture, LED-block kicker, headline, play and score box (scoring side in
  amber), with fades. Takeovers queue one at a time and stale ones are
  dropped. Rendered with a new background shader plus an overlay mode of the
  LED renderer (square blocks, transparent background).
- `--score` (with `--mock`) now runs the real event engine, so it can trigger
  takeovers in screenshots and recordings.
- **Server alerts** (Phase 3): `marqueet-server` runs the event engine on
  every successful poll, dedupes alerts by id, and pushes them to connected
  displays right after the content update. `GET /api/alerts` lists the 50
  most recent. No alerts on a league's first snapshot or across a stale gap.
- **Event engine** (`marqueet-core::events`, Phase 3): compares consecutive
  snapshots of a game and reports touchdowns, field goals, safeties, extra
  points, two-point conversions, home runs, grand slams, runs, goals and finals
  (score corrections are detected but never alerted). Big plays become
  takeover alerts with a kicker, headline, play text and score line; the rest
  flash the ticker. Alert ids are deterministic so a moment is never shown
  twice. The `--mock` feed now uses the same engine.
- **Display live feed** (Phase 2): `marqueet-display` connects to
  `marqueet-server` (`--server URL`, default `ws://127.0.0.1:7878/ws`) on a
  background thread, shows CONNECTING TO SERVER until scores arrive, keeps the
  last scores up while disconnected, and reconnects with backoff. Built-in
  demo data moved behind `--mock`.
- **`marqueet-server`** (Phase 2): polls each league on an adaptive schedule
  (12 s while live, 60 s near kickoff, 5 min on game days, 20 min idle, with
  jitter and exponential backoff), keeps the last good data when ESPN fails
  and marks it DELAYED after 3 failures, formats games into ticker/crawl
  segments, and pushes them to the display over `ws://127.0.0.1:7878/ws`.
  Also `GET /api/games` (games plus per-league fetch health) and `GET /healthz`.
- **Wire protocol** (`marqueet-core::protocol`): `hello`, `content`, `alert`.
- **ESPN provider** (`marqueet-provider-espn`, Phase 2): fetches ESPN's
  unofficial scoreboards for NFL, college football, MLB, NBA, WNBA, college
  basketball, NHL, MLS, Premier League and Champions League, and normalizes
  them into the shared schema. Lenient per-event parsing (one bad game never
  hides the league), an honest `marqueet/<version>` user agent, and rustls with
  ring for TLS ([ADR-0009](docs/adr/0009-espn-provider.md)). Tested against
  real captures plus synthetic live states.
- **`DataProvider` interface** (`marqueet-core::provider`).
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
