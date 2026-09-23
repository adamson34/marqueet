<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="media/marqueet-mark-paper.svg">
    <img src="media/marqueet-mark-ink.svg" alt="Marqueet" width="160" height="128" />
  </picture>
</p>

<h1 align="center">Marqueet</h1>

<p align="center">
  <strong>An old monitor + a cheap computer = a live LED sports ticker.</strong><br>
  Pure Rust. Native GPU rendering, no browser. Built to run on a Raspberry Pi 4.<br>
  Sports first; weather, stocks and anything else can plug in.
</p>

<p align="center">
  <a href="https://github.com/adamson34/marqueet/actions/workflows/ci.yml"><img src="https://github.com/adamson34/marqueet/actions/workflows/ci.yml/badge.svg?branch=dev" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/MSRV-1.88-blue" alt="MSRV 1.88">
  <img src="https://img.shields.io/badge/platform-Linux%20%7C%20Raspberry%20Pi-2e7d4a" alt="Platform: Linux | Raspberry Pi">
  <img src="https://img.shields.io/badge/status-phase%201%20of%207-b87325" alt="Status: phase 1 of 7">
</p>

```sh
cargo run --release -p marqueet-display
```

![Concept mockup: header with LIVE badge and clock, LED ticker, crawl of tonight's games, game-of-the-day and scores widgets, then a touchdown takeover in team colors](media/mockup.gif)

> **Concept mockup.** This shows where Marqueet is headed, not what it does
> today. The LED ticker in it is the real renderer (including the score flash);
> the header, crawl styling, widget cards and touchdown takeover are design
> layers composited on top, showing the planned look for Phases 3 and 4
> ([ADR-0007](docs/adr/0007-led-ticker-flat-widgets.md)). Today the widget
> area shows the Marqueet logo and a clock.

## What you get

A big scrolling LED sign across the top of the screen (every letter made of
glowing dots), a crawl of tonight's games beneath it, and clean, readable
widget cards below: game of the day with a line score, a scores board,
standings, your fantasy matchup. When someone scores, their game flashes. When a big
play happens (touchdown, home run, goal), a full-screen *takeover* in team
colors interrupts the widgets for a few seconds, and it tells you when it's
*your* fantasy player.

It boots straight into the display, with no desktop, and you manage it from a
web page on your laptop or phone.

## Why this exists

Sports tickers are great in a bar and absent at home. The options today:

- **MagicMirror²**: a general dashboard, but it's Electron plus a pile of npm
  modules, runs a whole browser, and has no concept of "something just
  happened".
- **mlb-led-scoreboard / nfl-led-scoreboard**: lovely, but they need RGB LED
  matrix panels, a HAT and soldering, and each covers a single league.
- **Tidbyt**: the hardware is no longer sold.
- **DAKboard and similar**: subscriptions, cloud accounts, not real-time.

Marqueet reuses hardware you already have (a spare monitor and a Pi 4, mini
PC or old laptop), draws a convincing LED sign on it with the GPU, and covers
every league in one place. It's a single Rust binary per component with no
JavaScript anywhere: no npm, no bundler, no `node_modules` to keep patched.

## What the ticker shows

| Sport | Live game block |
|---|---|
| Football | Teams, score, quarter and clock, possession `◀` (red inside the red zone) |
| Baseball | Teams, score, `▲`/`▼` inning, outs |
| Basketball | Teams, score, quarter and clock |
| Hockey | Teams, score, period and clock, `PP` on the power play |
| Soccer | Teams, score, half and minute |
| College | AP rank before the team |

Scheduled games show the local start time (plus the weekday when it isn't
today) and the network. Finals show `FINAL` or `F/OT`, with the losing team
dimmed. Team colors are adjusted so navy and near-black teams still glow
(Bills navy becomes Bills blue, not their red secondary). Games are grouped
under league headers: live first, then finals, then upcoming.

The crawl lists what's up next. The widget area shows the Marqueet logo on
startup, then (for now) an LED clock; widgets arrive in Phase 4.

## Install

### From source

```sh
git clone https://github.com/adamson34/marqueet.git
cd marqueet
cargo run --release -p marqueet-display
```

Requires Rust 1.88+. On Linux the display needs a GPU with OpenGL ES 3.0 or
Vulkan (Mesa drivers are fine).

### Device images and installer

Coming in Phases 6 and 7: a flashable Raspberry Pi image and an installer for
Ubuntu on x86, both booting straight into the display under
[Ubuntu Frame](https://ubuntu.com/frame).

## Usage

```sh
# Windowed preview at a common monitor size:
marqueet-display --size 1366x768

# Full screen (F toggles, Esc or Q quits):
marqueet-display --fullscreen

# Tune the look:
marqueet-display --led-color green --speed 30 --glow 0.8 --flicker 0.4
marqueet-display --led-color "#ff3355" --ticker-rows 17 --smooth

# Hide the crawl and give the ticker a quarter of the screen:
marqueet-display --crawl-share 0 --ticker-ratio 0.25
```

Everything also renders headless, which is handy for previews, docs and bug
reports:

```sh
# One frame to a PNG, 6 simulated seconds in, with a game mid-flash:
marqueet-display --screenshot out.png --size 1024x768 \
  --scroll-to mock:nfl:1 --flash mock:nfl:1

# Script a score as if a live alert arrived (updates the game and flashes it):
marqueet-display --screenshot td.png --score mock:nfl:1:home:7 --score-at 5.9 \
  --scroll-to mock:nfl:1

# A frame sequence, then a GIF:
marqueet-display --record frames/ --size 1280x720 --at 0 --duration 8 --fps 12 --flicker 0
ffmpeg -framerate 12 -i frames/frame_%05d.png \
  -vf "scale=720:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=64:stats_mode=diff[p];[b][p]paletteuse=dither=none" \
  ticker.gif
```

`marqueet-display --help` lists every option. Until Phase 2 lands, the display
runs on a built-in mock feed: game clocks tick, and a random live game scores
every 6 to 11 seconds.

## Hardware

| | Minimum | Recommended |
|---|---|---|
| CPU | 64-bit x86_64 or ARM64 | Quad core from the last ~10 years |
| GPU | OpenGL ES 3.0 or Vulkan (Mesa) | Vulkan |
| RAM | 1 GB | 2 GB+ |
| Storage | 16 GB | 32 GB+, A2 SD card or USB SSD |
| Network | Ethernet (WiFi optional) | Ethernet |
| OS | Ubuntu 24.04 LTS + Ubuntu Frame | |

Raspberry Pi 4 (2 GB+) and Pi 5 are the targets; Pi 3 and Zero 2 W are not
supported (OpenGL ES 2.0 only). Intel N100-class mini PCs and most laptops
from about 2012 onward work.

## Scope

**In scope:**

- A full-screen LED-style ticker and crawl, with flashes and takeovers
- Live scores for major leagues through pluggable data providers (ESPN first)
- Fantasy matchups through pluggable fantasy providers (Sleeper first)
- Configurable widgets: game of the day, scores, standings, fantasy, clock, weather
- A local admin page (laptop-first, works on phones), password protected
- An appliance experience: boot to display, first-boot setup code, mDNS `marqueet.local`

**Not in scope:**

- A browser-based display, or any JavaScript toolchain
- Physical RGB LED matrix panels (see mlb-led-scoreboard for that)
- Cloud accounts, telemetry, or anything leaving your network besides data-provider requests
- Betting features
- A general-purpose dashboard framework: new sources plug into the ticker and alerts, not arbitrary layouts

## What's next

Phase 1 (the LED display on mock data) is done. The full plan, with
checklists, lives in [`docs/ROADMAP.md`](docs/ROADMAP.md).

- [x] **Phase 1:** repo, CI, schema, LED renderer, flashes, crawl, welcome logo
- [ ] **Phase 2:** server + ESPN provider, caching and polling, WebSocket feed to the display
- [ ] **Phase 3:** event engine (touchdowns, home runs, goals) and takeover animations
- [ ] **Phase 4:** widget system and admin page
- [ ] **Phase 5:** Sleeper fantasy
- [ ] **Phase 6:** kiosk: Ubuntu Frame, systemd, mDNS, first-boot flow
- [ ] **Phase 7:** flashable image and installer

## Caveats

ESPN's scoreboard endpoints are **unofficial and undocumented**. They can
change or disappear without notice. Marqueet is built to degrade gracefully
(serve the last good data, mark it stale, back off), and providers are
plugins so another source can replace ESPN, but a broken upstream means
stale scores until an update ships. Marqueet is not affiliated with ESPN,
any league, or any team; team names and colors are used only to show scores.

Performance on a real Raspberry Pi 4 hasn't been measured yet. The display
logs its frame rate every 10 seconds so it's easy to check.

## Brand

**Marqueet** = *marquee* (the lit sign) + *parakeet*. The logo is a parakeet
drawn on an LED dot grid. It lives as editable text in
[`crates/core/assets/mark.txt`](crates/core/assets/mark.txt) (plus a head-only
[`favicon.txt`](crates/core/assets/favicon.txt)), and everything is generated
from it: the SVGs in [`media/`](media/) and the welcome screen on the device.
After editing, run `MARQUEET_BLESS=1 cargo test -p marqueet-core logo` to
regenerate the SVGs; CI fails if they drift.

| File | Use |
|---|---|
| `marqueet-mark.svg` | LED amber, for dark backgrounds |
| `marqueet-mark-ink.svg` / `-paper.svg` | Monochrome for light / dark backgrounds |
| `marqueet-favicon*.svg` | Head only, legible at 16 px (the `currentColor` variant follows the page) |

## Project layout

- [CLAUDE.md](CLAUDE.md): architecture, conventions, and the project's design contract.
- [docs/ROADMAP.md](docs/ROADMAP.md): the phased plan with checklists.
- [docs/adr/](docs/adr/): Architecture Decision Records.
- [CONTRIBUTING.md](CONTRIBUTING.md): branches (`dev` is the default; PRs go there), checks, principles.
- [SECURITY.md](SECURITY.md): private vulnerability reporting.
- [CHANGELOG.md](CHANGELOG.md): notable changes.

```
crates/
  core/       No I/O. Sports schema, ticker segments, LED font and rasterizer,
              alerts, layout math, config, logo. Most tests live here.
    assets/   mark.txt, favicon.txt: the logo as dot grids.
    fonts/    led5x8.txt: the LED font as ASCII art.
  provider-espn/  ESPN scoreboard provider: fetch + pure normalize, tested
              against saved real responses.
  display/    Native wgpu + winit app: LED shader, scrolling, flashes,
              welcome screen, mock feed, headless capture.
media/        Generated logo SVGs and the concept mockup GIF.
```

## License

[MIT](LICENSE)
