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
- [x] Crawl restyled as flat condensed text with a TONIGHT / TODAY / UP NEXT tag; drawn once per change and scrolled on the GPU (tiled strip, no per-frame uploads)
- [x] Feed API ([docs/FEEDS.md](FEEDS.md)): local programs in any language push ticker segments, crawl lines and flashes/takeovers over HTTP (`POST /api/feeds/<name>`, `/alert`, `DELETE`), with a per-feed Bearer token created on the admin page (stored apart from settings), expiring content, size limits and alert rate limits. The extension point instead of compiled-in plugins
- [x] Widget view models built on the server (`core::widgets`), sent with each content update; the display only draws them
- [x] Widgets: game of the day (picks a favorite's live game, else the closest live game, else next up, else latest final; big team names, LED-block scores, situation chips, line score) and scores list (live, then finals, then upcoming)
- [x] Standings in the ticker: each favorite's place rides on its league header (`NFL  BUF 1ST / AFC EAST 3-0`), or gets its own small segment on days the league has no games
- [x] Standings widget: ESPN standings (divisions for NFL/MLB/NBA/NHL, conferences for WNBA/MLS, the table for soccer; not college), refreshed every 30 min; shows a favorite's group (scrolled so they're visible), else the featured game's; sport-specific columns
- [x] Weather widget: Open-Meteo (free, no key, CC BY 4.0) via `marqueet-provider-openmeteo`; location by city search or "lat, lon" on the admin page, °F/°C; current conditions plus 5 days with drawn icons; fetched every 15 min only while a weather widget shows. The header clock replaces the clock widget
- [x] Weather in the ticker (ticker first: every source gets a ticker presence): an LED segment leading each loop with a multi-color LED icon (`Part::Icon`, icons as data in `assets/icons.txt`), temperature, city, high/low, and a RAIN/SNOW/STORMS heads-up when the chance is 50%+ in the next few days; on by default once a location is set, with its own admin toggle
- [x] Severe weather alerts from the US National Weather Service (`marqueet-provider-nws`; free, no key, public domain), checked every 2 minutes whenever a location is set: in effect → a colored segment at the very front of the ticker (red warning, orange watch, amber advisory, with an LED icon); new severe/extreme warnings take over the screen, watches flash; updates don't re-alert; outside the US the check stops for that place. Admin toggle, on by default
- [x] Admin web page (`/admin`): server-rendered HTML with an escape helper (no template engine), plain CSS, one small hand-written script for drag-to-reorder; works with scripting off and on phones. Open on the device; from the network only with `MARQUEET_ADMIN_PASSWORD` (session cookie, HttpOnly, SameSite=Strict), cross-site posts refused, strict CSP. The server refuses a non-loopback `--listen` without a password
- [x] Layout editor: five presets (wide left, wide right, halves, three, single) as `widget_layout` in the display settings; the admin page shows layout previews and slot boxes that follow the choice with CSS alone, a dropdown per slot (phones, no JS), and drag-a-slot-onto-another to swap on desktop. Widgets scale down to fit narrow slots
- [x] SQLite settings store (`--db`), applied live: leagues and order (pollers start/stop), favorite teams, takeover policy (all / favorites / off), widget slots, display look
- [x] Overnight quiet hours (screen blanks)
- [x] Stale data marked DELAYED on the league header (since Phase 2)
- [x] Time zone setting (IANA name from the system zoneinfo via jiff, with daylight saving; blank = the device's zone): drives start times, quiet hours, and the display's clock (sent as a UTC offset with the display settings)

## Phase 5: fantasy

- [x] Sleeper provider (`marqueet-provider-sleeper`): username / league / team lookup, this week's matchup with starters' live points and ESPN ids, daily player-list cache
- [x] Fantasy setup on the admin page (find by Sleeper username, pick league and team, up to 4 teams); matchups polled every 30 s while NFL games are live, else every 10 min
- [x] Fantasy on the ticker (each followed matchup near the front: teams stacked, the leader's score bright, a flash when the lead changes) and a matchup widget (LED totals, starters side by side by lineup slot)
- [x] Player matching through Sleeper's `espn_id`; fantasy details in takeovers ("YOUR STARTER | J. ALLEN 24.1 PTS", or the opponent's); with "favorites only" takeovers, a play by your starter counts as a favorite's
- [x] Per-widget settings (moved from Phase 4): each slot is `{kind, option}` (a league for Game of the Day / Scores / Standings, a followed team for Fantasy); `WidgetKind` carries its id, label and choices, so the admin page, the form parser and `build_views` share one table. No dynamic trait objects or `schemars`: widgets are built in, and outside code extends the sign through the feed API. Old settings (bare kinds) still load

## Phase 6: kiosk

- [x] Packaging decided ([ADR-0011](adr/0011-packaging-snap.md)): one strictly confined snap with `server` and `display` daemons next to `ubuntu-frame` (gpu-2404 graphics, waits for Frame's Wayland socket), `snap set marqueet reset-password=true`; built in CI for amd64 and arm64. systemd units for from-source installs
- [x] mDNS `marqueet.local`: the installer names the computer and installs Avahi (Ubuntu Core images: with the Phase 7 image work)
- [x] First-boot setup (server): with no admin password, a one-time 6-digit code (5 tries, then a new code) is sent only to displays on the device and logged; `/setup` takes the code plus a new password; remote visitors get only the setup page
- [x] First-boot screen (display): replaces the widget area while setup is pending: a QR code for the setup page (`qrcodegen`), the `.local` and IP addresses, and the code in LED digits; the ticker keeps running above
- [x] Password creation at setup (PBKDF2-SHA256, 600,000 iterations, via `ring`), stored in the database; the code then expires and you're logged in
- [x] SSH is an OS setting, off by default in the image, not an admin toggle (a confined app that could enable SSH is a target; ADR-0011)
- [ ] Optional self-signed HTTPS (moved to the hardening pass)
- [x] Password reset through a file (`--reset-file`, e.g. on the boot partition): back to first-boot setup, acted on once even if the file can't be deleted
- [ ] Optional: WiFi captive portal when there's no Ethernet (deferred: needs network-manager control; wired is the expected setup)

## Phase 7: distribution

- [x] Edge builds: every merge to `dev` publishes both snaps and checksums as the rolling `edge` pre-release
- [x] One-command installer (`install.sh`) for Ubuntu 24.04 on x86 and Raspberry Pi: Frame, Mesa, Avahi, the `marqueet` host name, the checksum-verified snap, connections, boot-to-ticker (asks before turning off a desktop); tested in CI on a fresh machine
- [x] Flashable Raspberry Pi image: Ubuntu Server 24.04 for Pi (Ubuntu's signed checksums verified) plus cloud-init that names it `marqueet`, turns SSH off and runs the installer on first boot (retrying until online); daily update check; `reset-password` file on the SD card; plain-language README on the card; built and checked in CI, published with each edge build
- [ ] Automatic updates through the Snap Store (signed; today the Pi image checks the edge release daily)
- [ ] Release workflow with checksums

## Phase 8: Home Assistant

For people who already use Home Assistant as their home dashboard: the ticker
sits at the top of their dashboard, with no second device or OS needed.

- [ ] Embeddable ticker page (`/embed/ticker`): just the LED ticker, sized for a card, fed by the same `/ws` feed (scores, weather, fantasy, flashes, feed items)
- [ ] Browser renderer: compile the existing wgpu display to WebAssembly (WebGL/WebGPU), so the embed looks the same as the device; still Rust-only, no npm
- [ ] Docs: add it with Home Assistant's built-in Webpage card (nothing to install in Home Assistant)
- [ ] Home Assistant add-on: the Marqueet server as an add-on container (amd64 + arm64) running inside Home Assistant OS, admin page through ingress; for people without a Marqueet device
- [ ] Optional integration: sensors (live games, fantasy score) and a "show message" service built on the feed API

The device display stays native (ADR-0002); the embed is an extra view for
dashboards, not a replacement.

## Later

- [ ] Non-sports sources: stocks, RSS
- [ ] More sports providers as fallbacks for ESPN

## Non-goals

- A browser-based display, or any JavaScript toolchain ([ADR-0001](adr/0001-rust-everywhere-no-js-toolchain.md))
- Physical RGB LED matrix panels
- Cloud accounts or telemetry
- Betting features
