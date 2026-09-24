# CLAUDE.md

Guidance for AI assistants (and humans) working in this repo. This is the
project's design contract; the reasons behind it are in [docs/adr/](docs/adr/).

## What Marqueet is

An appliance that turns an old monitor plus a Raspberry Pi 4/5, mini PC or old
laptop into a live LED-style sports ticker: a scrolling ticker and crawl on top,
widgets below, flashes and full-screen takeovers when someone scores. Managed
from a local admin web page. Built in phases; see [docs/ROADMAP.md](docs/ROADMAP.md).

## Hard rules

1. **Rust everywhere. No npm, no JavaScript toolchain, no third-party JS.**
   The admin page is server-rendered HTML/CSS; at most a small hand-written
   script. ([ADR-0001](docs/adr/0001-rust-everywhere-no-js-toolchain.md))
2. **Must run on a Raspberry Pi 4.** Rendering stays within WebGL2 / GLES 3.0
   limits. No per-frame CPU work that scales with content.
   ([ADR-0002](docs/adr/0002-native-wgpu-display.md), [ADR-0004](docs/adr/0004-led-rendering-pipeline.md))
3. **Pure core.** `marqueet-core` has no I/O. Parsing, normalization,
   formatting and event detection live there and are unit-tested with fixtures.
4. **LED is for the ticker; widgets are flat UI.** Widgets and takeovers use
   vector text in the chosen theme, with LED-block digits only as accents.
   Never name real teams in code, tests or docs (use "LA baseball").
   ([ADR-0007](docs/adr/0007-led-ticker-flat-widgets.md), [ADR-0012](docs/adr/0012-display-themes.md))
5. **The display is source-agnostic.** Sources emit `TickerSegment`s and
   `Alert`s; the display never learns what a "game" is.
   ([ADR-0003](docs/adr/0003-generic-segments-and-alerts.md))
6. **Few dependencies.** Every new crate needs a reason in the PR. `cargo deny
   check` must pass.
7. **Every change goes through a feature branch and a PR into `dev`.** Never
   commit directly to `dev` or `main`. ([ADR-0006](docs/adr/0006-branching-and-review-flow.md))

## Layout

```
crates/core/     marqueet-core: schema (sports/), ticker segments + rasterizer
                 (ticker.rs), LED font (font.rs, fonts/led5x8.txt), logo
                 (logo.rs, assets/mark.txt), LED icons (icons.rs, assets/icons.txt),
                 weather model + ticker segment (weather.rs), alerts, layout math, config.
crates/provider-espn/  marqueet-provider-espn: ESPN scoreboard fetch + pure
                 normalize (normalize.rs), lenient models (model.rs), league
                 table (leagues.rs), fixtures in tests/fixtures/.
crates/provider-nws/  marqueet-provider-nws: US National Weather Service alerts,
                 pure parser, fixtures in tests/fixtures/.
crates/provider-openmeteo/  marqueet-provider-openmeteo: Open-Meteo forecast and
                 place search, pure parsers, fixtures in tests/fixtures/.
crates/provider-sleeper/  marqueet-provider-sleeper: Sleeper fantasy (user, leagues,
                 teams, matchup with live points), daily player-list cache, pure
                 parsers in parse.rs, anonymized fixtures in tests/fixtures/.
crates/server/   marqueet-server: pure polling policy (schedule.rs), per-league
                 cache (store.rs), games → segments and widgets (content.rs), SQLite
                 settings (settings_store.rs), pollers +
                 shared state (hub.rs), axum routes /ws /api/games /api/alerts (web.rs),
                 the feed API for local scripts (feed_api.rs; validation in core feeds.rs).
                 People's team colors and logos (team_art.rs: PNG, team packs;
                 admin/teams.rs, admin/multipart.rs; ADR-0013). Alerts come from
                 core::events on each poll, deduped by id (hub.rs).
crates/display/  marqueet-display: wgpu renderer (gpu.rs, led.wgsl, render.rs),
                 bands and flashes (band.rs), scene (scene.rs), mock feed
                 (mock.rs), headless capture (screenshot.rs), window loop
                 (app.rs), UI canvas (ui/: canvas.rs, text.rs with bundled fonts in
                 assets/fonts/; ui.wgsl), header bar (header.rs), widgets (widgets.rs; views
                 come from core::widgets) drawn in a theme (theme/: kit + broadcast,
                 ballpark, varsity; ADR-0012; palettes in core::theme), takeovers (takeover.rs: queue, palette, layout;
                 takeover.wgsl: background), live feed client (feed.rs: tungstenite on a background
                 thread, reconnects with backoff).
snap/            The snap (ADR-0011): snapcraft.yaml, launchers in local/, the
                 configure hook (reset-password).
packaging/       systemd units for from-source installs.
media/           Generated logo SVGs (do not hand-edit), the concept
                 mockup GIF, and theme screenshots (themes/, from --mock).
docs/            ROADMAP.md, adr/.
```

## Commands

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
cargo run --release -p marqueet-server [-- --leagues nfl,mlb | --list-leagues]
cargo run --release -p marqueet-display [-- --mock | --server URL | --size 1366x768 | --help]
marqueet-display --mock --screenshot out.png [--at 6 --scroll-to ID --flash ID]
MARQUEET_BLESS=1 cargo test -p marqueet-core logo   # regenerate media/*.svg
```

## Conventions

- Match the surrounding code: small modules, doc comments on public items,
  tests next to the code (`#[cfg(test)] mod tests`).
- Unwraps are fine in tests only (clippy enforces this).
- Visual changes: attach before/after `--screenshot` images to the PR.
- Font and logo are data files; edit the `.txt`, not generated output.
- Keep `docs/ROADMAP.md` checklists and `CHANGELOG.md` current in the same PR
  as the change.
- Commit subjects are short and imperative (`Add ESPN normalizer`).
