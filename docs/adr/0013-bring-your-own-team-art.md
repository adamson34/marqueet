# ADR-0013: People bring their own team colors and logos

- **Status:** Accepted; amended by [ADR-0015](0015-security-review-decisions.md) (logo keys are team ids the display doesn't interpret)
- **Date:** 2026-09-24

## Context

Team logos make a scoreboard look finished, and plenty of people want them.
But logos are copyrighted and trademarked artwork. Shipping them, or
downloading them from a data provider on the user's behalf, would put the
project in the business of distributing other people's art. Team names and
scores are facts and fine to show; logos are not the same.

## Decision

- Marqueet ships no team logos and never fetches or shows them (the
  `logo_url` in ESPN's data only passes through in `/api/games`).
- People can add their own: per team on the admin page (a PNG upload and
  optional colors), or many at once with a **team pack** file
  (`docs/TEAM_PACKS.md`) that they can export, edit and share among
  themselves. The admin page and docs say to use only art they may use.
- Art is stored on the device (SQLite, apart from the settings JSON), shrunk
  to 128 px, and sent to displays in `Logos` messages after the scores, in
  chunks of at most 4 MB of pixels (protocol 5), so no single WebSocket
  message gets near a frame limit.
- Custom colors are applied to the games before anything is built, so the
  ticker, widgets and takeover alerts all use them. Logos appear on the LED
  ticker (a small logo per team line, drawn from the image at LED size) and in
  each theme's widgets. Views carry an opaque logo key; the display never
  learns team ids (ADR-0003).
- Uploads are parsed by a small hand-written `multipart/form-data` reader and
  decoded with the `png` crate (already in the tree), so the page works
  without JavaScript and needs no new crate.

## Consequences

- The feature is useful the moment someone has art, without the project
  hosting any.
- Packs made for one data provider name that provider's team ids; a future
  provider would need a mapping.
- Logos add up to ~64 KB each on the display connection; a device keeps art
  for at most 250 teams (about 16 MB), sent in chunks.

## Amendment (2026-09-24): an opt-in for the provider's logos

People asked for logos without hunting for image files. Marqueet still ships
none, but the owner can now turn on **"Show team logos from ESPN, the scores
service"** (admin page, Look section; `Settings::provider_logos`, **off by
default**). When on, the server downloads the logo link ESPN already puts in
its scoreboard data, only for teams in today's games, only from ESPN's image
CDN over https (`*.espncdn.com`, size-limited), decodes and shrinks it once,
and caches it in SQLite. Logos the owner adds always win. Turning it off
stops showing them at once. This is the owner's choice, like other
self-hosted scoreboards that load ESPN's logo links; the admin page says the
logos belong to the teams.

