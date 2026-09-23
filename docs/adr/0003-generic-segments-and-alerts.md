# ADR-0003: Sources produce generic ticker segments and alerts

- **Status:** Accepted
- **Date:** 2026-09-23

## Context

Sports is the flagship, but users will want weather, stocks and other feeds.
If the display understood "games", every new source would need display
changes.

## Decision

- Sources describe ticker content as `TickerSegment`s: an id plus parts
  (single-line text, two-line stacks, gaps) made of spans with tints (primary,
  dim, accent, or an explicit color). They never deal with pixels.
- Attention-grabbing moments are `Alert`s with a level (`Flash` or
  `Takeover`), an optional segment to flash, title, detail and theme colors,
  and a deterministic id for de-duplication.
- Sports keeps its rich normalized schema (`core::sports`) and formats games
  into segments in one place (`core::sports::ticker`).

## Consequences

- The display is source-agnostic; a stocks provider only has to emit segments
  and alerts.
- Segment formatting is pure and unit-tested with fixtures.
- Rich widgets (line scores, standings) still need source-specific data; the
  widget system (Phase 4) consumes typed data, not segments.
