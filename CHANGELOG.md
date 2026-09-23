# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Fixed

- A two-point conversion that arrives in the poll after its touchdown (+6,
  then +2) is now reported as a two-point conversion instead of a safety.

### Changed

- **The crawl is flat text now** (ADR-0007: LED for the ticker only): league
  in amber, matchup in white, start time and TV muted, behind an amber
  TONIGHT / TODAY / UP NEXT tag. It's drawn once when the schedule changes
  and scrolled by the GPU. `crawl_speed` now means tenths of the crawl's
  height per second (the same pace as before); `crawl_rows` is ignored and
  `--crawl-rows` is gone.
- **Renamed from Tickadee to Marqueet** (marquee + parakeet): "Tickadee" is an
  existing time-clock product ([ADR-0008](docs/adr/0008-name-marqueet.md)).
  Crates are now `marqueet-core` / `marqueet-display`, the mascot is a
  parakeet, and the repository moved to `adamson34/marqueet`.

### Added

- **Feed API** ([docs/FEEDS.md](docs/FEEDS.md)): your own scripts, in any
  language, can put things on the sign. Create a feed on the admin page to get
  a token, then `POST /api/feeds/<name>` ticker segments (text, an optional
  second line, an LED icon, a color) and crawl lines, and
  `POST /api/feeds/<name>/alert` a flash or takeover. Content expires unless
  refreshed (15 minutes by default); feeds are limited in size and alert rate.
  Tokens are stored apart from the settings and never appear in
  `/api/settings`.
- `marqueet-display --server URL --screenshot out.png --wait 10` keeps
  listening in real time before capturing, so a live alert can be captured.
- **Takeovers without a score**: feed takeovers show a kicker, headline and
  detail line; `Takeover.score` is now optional.

- **Weather in the ticker**: once a location is set, each ticker loop opens
  with the weather on the LED sign: a color LED icon (sun, moon, clouds,
  rain, snow, storms, fog), the temperature, the city with today's high and
  low, and a red heads-up like `RAIN FRI / 80% CHANCE` when rain, snow or
  storms are 50%+ likely in the next few days. On by default; turn it off on
  the admin page. Ticker segments can now carry multi-color LED icons
  (`Part::Icon`, drawn from `assets/icons.txt`), so display protocol is now
  version 2.
- **Weather widget**: set a location on the admin page (a city name, looked
  up once, or "lat, lon") and °F or °C, then pick Weather for a widget slot.
  Shows current temperature, conditions, feels-like, wind and humidity, and
  five days of highs, lows and rain chances, with icons drawn on the card.
  Data from [Open-Meteo](https://open-meteo.com) (free, no API key, CC BY
  4.0), fetched every 15 minutes, and only while a weather widget is on
  screen: the location isn't sent anywhere otherwise.
- **Standings widget**: pick it for either widget slot on the admin page.
  It shows the group (division, conference or table) of your first favorite
  team, scrolled so that team is visible and highlighted, else the featured
  game's division, else the first league's first group. Columns fit the
  sport: W-L-T-PCT (football), W-L-PCT-GB (baseball, basketball),
  W-L-OTL-PTS (hockey), P-W-D-L-PTS (soccer). Standings come from ESPN's
  standings endpoint every 30 minutes (retry after 10 on failure); college
  leagues have none. `marqueet-display --mock --widgets game_of_the_day,standings`
  shows it with demo data.
- **Time zone setting** (admin page, `time_zone` in settings): an IANA name
  such as `America/Chicago`, with daylight saving, read from the system's
  zoneinfo. It drives start times in the ticker and crawl, the crawl's
  TONIGHT tag, quiet hours and the display's clock. Blank uses the device's
  time zone.
- **Admin page** (Phase 4) at `/admin`: leagues (tick and drag to order),
  favorite teams picked from today's games, takeover policy, widget slots,
  LED color, speeds, rows, glow, flicker and scroll mode, quiet hours, plus
  per-league fetch health and recent plays. Server-rendered HTML, plain CSS
  and one small hand-written script; no template engine, no front-end
  dependencies. Open without login on the device itself; from the network
  only when the server has an admin password (`MARQUEET_ADMIN_PASSWORD` or
  `--admin-password`, 8+ characters), which the server now requires before it
  will listen on a non-loopback address. Sessions are random tokens in an
  HttpOnly, SameSite=Strict cookie; cross-site form posts are refused; pages
  carry a strict Content-Security-Policy. `PUT /api/settings` follows the
  same rules.
- **Settings** (Phase 4): stored in SQLite (`marqueet-server --db`, default
  `marqueet.db`) and applied live: leagues and their order (pollers start and
  stop), favorite teams (they win Game of the Day), takeover policy (all,
  favorites only, or off, where the rest just flash), widget slots, the
  display look (sent to the display, which restyles on the fly), and
  overnight quiet hours (the screen blanks). `GET`/`PUT /api/settings`;
  changes are accepted only from the device itself until the admin page has a
  password. `--leagues` now overrides and saves the stored list.
- **Widgets** (Phase 4): the widget area now shows a **Game of the Day** card
  (big team names, LED-block scores, situation chips like "BUF ball / 2nd & 6
  / KC 14" or "1 out / On 1st & 3rd", and a line score) and a **Scores** card
  (live in red, breaks in amber, finals in green, upcoming muted). The server
  builds ready-to-draw views (`core::widgets`) and sends them with each
  content update; the display fades them in after the welcome logo and
  redraws only when they change (about 3 ms at 1080p on an M3). The LED clock
  in the widget area is gone; the header shows the time.
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

- `GET /api/settings` now follows the admin page's access rules (it includes
  the weather location).
- winit's Wayland client-side decorations are disabled, removing the
  unmaintained `ttf-parser` (RUSTSEC-2026-0192) from the dependency tree.
