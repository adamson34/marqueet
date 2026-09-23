# ADR-0002: The display is a native wgpu app, not a browser kiosk

- **Status:** Accepted
- **Date:** 2026-09-23

## Context

The brief proposed a full-screen web page in WPE WebKit or Chromium under
Ubuntu Frame. On a Raspberry Pi 4, a browser engine costs roughly 200 to 400 MB
of RAM, several seconds of boot time, and makes smooth scrolling a fight.

## Decision

The display (`marqueet-display`) is a native app using wgpu for rendering and
winit for windowing. Under Ubuntu Frame it runs as a Wayland client. It
receives data from the server over a local WebSocket (Phase 2), so a display
crash doesn't stop data collection.

Rendering is limited to WebGL2 / OpenGL ES 3.0 capabilities
(`Limits::downlevel_webgl2_defaults`) so it runs on a Pi 4's GPU. winit's
client-side decorations are disabled; a kiosk never shows a title bar, and
the feature pulled in an unmaintained font parser (RUSTSEC-2026-0192).

## Consequences

- Low memory use, fast startup, GPU-driven animation.
- Text layout, images and animation for widgets must be built rather than
  borrowed from HTML/CSS (Phase 4).
- Pi 3 / Zero 2 W (OpenGL ES 2.0 only) are not supported.
