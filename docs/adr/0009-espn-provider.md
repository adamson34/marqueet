# ADR-0009: ESPN provider: honest client, lenient parsing, ring TLS

- **Status:** Accepted
- **Date:** 2026-09-23

## Context

ESPN's `site.api.espn.com` scoreboard endpoints are the first data source.
They're unofficial, undocumented, fronted by Akamai, and change shape without
notice. While capturing fixtures, a request that spoofed a desktop Chrome
user agent got `403 Access Denied`, while plain clients got `200`.

## Decision

- **Honest client.** Requests identify as
  `marqueet/<version> (+https://github.com/adamson34/marqueet)`, with 10 s
  timeouts. We never spoof a browser: it's dishonest, and the CDN blocks it.
- **Provider interface in core.** `DataProvider` (`core::provider`) returns
  boxed futures, so providers can be `Box<dyn DataProvider>` without an async
  runtime in core. Providers only fetch and normalize; polling, caching and
  backoff belong to the server.
- **Lenient, isolated parsing.** Models default every field and accept numbers
  as strings or floats. Each event is deserialized on its own; a malformed one
  is skipped and reported in `Scoreboard::skipped`, never fatal to the league.
- **Pure normalizer, fixture-tested.** `normalize()` is a pure function
  tested against real captures plus a labeled synthetic file for live states
  (`crates/provider-espn/tests/fixtures/`). An ignored test hits the live API
  on demand.
- **TLS via rustls + ring**, not reqwest's default aws-lc-rs: a smaller C
  build and permissive licenses. `CDLA-Permissive-2.0` is allowed for the
  Mozilla root-certificate bundle.

## Consequences

- When ESPN changes, the fix is usually a fixture plus a model or normalize
  tweak, and one broken game never blanks the ticker.
- Heavy polling must stay polite (Phase 2 scheduler: about 12 s only while games
  are live, much slower otherwise).
