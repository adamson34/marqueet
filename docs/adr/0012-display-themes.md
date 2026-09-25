# ADR-0012: Display themes: a style plus a palette

- **Status:** Accepted
- **Date:** 2026-09-24

## Context

The first flat UI under the ticker (ADR-0007) was near-black rounded cards,
small spaced-out amber labels and a big number between two team names. It
looked like a generic dashboard, not something from sports: swap the scores
for stock prices and nothing about the design would change. Mockups of three
directions grounded in real sports places (TV score graphics, a hand-operated
ballpark scoreboard, jersey lettering) all read better. Different people want
different ones, and people who aren't sports fans (weather, news feeds) want
something neutral, so we let people choose and build their own instead of
picking one.

## Decision

- A **theme** is a **style** plus a **palette** plus a team-colors switch,
  stored in `DisplayConfig` and sent to the display with the rest of the look.
- **Styles** decide shapes and fonts. Three ship:
  - **Broadcast** (the default): TV graphics. The game of the day is split
    into the teams' colors with a score box on a slanted seam; other games are
    score bugs; the crawl is a white lower-third line with a slanted tag.
    Neutral enough for weather and news.
  - **Ballpark**: a painted scoreboard with stencil lettering; every number
    on a black plate; a possession bulb; an out-of-town board for scores.
  - **Varsity**: jersey lettering; scores as two-color tackle-twill numbers
    on each team's color, with a center patch for the clock.
- A **palette** is nine colors by role (background, cards, score boxes, text,
  labels, highlight, live, crawl, crawl text). Each style has its own preset;
  people can change any of them. The admin page warns about hard-to-read
  pairs rather than refusing them.
- **Team colors** come from the data (ESPN), never from team names in our
  code. Pure rules in `core::theme` keep them readable: white text unless it
  would be hard to read, a team's second color when its first vanishes into
  the background, and one team switching colors when both look alike. Turning
  team colors off keeps everything in the palette.
- The display draws shared widgets (standings, weather, fantasy) once, through
  a resolved theme kit; each style draws its own game of the day, scores list,
  crawl and tag.
- Fonts are bundled, all SIL OFL: Barlow Condensed (including ExtraBold
  Italic), Big Shoulders Display and Stencil Display, and Graduate.

## Consequences

- Widget view models now carry team colors and scores as data
  (`Side.colors`, `RowTeam`), so protocol version 3.
- About 420 KB more fonts in the display binary.
- A new style is a module in `crates/display/src/theme/` plus a preset
  palette; the shared widgets pick it up for free.
- Still one CPU draw per content change (ADR-0004): outlines, polygons and
  mesh dots cost nothing per frame.
- The ticker stays LED in every theme (ADR-0007). The header bar and score
  takeovers follow the look too: takeovers keep their GPU background and LED
  text, with per-look colors and a pattern (stripe direction, width, drift,
  light dots or a dark mesh) passed to the shader.
