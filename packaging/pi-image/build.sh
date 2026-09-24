#!/bin/sh
# Builds the Marqueet Raspberry Pi image from Ubuntu Server 24.04 for
# Raspberry Pi: verifies Ubuntu's signed checksums, adds the installer to the
# boot partition and Marqueet's services to the system, and writes
# marqueet-pi.img.xz to $1.
# Needs Linux with sudo, losetup, gpg, xz and curl.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
out=$(mkdir -p "${1:-dist}" && cd "${1:-dist}" && pwd)
work=$(mktemp -d)
base=https://cdimage.ubuntu.com/releases/24.04/release
# Ubuntu CD Image Automatic Signing Key (2012)
ubuntu_key=843938DF228D22F7B3742BC0D94AA3F0EFE21092

cleanup() {
  sudo umount "$work/boot" "$work/root" 2>/dev/null || true
  if [ -n "${loop:-}" ]; then sudo losetup -d "$loop" 2>/dev/null || true; fi
  rm -rf "$work"
}
trap cleanup EXIT

echo "==> Verifying Ubuntu's checksums"
curl -fsSL -o "$work/SHA256SUMS" "$base/SHA256SUMS"
curl -fsSL -o "$work/SHA256SUMS.gpg" "$base/SHA256SUMS.gpg"
export GNUPGHOME="$work/gnupg"
mkdir -m 700 "$GNUPGHOME"
gpg --batch --quiet --keyserver hkps://keyserver.ubuntu.com --recv-keys "$ubuntu_key"
gpg --batch --quiet --verify "$work/SHA256SUMS.gpg" "$work/SHA256SUMS"

name=$(grep -oE 'ubuntu-24\.04(\.[0-9]+)?-preinstalled-server-arm64\+raspi\.img\.xz' "$work/SHA256SUMS" | sort -V | tail -1)
[ -n "$name" ] || { echo "no Raspberry Pi server image listed" >&2; exit 1; }
echo "==> Downloading $name"
curl -fL --retry 3 -o "$work/$name" "$base/$name"
(cd "$work" && grep " \*$name\$" SHA256SUMS | sha256sum -c -)
xz -dT0 "$work/$name"
img="$work/${name%.xz}"

echo "==> Adding Marqueet's first-boot setup"
# The services go into the system itself, not cloud-init, so Raspberry Pi
# Imager's own settings (WiFi, name, account) can be used freely: Imager
# replaces cloud-init's user-data, and Marqueet still installs itself.
loop=$(sudo losetup -Pf --show "$img")
mkdir "$work/boot" "$work/root"
sudo mount "${loop}p1" "$work/boot"
sudo mount "${loop}p2" "$work/root"

sudo mkdir -p "$work/boot/marqueet"
sudo cp "$here/../../install.sh" "$work/boot/marqueet/"
sudo cp "$here/README.txt" "$work/boot/MARQUEET-README.txt"

units="$work/root/etc/systemd/system"
sudo cp "$here"/*.service "$here"/*.timer "$here"/*.path "$units/"
enable_unit() { # unit, target: what `systemctl enable` would do
  sudo mkdir -p "$units/$2.wants"
  sudo ln -sf "../$1" "$units/$2.wants/$1"
}
enable_unit marqueet-firstboot.service multi-user.target
enable_unit marqueet-reset.path multi-user.target
enable_unit marqueet-update.timer timers.target
# An appliance: no remote logins.
sudo ln -sf /dev/null "$units/ssh.service"
sudo ln -sf /dev/null "$units/ssh.socket"

sync
sudo umount "$work/boot" "$work/root"
sudo losetup -d "$loop"
loop=

echo "==> Compressing"
xz -T0 -6 -c "$img" >"$out/marqueet-pi.img.xz"
ls -l "$out/marqueet-pi.img.xz"
