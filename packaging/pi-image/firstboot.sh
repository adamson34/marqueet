#!/bin/sh
# First boot on the Marqueet Pi image: installs Marqueet while showing plain,
# large progress on the TV (instead of scrolling Linux text), waiting for the
# internet if needed. The full installer output goes to the log.
#
# For testing: MARQUEET_TTY (default /dev/tty1), MARQUEET_INSTALLER (default
# the installer next to this script), MARQUEET_LOG, MARQUEET_ONLINE_CHECK.
set -u

here=$(cd "$(dirname "$0")" && pwd)
tty=${MARQUEET_TTY:-/dev/tty1}
installer=${MARQUEET_INSTALLER:-$here/install.sh}
log=${MARQUEET_LOG:-/var/log/marqueet-install.log}
online_check=${MARQUEET_ONLINE_CHECK:-curl -fsI --max-time 10 https://github.com}

# A full screen of large text: title, a main line, an optional detail line.
show() {
  {
    printf '\033c\n\n\n'
    printf '    M A R Q U E E T\n\n\n'
    printf '    %s\n\n' "$1"
    if [ -n "${2:-}" ]; then printf '    %s\n' "$2"; fi
  } >"$tty" 2>/dev/null || true
}

if [ "$tty" = /dev/tty1 ]; then
  # Large console font, no kernel chatter, and no login prompt drawing over us.
  for font in /usr/share/consolefonts/Uni2-TerminusBold32x16.psf.gz /usr/share/consolefonts/Lat15-TerminusBold32x16.psf.gz; do
    if [ -f "$font" ]; then setfont -C "$tty" "$font" 2>/dev/null && break; fi
  done
  dmesg -n 1 2>/dev/null || true
  systemctl stop getty@tty1.service 2>/dev/null || true
  setterm --blank 0 --powersave off --cursor off >"$tty" 2>/dev/null || true
fi

show "Getting ready..." "Please leave it plugged in."

waited=0
until $online_check >/dev/null 2>&1; do
  if [ "$waited" -ge 20 ]; then
    show "Waiting for the internet..." "Check the network cable, or the WiFi name and password you set in Raspberry Pi Imager."
  else
    show "Connecting to the internet..." "Please leave it plugged in."
  fi
  waited=$((waited + 5))
  sleep 5
done

show "Setting up Marqueet. This takes about 10 minutes." "Please leave it plugged in."
status_file=$(mktemp)
{
  MARQUEET_YES=1 sh "$installer"
  echo $? >"$status_file"
} 2>&1 | tee -a "$log" | while IFS= read -r line; do
  case $line in
    *'==>'*)
      step=$(printf '%s' "$line" | tr -d '\033' | sed 's/\[[0-9;]*m//g; s/^.*==> *//')
      show "Setting up Marqueet. This takes about 10 minutes." "$step..."
      ;;
  esac
done
status=$(cat "$status_file" 2>/dev/null || echo 1)
rm -f "$status_file"

if [ "$status" = 0 ]; then
  show "All set! Starting the ticker..." "Scan the QR code on the next screen with your phone."
else
  show "Something went wrong. Trying again in a minute..." "Details: $log"
fi
exit "$status"
