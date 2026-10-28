# ADR-0016: Camera snapshots from Home Assistant

- **Status:** Accepted
- **Date:** 2026-10-08

## Context

People want their home cameras on the sign: a camera taking turns with the
scores, and the porch camera when the doorbell rings. Cameras speak many
protocols (RTSP, ONVIF, vendor clouds, HomeKit), and showing live video means
decoding H.264 on a Raspberry Pi whose GPU is already busy drawing the sign.
Home Assistant already talks to nearly every camera and serves a still image
of any of them over its REST API (`GET /api/camera_proxy/<entity>`, with a
long-lived access token).

## Decision

- **Home Assistant is the camera source.** A new provider crate
  (`marqueet-provider-homeassistant`) lists `camera.*` entities
  (`GET /api/states`) and fetches snapshots. Marqueet never speaks RTSP or a
  camera vendor's protocol, and never decodes video.
- **Snapshots, not video.** The server fetches a shown camera's snapshot every
  few seconds (never faster than every 2 s), only while a camera widget or a
  camera takeover is on screen. Snapshots are JPEG; the server decodes them
  with `zune-jpeg` (pure Rust, no C, MIT/Apache-2.0/Zlib) and shrinks them to
  at most 960 pixels on the longest side before sending, so a display only
  uploads small RGBA images. In a rotation, the server and the display work
  out whose turn it is from the wall clock the same way
  (`core::widgets::cameras_on_screen`), so only the camera that's up (and
  the next one, just before its turn) is fetched.
- **Their own message.** Snapshots travel in a `Snapshots` message keyed by an
  opaque id, separate from `Logos`: a new logo rebuilds every LED panel, a new
  snapshot only redraws the widget area. Views name the snapshot by key; the
  display never learns entity ids or Home Assistant exists (ADR-0003).
- **A `camera` widget** shows one camera, or the chosen cameras taking turns.
  Feed takeovers may name a camera (`"camera": "<entity>"`), so a doorbell
  automation can show the porch.
- **The token is a secret.** It is stored raw (Marqueet must send it), in its
  own table apart from the settings JSON, like feed tokens, so it never appears
  in `/api/settings` or a settings export. The admin page shows only whether
  one is set. The Home Assistant address must be `http` or `https` on the
  local network or a host the owner typed; requests go nowhere else.

## Consequences

- Any camera Home Assistant can see works, with no camera code in Marqueet.
- Pictures are a few seconds old, not live video. Good enough for a glance at
  the porch; not a security monitor.
- One new dependency (`zune-jpeg`, plus its own `zune-core`, whose option
  types it takes).
- Camera images exist only in memory on the device and on the screen; they are
  never stored or sent anywhere else.
