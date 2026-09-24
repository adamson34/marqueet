#!/bin/sh
# Marqueet installer: turns this computer into a Marqueet sports ticker.
#
#   curl -fsSL https://raw.githubusercontent.com/adamson34/marqueet/dev/install.sh | sudo sh
#
# Works on Ubuntu 24.04 (Raspberry Pi 4/5 with the 64-bit image, mini PCs,
# old laptops). It installs Ubuntu Frame and Marqueet, names the computer
# "marqueet" (so the setup page is at http://marqueet.local:7878), and makes it
# boot straight into the ticker. Run it again to update.
#
# Options (environment variables):
#   MARQUEET_CHANNEL   release to install from (default: edge)
#   MARQUEET_SNAP      install this .snap file instead of downloading
#   MARQUEET_HOSTNAME  computer name (default: marqueet; "keep" to leave it)
#   MARQUEET_YES=1     don't ask before turning off a desktop
#   MARQUEET_UPDATE=1  only update Marqueet (skip system setup); does nothing
#                      when the latest build is already installed
set -eu

REPO=adamson34/marqueet
CHANNEL=${MARQUEET_CHANNEL:-edge}
NAME=${MARQUEET_HOSTNAME:-marqueet}

say() { printf '\033[1;33m==>\033[0m %s\n' "$*"; }
die() {
  printf '\033[1;31mMarqueet install failed:\033[0m %s\n' "$*" >&2
  exit 1
}

# Yes/no question on the terminal (works when piped from curl).
confirm() {
  [ "${MARQUEET_YES:-}" = 1 ] && return 0
  [ -r /dev/tty ] || return 1
  printf '%s [y/N] ' "$1" >/dev/tty
  read -r answer </dev/tty || return 1
  case $answer in [yY]*) return 0 ;; *) return 1 ;; esac
}

[ "$(id -u)" -eq 0 ] || die "run it with sudo: curl -fsSL https://raw.githubusercontent.com/$REPO/dev/install.sh | sudo sh"
command -v apt-get >/dev/null 2>&1 || die "this needs Ubuntu (24.04 recommended)."

arch=$(dpkg --print-architecture 2>/dev/null || uname -m)
case $arch in
  amd64 | x86_64) arch=amd64 ;;
  arm64 | aarch64) arch=arm64 ;;
  *) die "this computer's processor ($arch) isn't supported. Marqueet needs 64-bit x86, or a Raspberry Pi 4/5 running the 64-bit image." ;;
esac

STATE=/var/lib/marqueet-installer
UPDATE=${MARQUEET_UPDATE:-}
if [ -n "$UPDATE" ] && ! snap list marqueet >/dev/null 2>&1; then
  UPDATE=  # nothing to update yet: do the full install
fi

if [ -z "$UPDATE" ]; then
  say "Installing system packages"
  export DEBIAN_FRONTEND=noninteractive
  apt-get update -q
  apt-get install -y -q snapd avahi-daemon curl ca-certificates
  systemctl enable --now snapd.socket >/dev/null 2>&1 || true
  snap wait system seed.loaded

  if [ "$NAME" != keep ] && [ "$(hostname)" != "$NAME" ]; then
    say "Naming this computer \"$NAME\" (reachable as $NAME.local)"
    hostnamectl set-hostname "$NAME"
    if grep -q '^127\.0\.1\.1' /etc/hosts; then
      sed -i "s/^127\.0\.1\.1.*/127.0.1.1 $NAME/" /etc/hosts
    else
      echo "127.0.1.1 $NAME" >>/etc/hosts
    fi
    systemctl restart avahi-daemon || true
  fi

  if [ "$(systemctl get-default)" = graphical.target ]; then
    echo
    echo "This computer starts a desktop. Marqueet needs the screen to itself."
    if confirm "Make it start straight into the ticker instead? (undo later: sudo systemctl set-default graphical.target)"; then
      systemctl set-default multi-user.target
      reboot_needed=1
    else
      die "cancelled; nothing about the desktop was changed."
    fi
  fi

  say "Installing Ubuntu Frame (the full-screen display system)"
  snap install ubuntu-frame
  snap install mesa-2404
  snap set ubuntu-frame daemon=true
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
if [ -n "${MARQUEET_SNAP:-}" ]; then
  snap_file=$MARQUEET_SNAP
else
  say "Downloading Marqueet ($CHANNEL, $arch)"
  base="https://github.com/$REPO/releases/download/$CHANNEL"
  curl -fsSL --retry 3 -o "$tmp/marqueet_$arch.snap" "$base/marqueet_$arch.snap" ||
    die "couldn't download Marqueet. Is this computer connected to the internet?"
  curl -fsSL --retry 3 -o "$tmp/SHA256SUMS" "$base/SHA256SUMS" || die "couldn't download the checksums."
  (cd "$tmp" && grep " marqueet_$arch.snap\$" SHA256SUMS | sha256sum -c --quiet -) ||
    die "the download didn't match its checksum; try again."
  snap_file="$tmp/marqueet_$arch.snap"
  sum=$(sha256sum "$snap_file" | cut -d' ' -f1)
  if [ -n "$UPDATE" ] && [ "$(cat "$STATE/installed.sha256" 2>/dev/null)" = "$sum" ]; then
    say "Marqueet is up to date."
    exit 0
  fi
fi

say "Installing Marqueet"
snap install --dangerous "$snap_file"
snap connect marqueet:wayland ubuntu-frame:wayland
snap connect marqueet:gpu-2404 mesa-2404:gpu-2404 2>/dev/null || true
snap restart marqueet >/dev/null
mkdir -p "$STATE"
if [ -n "${sum:-}" ]; then echo "$sum" >"$STATE/installed.sha256"; fi

echo
say "Marqueet is installed."
if [ "${reboot_needed:-}" = 1 ]; then
  echo "    Restart this computer (sudo reboot). It will start straight into the ticker."
fi
echo "    Look at the screen: scan the QR code with your phone (or open"
echo "    http://$(hostname).local:7878/setup), enter the 6-digit code, and choose a password."
