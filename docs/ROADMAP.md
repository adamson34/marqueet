# Roadmap

Marqueet is built in phases. Each phase ends with a review before the next
one starts; work lands on `dev` through feature-branch PRs (several per
phase). `main` is only updated for releases, starting with v1
([ADR-0010](adr/0010-main-is-for-releases.md)). Architectural decisions are recorded in
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

## Phase 2: live data ✅

- [x] `DataProvider` trait in core ([ADR-0009](adr/0009-espn-provider.md))
- [x] ESPN provider: fetch plus a pure `normalize()`, tested against saved JSON fixtures (real captures + labeled synthetic live states)
- [x] `server` crate (axum + tokio): `/ws` display feed, `/api/games` JSON, `/healthz`
- [x] Poll scheduler: 12 s while live, 60 s near kickoff, 5 min on game days, 20 min idle; ±10% jitter; exponential backoff to 5 min; last good data kept and marked stale (DELAYED) after 3 failures
- [x] WebSocket protocol (`core::protocol`)
- [x] The server formats games into segments so the display stays source-agnostic
- [x] Display client with reconnect (keeps last scores while disconnected); the mock feed becomes `--mock`

## Phase 3: events + takeovers ✅

- [x] Event engine (`core::events`): diff snapshots into touchdown / field goal / safety / extra point / two-point / home run / grand slam / runs / goal / final / score correction; play text breaks ties, score delta otherwise; basketball only alerts on finals
- [x] Deterministic alert ids (`<game>:<kind>:<away>-<home>`); no alerts on the first snapshot or on stale data
- [x] Mock feed runs the real engine
- [x] Takeover in the widget area: drifting team-color stripes and dot texture, LED-block kicker / headline / play / score box / "your player" pill, fade in and out, 10 s each, queued one at a time (stale ones dropped)
- [ ] Play text and "your player" pill in vector text (with the Phase 4 UI renderer)
- [x] Tests for event detection in every sport
- [x] Server runs the engine on every poll (only against a fresh previous snapshot), dedupes by id, pushes alerts to displays after the content update; `GET /api/alerts`

## Phase 4: widgets + admin

- [x] UI renderer: CPU canvas (rounded rects, vector text via swash with bundled Barlow Condensed (OFL), LED-block text) uploaded only when it changes, composited by the GPU with fades ([ADR-0007](adr/0007-led-ticker-flat-widgets.md))
- [x] Header bar: LIVE badge (grey when nothing is live), league filter label, LED clock
- [ ] Crawl restyled as flat condensed text with a TONIGHT tag
- [ ] Widget trait plus manifests with JSON Schema settings (schemars)
- [x] Widget view models built on the server (`core::widgets`), sent with each content update; the display only draws them
- [x] Widgets: game of the day (picks a favorite's live game, else the closest live game, else next up, else latest final; big team names, LED-block scores, situation chips, line score) and scores list (live, then finals, then upcoming)
- [ ] Widgets: standings, weather (fantasy in Phase 5); the header clock replaces the clock widget
- [x] Admin web page (`/admin`): server-rendered HTML with an escape helper (no template engine), plain CSS, one small hand-written script for drag-to-reorder; works with scripting off and on phones. Open on the device; from the network only with `MARQUEET_ADMIN_PASSWORD` (session cookie, HttpOnly, SameSite=Strict), cross-site posts refused, strict CSP. The server refuses a non-loopback `--listen` without a password
- [ ] Layout editor: preset slots with drag-and-drop on desktop, dropdowns on phones
- [x] SQLite settings store (`--db`), applied live: leagues and order (pollers start/stop), favorite teams, takeover policy (all / favorites / off), widget slots, display look
- [x] Overnight quiet hours (screen blanks)
- [x] Stale data marked DELAYED on the league header (since Phase 2)
- [ ] Time zone setting (uses the device's time zone for now)

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
