# Roadmap

Marqueet is built in phases. Each phase ends with a review before the next
one starts; work lands on `dev` through feature-branch PRs and is promoted to
`main` when a phase is approved. Architectural decisions are recorded in
[`adr/`](adr/).

## Phase 1: repo + LED display on mock data ✅

- [x] Cargo workspace, rustfmt, clippy (`-D warnings`), CI (incl. ARM64), cargo-deny, Dependabot
- [x] Normalized game schema covering all five sports, plus mock fixtures
- [x] Generic ticker segments, alerts, display config, layout math ([ADR-0003](adr/0003-generic-segments-and-alerts.md))
- [x] Hand-drawn LED font plus a Scale2x large font
- [x] LED renderer: dot grid, glow, flicker, stepped or smooth scroll ([ADR-0004](adr/0004-led-rendering-pipeline.md))
- [x] Two-line game blocks, league headers, team colors made LED-safe
- [x] Score flash (invert blink, then a fading boost)
- [x] Crawl with upcoming games; placeholder LED clock in the widget area
- [x] Layouts for 1080p, 1366x768 and 4:3; headless `--screenshot` / `--record`
- [x] Parakeet dot-grid logo, generated SVGs, LED welcome screen ([ADR-0005](adr/0005-dot-grid-brand-source-of-truth.md))
- [x] Concept mockup GIF of the planned design ([ADR-0007](adr/0007-led-ticker-flat-widgets.md))
- [x] Headless `--score` to script a scoring alert
- [ ] Verify 60 fps on a real Raspberry Pi 4 (the display logs fps every 10 s)

## Phase 2: live data

- [x] `DataProvider` trait in core ([ADR-0009](adr/0009-espn-provider.md))
- [x] ESPN provider: fetch plus a pure `normalize()`, tested against saved JSON fixtures (real captures + labeled synthetic live states)
- [ ] `server` crate (axum + tokio)
- [ ] Poll scheduler: 10 to 15 s for live games, 60 s pre-game, 15 to 30 min idle; jitter; backoff; stale-but-served cache
- [ ] WebSocket protocol (`core::protocol`); display client with reconnect; the mock feed becomes `--mock`
- [ ] The server formats games into segments so the display stays source-agnostic

## Phase 3: events + takeovers

- [ ] Event engine: diff snapshots into touchdown / field goal / safety / home run / run / goal / final / score correction
- [ ] Deterministic alert ids (no repeats across restarts); no alerts on the first snapshot or on stale data
- [ ] Takeover in the widget area: team-color stripes and dot texture, LED-block headline (TOUCHDOWN, HOME RUN, GOAL), play text, score box, "your player" pill; about 10 s
- [ ] Tests for event detection in every sport

## Phase 4: widgets + admin

- [ ] UI renderer for the widget area: rounded cards, vector text with a bundled condensed font (OFL), LED-block accents ([ADR-0007](adr/0007-led-ticker-flat-widgets.md)); audit the text stack with cargo-deny
- [ ] Header bar: LIVE badge, league filter, LED clock
- [ ] Crawl restyled as flat condensed text with a TONIGHT tag
- [ ] Widget trait plus manifests with JSON Schema settings (schemars)
- [ ] Widgets: game of the day (line score, down and distance), scores list, standings, fantasy, clock, weather
- [ ] Admin web UI: askama templates, plain HTML/CSS, no third-party JS; laptop-first, works on phones
- [ ] Layout editor: preset slots with drag-and-drop on desktop, dropdowns on phones
- [ ] SQLite settings store; time zone; overnight screen-off schedule; "data is stale" indicator

## Phase 5: fantasy

- [ ] Sleeper plugin: username / league lookup, matchup, starters with live points
- [ ] Player matching through Sleeper's `espn_id`; fantasy details in takeovers

## Phase 6: kiosk

- [ ] Ubuntu Frame plus systemd units (server, display); mDNS `marqueet.local`
- [ ] First-boot screen: logo, hostname, IP, QR code, one-time 6-digit setup code (loopback-only)
- [ ] Password creation on first login; the setup code then expires
- [ ] SSH off by default (toggle in admin, keys recommended); optional self-signed HTTPS
- [ ] Password reset through a file on the boot partition
- [ ] Optional: WiFi captive portal when there's no Ethernet
- [ ] Decide packaging: snaps on Ubuntu Core (auto-update, rollback) vs .deb

## Phase 7: distribution

- [ ] Flashable Raspberry Pi image; installer for Ubuntu on x86
- [ ] Automatic updates
- [ ] Release workflow with checksums

## Later

- [ ] Non-sports sources: stocks, weather alerts, RSS, Home Assistant
- [ ] More sports providers as fallbacks for ESPN

## Non-goals

- A browser-based display, or any JavaScript toolchain ([ADR-0001](adr/0001-rust-everywhere-no-js-toolchain.md))
- Physical RGB LED matrix panels
- Cloud accounts or telemetry
- Betting features
