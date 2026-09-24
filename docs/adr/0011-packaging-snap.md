# ADR-0011: Ship as a snap; SSH and mDNS belong to the OS image

- **Status:** Accepted
- **Date:** 2026-09-24

## Context

Marqueet is an appliance: an old monitor plus a Raspberry Pi 4/5, mini PC or
old laptop that boots straight into the ticker, keeps itself up to date, and
survives a bad update. The display runs as a Wayland client of Ubuntu Frame
(ADR-0002), and Ubuntu Frame is distributed as a snap. Phase 6 had to decide
between snaps and `.deb` packages, and where system concerns (SSH, the
`marqueet.local` name) live.

## Decision

1. **Marqueet ships as one strictly confined snap, `marqueet`,** with two
   daemons: `server` (network, network-bind) and `display` (wayland, the
   `gpu-2404` content interface from `mesa-2404`, network to reach the
   server on loopback). It runs next to the `ubuntu-frame` snap on Ubuntu
   Core 24 (the image in Phase 7) and on classic Ubuntu 24.04.
   - Automatic updates and rollback (`snap revert`) come from snapd.
   - Strict confinement limits what a compromised server could touch.
   - Settings live in `$SNAP_DATA` (copied per revision, so a revert also
     restores the settings that revision used).
   - A forgotten password is reset with `snap set marqueet reset-password=true`
     (a configure hook drops the reset file the server already understands).
2. **Plain systemd units** in `packaging/systemd/` remain for people building
   from source on a classic distro; they're documented, not the main path.
3. **SSH is an OS setting, not an admin-page toggle.** Ubuntu Core has no
   SSH access until keys are added; on classic Ubuntu it's
   `systemctl disable --now ssh`. A confined app able to turn SSH on would be
   a privilege worth attacking, so Marqueet doesn't have one.
4. **`marqueet.local`** comes from the OS: the image sets the hostname and
   runs an mDNS responder (Avahi on classic Ubuntu). On Ubuntu Core this is
   part of the Phase 7 image work.
5. **Deferred:** the WiFi captive portal (needs network-manager control and a
   whole UI; wired Ethernet is the expected setup) and publishing to the Snap
   Store (needs the project's store account and the `marqueet` name
   registered).

## Consequences

- CI builds the snap for amd64 and arm64 on changes to `snap/` and on `main`,
  and uploads it as an artifact; nothing is published yet.
- On classic Ubuntu the display needs its Wayland plug connected once:
  `snap connect marqueet:wayland ubuntu-frame:wayland` (auto-connection needs
  a store assertion, requested when the snap is published).
- The display's snap wrapper waits for Frame's Wayland socket, so start order
  doesn't matter.
