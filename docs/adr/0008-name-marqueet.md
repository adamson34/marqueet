# ADR-0008: The project is named Marqueet

- **Status:** Accepted
- **Date:** 2026-09-23

## Context

The project launched as "Tickadee" (chickadee + ticker). It passed GitHub and
crates.io checks, but a web search showed [Tickadee](https://www.tickadee.com/)
is an established time-clock SaaS that also advertises kiosk options: too
close for an appliance with a kiosk display.

## Decision

Rename to **Marqueet**: *marquee* (the lit sign) + *parakeet*. The mascot
becomes a parakeet (budgie) on the LED dot grid (ADR-0005). Crates are
`marqueet-core` and `marqueet-display`, the hostname will be
`marqueet.local`, and the repository is `adamson34/marqueet` (GitHub
redirects the old URL).

Checks at the time: no product, company or app named Marqueet; free on
crates.io, npm and PyPI; no GitHub repos or user with the name;
`marqueet.com` registered but parked. Trademark search (USPTO) still to do.

## Consequences

- Search engines may autocorrect to "marquee"; the README and repo
  description spell out the pun so it sticks.
- Name checks for future products include a web search for existing
  products, not just code registries.
