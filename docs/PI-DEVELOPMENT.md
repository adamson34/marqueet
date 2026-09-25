# Developing on a Raspberry Pi

The Marqueet Pi image is an appliance: SSH is off, and updates arrive on
their own from the Snap Store's `stable` channel (releases from `main`). For
development you usually want the `edge` channel instead (rebuilt on every
merge to `dev`), and to reach the Pi from your computer instead of
reflashing. The image has a developer switch for that.

## Turn SSH on

1. Flash `marqueet-pi.img.xz` with Raspberry Pi Imager, setting up WiFi (if
   needed) and **an account (username and password)** in its settings.
2. With the card still in your computer, copy your **public** key onto the
   boot partition (it shows up as `system-boot`) as `marqueet-ssh-key.pub`:

   ```sh
   cp ~/.ssh/id_ed25519.pub /Volumes/system-boot/marqueet-ssh-key.pub   # macOS
   ```

   On Windows, a file saved as `marqueet-ssh-key.pub.txt` works too.
3. Put the card in the Pi and start it. At boot, `marqueet-devssh.service`
   adds the key for the Imager account, allows keys only (no passwords, no
   root), and starts SSH. The file is renamed `marqueet-ssh-key.applied` so
   you can see it worked.
4. Connect: `ssh <username>@marqueet.local` (or the Pi's address).

Only plain public keys are accepted (a key type, the key, an optional
comment). A file with nothing usable is renamed `marqueet-ssh-key.invalid`
and SSH stays off. Adding the same key twice doesn't duplicate it; adding
another file later adds more keys.

## Turn SSH off

Put an empty file named `marqueet-ssh-off` on the boot partition and start
the Pi: SSH is stopped and masked again and the keys-only config is removed.
(Keys stay in `~/.ssh/authorized_keys`; remove them yourself if you like.)

## Useful once you're in

```sh
sudo snap refresh marqueet --channel=edge      # follow edge (updates keep to it)
sudo snap refresh marqueet                     # install the latest build now
snap logs -f marqueet.server                   # server log
snap logs -f marqueet.display                  # display log
sudo snap install --dangerous ./marqueet_arm64.snap   # try a local build
```

## Why this is safe enough

Anyone who can write to the SD card can already change anything on the Pi,
so a key on the card grants nothing new. The switch never turns on password
logins, never allows root, and does nothing unless the file is there.
