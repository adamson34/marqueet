#!/bin/sh
# Developer switch for the Marqueet Pi image. SSH is off in the image (an
# appliance has no remote logins). To turn it on for development, put a
# public key in a file named "marqueet-ssh-key.pub" on the SD card's boot
# partition: at the next start this adds it for the account made in
# Raspberry Pi Imager and starts SSH with keys only (no passwords, no root).
# A file named "marqueet-ssh-off" turns SSH off again.
#
# Runs at boot from marqueet-devssh.service. The MARQUEET_* variables are
# for testing.
set -eu

boot=${MARQUEET_BOOT:-/boot/firmware}
sshd_conf=${MARQUEET_SSHD_DIR:-/etc/ssh/sshd_config.d}
systemctl=${MARQUEET_SYSTEMCTL:-systemctl}
keygen=${MARQUEET_KEYGEN:-ssh-keygen -A}
chown=${MARQUEET_CHOWN:-chown}
max_bytes=16384

say() {
  echo "marqueet-devssh: $*"
}

first() { # the first of the given files that exists
  for f in "$@"; do
    if [ -e "$f" ]; then
      echo "$f"
      return
    fi
  done
}

off=$(first "$boot/marqueet-ssh-off" "$boot/marqueet-ssh-off.txt")
if [ -n "$off" ]; then
  $systemctl disable --now ssh.service ssh.socket || true
  $systemctl mask ssh.service ssh.socket
  rm -f "$sshd_conf/10-marqueet-keys-only.conf" "$off"
  say "SSH is off."
  exit 0
fi

# Windows may save the file with .txt on the end.
key=$(first "$boot/marqueet-ssh-key.pub" "$boot/marqueet-ssh-key.pub.txt")
[ -n "$key" ] || exit 0

user=${MARQUEET_DEV_USER:-$(getent passwd 1000 | cut -d: -f1)}
if [ -z "$user" ]; then
  say "no account yet (set one in Raspberry Pi Imager); trying again at the next start"
  exit 0
fi
home=${MARQUEET_DEV_HOME:-$(getent passwd "$user" | cut -d: -f6)}
group=${MARQUEET_DEV_GROUP:-$(id -gn "$user")}

# Only plain public keys: a key type, the key, an optional comment. No
# options (command=..., from=...), nothing else.
if [ "$(wc -c <"$key")" -gt "$max_bytes" ]; then
  keys=""
else
  keys=$(tr -d '\r' <"$key" |
    grep -E '^(ssh-ed25519|ssh-rsa|ecdsa-sha2-nistp(256|384|521)|sk-ssh-ed25519@openssh\.com|sk-ecdsa-sha2-nistp256@openssh\.com) [A-Za-z0-9+/]+={0,3}( [[:print:]]*)?$' ||
    true)
fi
if [ -z "$keys" ]; then
  mv "$key" "$boot/marqueet-ssh-key.invalid"
  say "$(basename "$key") has no public key in it; SSH stays off (renamed it marqueet-ssh-key.invalid)"
  exit 0
fi

mkdir -p "$home/.ssh"
chmod 700 "$home/.ssh"
auth="$home/.ssh/authorized_keys"
touch "$auth"
echo "$keys" | while IFS= read -r line; do
  grep -qxF "$line" "$auth" || printf '%s\n' "$line" >>"$auth"
done
chmod 600 "$auth"
$chown "$user:$group" "$home/.ssh" "$auth"

mkdir -p "$sshd_conf"
printf '%s\n' \
  "# Written by Marqueet's developer switch: keys only." \
  "PasswordAuthentication no" \
  "KbdInteractiveAuthentication no" \
  "PermitRootLogin no" >"$sshd_conf/10-marqueet-keys-only.conf"

$keygen
$systemctl unmask ssh.service ssh.socket
# A plain service rather than Ubuntu's socket activation: one thing to start
# and stop.
$systemctl disable ssh.socket || true
$systemctl enable --now ssh.service

mv "$key" "$boot/marqueet-ssh-key.applied"
say "SSH is on for $user, keys only. To turn it off, put a file named marqueet-ssh-off on the card."
