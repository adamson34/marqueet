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

## Supported versions

Until 1.0, only the latest release (and `main`) receives fixes.
