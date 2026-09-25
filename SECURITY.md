# Security policy

Marqueet runs on people's home networks, so security reports are taken
seriously.

## Reporting a vulnerability

Please **do not** open a public issue. Report privately through
[GitHub's private vulnerability reporting](https://github.com/adamson34/marqueet/security/advisories/new).

Include what you found, how to reproduce it, and the version or commit. You
should get an acknowledgement within a week.

## Scope

Especially interesting: the admin web page and its authentication, the feed
API and its tokens, the
first-boot setup code, anything reachable from the local network, and
supply-chain issues in dependencies.

## Built-in protections

- **Who can change settings:** anyone on the device itself (loopback); from
  the network, only after logging in, or during first-boot setup with the
  6-digit code shown on the device's screen.
- **Guessing:** setup codes and passwords are checked one at a time across
  the whole server, with a wait after each failure that grows from a second
  to a minute (forgotten after 15 quiet minutes).
- **DNS rebinding:** every request must name the device (an IP address,
  `localhost`, or its own name, e.g. `marqueet.local`); anything else is
  refused. Extra names (a reverse proxy's) can be allowed with
  `--allowed-host` / `MARQUEET_ALLOWED_HOSTS`. The display feed (`/ws`) also
  refuses browser pages from other sites.
- **Cross-site forms:** posts whose `Origin` names another site are refused;
  the session cookie is `SameSite=Strict`.
- **Sessions:** a random token in an `HttpOnly` cookie, kept in memory (a
  restart logs everyone out). A session ends after a week unused or 30 days
  after logging in; changing the password ends all of them.
- **Feed tokens:** 256 random bits each, stored only as a SHA-256 hash and
  shown once. Content posts are limited to one every 2 seconds per feed,
  alerts to one every 5 seconds and takeovers to one every 30.

## Supported versions

The latest 1.0.x release. Fixes land on `dev` first (the Snap Store's `edge`
channel, rebuilt on every merge) and ship to `stable` in the next release.

## What's exposed on your network

The decisions behind this are in
[ADR-0015](docs/adr/0015-security-review-decisions.md).

- **Readable without logging in:** what the screen shows. The display feed
  (`/ws`), `GET /api/games`, `GET /api/alerts` and `/healthz` carry scores,
  the weather town, followed fantasy team names, and whatever your feeds put
  on the sign. Don't put things on the sign you wouldn't show everyone on
  your network.
- **Needs the admin page's access:** settings (`/api/settings`, including the
  exact location), the feed list, team pack downloads, and every change.
- **Plain HTTP:** the admin password, the session cookie and feed tokens
  travel unencrypted on your network. Use Marqueet on a network you trust;
  HTTPS is on the roadmap.
- **Physical access is full access:** a `reset-password` file on the Pi's SD
  card restarts setup, and a `marqueet-ssh-key.pub` file turns on SSH for
  that key ([PI-DEVELOPMENT.md](docs/PI-DEVELOPMENT.md)).
- **The first-boot code** also goes to the server log (for setups without a
  screen), which only root and the `adm` group can read. It stops working
  once a password is set.

## What leaves the device

- **ESPN:** requests for the leagues you follow (and, when the spotlight is
  on a game, that game's summary; team logos only if you turn them on). No
  location, no account.
- **Open-Meteo:** your location's coordinates, every 15 minutes while the
  ticker's weather or the weather widget is on, and the town you search for
  on the admin page.
- **US National Weather Service:** your location's coordinates, every 2
  minutes whenever a location is set and weather warnings are on.
- **Sleeper:** the username you look up, and your leagues' ids.
- **Snap Store / GitHub:** update checks.

Nothing else: no accounts, no telemetry.
