# ADR-0007: LED for the ticker; flat UI cards for widgets and takeovers

- **Status:** Accepted
- **Date:** 2026-09-23

## Context

A first mockup drew the widget area as more LED panels (dot text for the game
of the day, standings and fantasy). It read as one giant sign, but dense
information (line scores, standings, fantasy points) was hard to scan, and it
didn't look like the product people want on their wall. The preferred design
(see `media/mockup.gif`) keeps the LED look where it shines and uses clean UI
everywhere else.

## Decision

- **Top: LED.** The main ticker stays the dot-matrix LED sign (ADR-0004),
  including score flashes.
- **Header bar:** a slim strip above the ticker with a LIVE badge, the league
  filter and an LED-style clock.
- **Crawl:** flat condensed text on a dark strip with an amber TONIGHT tag
  (league in amber, matchup in white, time muted).
- **Widgets:** rounded dark cards with condensed sans-serif vector text.
  LED-block digits are an accent (the big score on the game of the day), not
  the body text. Status colors: red live, green final, muted scheduled.
- **Takeovers:** fill the widget area with team-color diagonal stripes and a
  dot texture, a large LED-block headline (TOUCHDOWN, HOME RUN, GOAL), the
  play text, a score box, and a "your player" pill for fantasy.

## Consequences

- The display needs a second renderer for the widget area: rounded rects,
  vector text shaping and rendering, and a bundled condensed font under an
  open license (e.g. SIL OFL). This is Phase 4 work; the text stack must pass
  `cargo deny` (ADR-0002 already removed one unmaintained font crate).
- LED-block text in the UI reuses the project's LED font (`fonts/led5x8.txt`)
  drawn as square pixels, so the ticker and the accents share one glyph set.
- The LED renderer stays focused on the ticker and short LED accents.
