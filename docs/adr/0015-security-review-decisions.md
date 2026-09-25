# ADR-0015: Decisions from the first security review

- **Status:** Accepted; amends [ADR-0001](0001-rust-everywhere-no-js-toolchain.md), [ADR-0003](0003-generic-segments-and-alerts.md) and [ADR-0013](0013-bring-your-own-team-art.md)
- **Date:** 2026-09-25

## Context

An adversarial review before v1 compared the docs with the code
([SECURITY-REVIEW.md](../SECURITY-REVIEW.md) lists every finding and what
happened to it). The six high findings were fixed before v1. Several medium
and low ones are not bugs but places where the docs promised something the
product doesn't do, or where a trust boundary was never written down. This
records those decisions in one place.

## Decision

1. **No template engine; extend through the feed API** (amends ADR-0001).
   The admin page is built with plain `format!` and an escape helper, not
   askama. Outside code extends the sign through the HTTP feed API
   ([FEEDS.md](../FEEDS.md)) in any language; there are no compiled-in
   plugins. "Rust everywhere" is about Marqueet's own code.

2. **What the local network can read.** The display feed (`/ws`),
   `GET /api/games`, `GET /api/alerts` and `/healthz` need no login: they
   carry what the screen shows (scores, the weather town, followed fantasy
   team names, feed content), and other screens and local scripts read them.
   Anything that isn't on the screen needs the admin page's access: settings
   (`/api/settings`, which includes the exact location), feed tokens (only
   hashes are kept), the feed list, and team pack downloads (the logos
   themselves ride on `/ws`, since the screen shows them). The first-boot setup code
   is only sent to displays on the device itself. Don't put things on the
   sign you wouldn't show everyone on your network.

3. **Plain HTTP on the local network, for now.** The admin password,
   session cookie and feed tokens cross the LAN unencrypted. Accepted until
   HTTPS lands, because a home appliance can't get a trusted certificate
   without a cloud service, and a self-signed one teaches people to click
   through warnings. Mitigations: sessions end after a week unused or 30
   days, the password can be changed (ending every session), feed tokens are
   hashed at rest and can be replaced, and guessing is throttled.

4. **Physical access is full access.** Whoever holds the SD card (or the
   boot partition) controls the device: `reset-password` restarts setup, and
   `marqueet-ssh-key.pub` turns on key-only SSH for the default user (the
   developer switch, [PI-DEVELOPMENT.md](../PI-DEVELOPMENT.md)). Both are
   deliberate: the audience is people who own the device, and neither is
   reachable from the network. Showing "SSH is on" on the screen is a
   possible follow-up.

5. **The display's `--mock` mode is the one exception to "the display never
   learns what a game is"** (amends ADR-0003). It builds demo content with
   core's sports model and event engine for development, screenshots and
   the mock `--score` alert; with a server, the display only gets segments,
   alerts and widget views.

6. **Logo keys are strings the display doesn't interpret** (amends
   ADR-0013). Today a logo's key is the team id (`espn:nfl:…`). The display
   only matches keys between views and the logo set; it never parses them.
   That's "opaque" in the sense that matters (no sports logic in the
   display), not secret.

7. **The first-boot code is also written to the server log**, for headless
   setups (no screen). The log is readable only by root and the `adm` group
   on the device, and the code stops working once the password is set.

8. **Offline start and the missing clock are known gaps.** After a power cut
   with no internet the ticker starts empty (scores are kept in memory), and a
   Pi has no battery clock, so until it syncs the time is wrong: the header
   clock, night mode, feed expiry and certificate checks. Both are on the
   roadmap (show "SETTING CLOCK" and hold time-based behavior until the
   clock syncs; keep the last snapshot on disk).

## Consequences

- SECURITY.md describes the network exposure, plain HTTP and physical
  access, so nobody has to infer them.
- Phase 8 (Home Assistant) needs its own ADR before it starts: a browser
  renderer, authentication behind the add-on's proxy (where every request
  comes from the proxy's address), and refusing a display whose protocol
  version doesn't match.
