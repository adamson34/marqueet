# Security review: findings and what became of them

An adversarial review compared the docs with the code before v1 (27
findings: 6 high, 14 medium, 7 low). The high ones were fixed before v1.0.0;
the rest were triaged against v1.0.0 afterwards. Nothing is dropped
silently: each finding is **fixed**, **already fixed**, **documented** (a
decision written down instead of a code change), or **deferred** (on the
roadmap). Decisions are in [ADR-0015](adr/0015-security-review-decisions.md).

| ID | Finding | Outcome |
|---|---|---|
| ADV-P1-001 | Setup code could be brute-forced; code swapping locked the owner out; login floods pinned the CPU | Already fixed (v1.0.0): one check at a time server-wide with growing waits; wrong codes don't change the code (`admin/auth.rs`) |
| ADV-P1-002 | DNS rebinding through "no login on the device"; `/ws` open to any origin | Already fixed (v1.0.0): Host allowlist on every route and an Origin check on `/ws` (`hosts.rs`) |
| ADV-P1-003 | Devices installed whatever landed on `dev`, unsigned | Already fixed (v1.0.0): edge publishes only after CI passes; installs come from the Snap Store (store-signed, snapd verifies and can revert); `gpu-2404` pinned ([ADR-0014](adr/0014-edge-releases-and-server-image.md)) |
| ADV-P1-004 | ADR-0011 didn't match what ships | Already fixed (v1.0.0): ADR-0014 amends it |
| ADV-P1-005 | Feed `parts` had no width limit | Already fixed (v1.0.0): at most 24 parts, gaps of at most 64 columns, strips capped ([FEEDS.md](FEEDS.md)) |
| ADV-P1-006 | Unbounded team art could stop the display connecting | Already fixed (v1.0.0): 250 teams with art, logos sent in chunks of at most 4 MiB after the content, provider logos capped at 400 |
| ADV-P1-007 | ROADMAP/CHANGELOG said the server refuses to listen without a password | Already fixed: the ROADMAP describes setup mode; the CHANGELOG's older line is history, and a later entry in the same release says it changed |
| ADV-P1-008 | ADR-0001 said askama and Rust plugins | Documented: ADR-0015 amends ADR-0001 (no template engine; extension through the feed API) |
| ADV-P1-009 | Claims about when the location leaves the device were wrong | Fixed: the ROADMAP line is corrected, and [SECURITY.md](../SECURITY.md) lists what goes to each provider and when |
| ADV-P1-010 | `/ws`, `/api/games`, `/api/alerts` open on the LAN while `/api/settings` is gated | Documented: what the screen shows is readable on the LAN by design; everything else needs admin access (ADR-0015, SECURITY.md) |
| ADV-P1-011 | README three phases behind | Fixed: status, mockup note, "What's next", scope, caveats and layout brought up to v1; the stale "Device images" section removed |
| ADV-P1-012 | CLAUDE.md layout left out security modules and the Pi image | Fixed: auth, password, hosts, device, tz, the admin modules, `install.sh` and `packaging/pi-image/` are listed |
| ADV-P1-013 | The display's `--mock` breaks "never learns what a game is"; logo keys are team ids | Documented: the `--mock` exception is in CLAUDE.md and ADR-0015; ADR-0013's "opaque" now means "not interpreted", which is what the code does |
| ADV-P1-014 | A real score after a correction was hidden by the dedupe id | Fixed ([#72](https://github.com/adamson34/marqueet/pull/72)): an id sent before a game's last correction can be sent again after 10 minutes (quick review-and-restore still announces once); tested |
| ADV-P1-015 | Pi 4 performance never checked | Fixed, with a gap deferred: measured on a Pi 4 (22 to 28 fps at 1080p, about 9 at 4K) and recorded in the ROADMAP and README; 60 fps and a hardware release check are on the roadmap |
| ADV-P1-016 | Undocumented SSH switch on the Pi image | Already fixed and documented: [PI-DEVELOPMENT.md](PI-DEVELOPMENT.md) and the ROADMAP describe it; the physical-access threat model is in ADR-0015 and SECURITY.md. "SSH is on" on the screen is deferred |
| ADV-P1-017 | SECURITY.md "Supported versions" named things that didn't exist | Fixed: the latest 1.0.x; fixes land on `dev` (edge) and ship in the next release |
| ADV-P1-018 | Plain HTTP, cleartext feed tokens, sessions that never expire | Fixed ([#72](https://github.com/adamson34/marqueet/pull/72)): sessions end after a week unused or 30 days, and on a password change; feed tokens stored as SHA-256 and shown once, with **New token**. Plain HTTP accepted and documented until HTTPS (deferred, ADR-0015) |
| ADV-P1-019 | Offline start and the Pi's missing clock unspecified | Documented and deferred: ADR-0015 describes the gaps; "SETTING CLOCK" and an on-disk snapshot are on the roadmap |
| ADV-P1-020 | "Never name real teams" conflicts with recorded fixtures; broken in docs | Fixed: recorded fixtures and their assertions are carved out in CLAUDE.md; the README, ROADMAP and TEAM_PACKS examples use stand-ins |
| ADV-P1-021 | Feed takeovers under "favorites only" unspecified | Fixed ([#72](https://github.com/adamson34/marqueet/pull/72)): FEEDS.md says feed and weather takeovers still take over |
| ADV-P1-022 | Feed content posts had no rate limit | Fixed ([#72](https://github.com/adamson34/marqueet/pull/72)): one content post every 2 seconds per feed (429 with `Retry-After`); tested |
| ADV-P1-023 | An open ROADMAP item contradicted ADR-0012 | Fixed: marked superseded (takeovers keep LED text; play text is in the spotlight strip) |
| ADV-P1-024 | ADR decisions and the MSRV not enforced | Fixed ([#72](https://github.com/adamson34/marqueet/pull/72)): `cargo deny` bans OpenSSL, aws-lc and ttf-parser; a CI job checks Rust 1.88 |
| ADV-P1-025 | Phase 8 contradicts the Non-goals and has no ADR | Documented and deferred: the README's non-goal is reworded, and the ROADMAP says Phase 8 starts with an ADR (browser renderer, auth behind the add-on's proxy, protocol version checks) |
| ADV-P1-026 | The reset file is checked only at startup | Fixed ([#72](https://github.com/adamson34/marqueet/pull/72)): the `--reset-file` help and README say so (the snap and the Pi image restart the server for you) |
| ADV-P1-027 | Small index and reference drift | Fixed: ADR index titles and statuses, ADR-0004's font size; FEEDS.md now covers `GET /api/feeds`, the name limit and how `ttl` is clamped ([#72](https://github.com/adamson34/marqueet/pull/72)) |

## Other items from the hardening list

| Item | Outcome |
|---|---|
| No way to change the admin password | Fixed ([#72](https://github.com/adamson34/marqueet/pull/72)): an **Admin password** section, throttled like login, ending every other session |
| Admin sessions only in memory | Documented: a restart logs everyone out, which is the safer default for an appliance |
| Login rate limiting was a 1-second delay | Already fixed (ADV-P1-001) |
| The first-boot code is written to the log | Documented: kept for setups without a screen; the log is root/`adm` only and the code dies with setup (ADR-0015) |
| No HTTPS | Deferred (ADR-0015, ROADMAP) |
| `/api/games` and `/ws` open on the network | Documented (ADV-P1-010) |

## Next pass

Re-check the fixes above, then the areas the review suggested next: theme-code
parsing, the multipart reader (`admin/multipart.rs`), team-pack PNG decoding
cost, and the Sleeper cache file across snap revisions.
