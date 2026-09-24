# ADR-0014: Edge releases on GitHub, an Ubuntu Server Pi image, and the Snap Store next

- **Status:** Accepted
- **Date:** 2026-09-24
- **Amends:** [ADR-0011](0011-packaging-snap.md) (where it runs, how it updates, what's published)

## Context

ADR-0011 chose a strictly confined snap on Ubuntu Core 24 with updates and
rollback from snapd, and said nothing was published yet. What Phase 7 built
differs, and an adversarial review (ADV-P1-003, -004) found the documents
claiming protections that aren't there:

- The Pi image is **Ubuntu Server 24.04 for Raspberry Pi**, not Ubuntu Core:
  it works with Raspberry Pi Imager's WiFi and account settings, and the
  installer (Frame, Mesa, Avahi, the host name) is the same one used on
  laptops and mini PCs.
- Builds are **published on every merge to `dev`** as the rolling `edge`
  GitHub pre-release, and devices install from it.
- Snaps from GitHub are installed with `snap install --dangerous`, so snapd
  doesn't verify a signature and doesn't refresh or revert them. Updates are
  the installer run again (daily on the Pi image), with no rollback.
- `SHA256SUMS` comes from the same release, so it catches a broken download,
  not a forged one.

## Decision

1. The Pi image stays Ubuntu Server 24.04 (preinstalled, Ubuntu's signed
   checksums verified at build time), with Marqueet's first-boot and update
   services in the system and SSH off (a developer switch aside, see
   `docs/PI-DEVELOPMENT.md`).
2. Until the Snap Store: `edge` is published only after the full CI checks
   pass on the same commit (`edge.yml` runs `ci.yml` first), and third-party
   build inputs are pinned (the `gpu-2404` part to a commit), so what ships
   is what was tested and reviewed.
3. The checksum is described as a download check, not a signature.
4. **Next: the Snap Store.** Publishing `marqueet` to the store's `edge`
   channel replaces the GitHub download and `--dangerous`: the store signs
   every revision and snapd verifies it, refreshes automatically and can
   `snap revert`. The installer then runs `snap install marqueet --channel=…`.
   This needs the project's store account, the name registered, store
   credentials as a CI secret, and auto-connection approval for the `wayland`
   and `gpu-2404` plugs. Our own signing of `SHA256SUMS` was considered and
   dropped in favor of this.
5. Ubuntu Core stays a possible later image; mDNS on it would come with that.
6. **Status (2026-09-24):** the `marqueet` name is registered; `edge.yml`
   uploads both snaps to the store's `edge` channel after CI (a separate job,
   so a store review never blocks the GitHub release). The installer installs
   from the store when the channel has a build, moves a copy installed from a
   GitHub download over with `snap refresh --amend` (keeping its settings),
   and falls back to the GitHub download otherwise (`MARQUEET_NO_STORE=1`
   forces that). Auto-connection of `wayland` and `gpu-2404` is requested
   from the store; until then the installer connects them.

## Consequences

- Until the store is live, a compromised GitHub account or `dev` branch can
  still reach devices within a day; CI gating and pinning narrow what an
  honest mistake can ship, not what an attacker can. There are no deployed
  devices besides test hardware yet.
- The Pi keeps running the installer copy on its boot partition until the
  store switch-over, when updates move to snapd.
