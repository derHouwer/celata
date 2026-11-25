# Manual setup

This page describes how to set up Celata **without a package**: building it from source and installing by hand
the service unit, the group and the rule that lets Celata start the service.

If there is a package for your distribution, use it instead. It does all of this for you; see "Install" in
the [README](../README.md). The setup script `celata-setup.sh` installs the package and does the steps around
it.

The setup by hand stays for systems without a package and for developers. The setup script recognizes a
setup made by hand and does not change it; with `bash celata-setup.sh --check` you can see what it finds.

The setup is needed once and requires administrator rights (`sudo`). None of the commands changes your VPN
connection or your network: the hide.me service only connects when Celata tells it to.

If your user may not use `sudo` (a default Debian installation, for example), run the commands that start
with `sudo` in a root shell instead (`su -`), without the word `sudo`, and replace `"$USER"` with your user
name.

## Before you start

- The hide.me command-line client is installed and has an Access-Token, as described in its
  [README](https://github.com/eventure/hide.client.linux). Celata does not include the client.
- No other hide.me unit is running. Both would use the same network interface:

  ```bash
  systemctl list-units 'hide.me@*'
  ```

  The list must be empty.

## 1. Build Celata

You need Rust 1.92 or newer and the development files of fontconfig (`libfontconfig-dev` on Debian and
Ubuntu, `fontconfig-devel` on Fedora, `fontconfig` on Arch Linux).

To run, Celata needs the libraries of your graphical session (Wayland, xkbcommon and EGL; any desktop has
them), the program `ip` (package `iproute2`) and at least one installed font. Without any font Celata ends
at start.

Run all commands in the directory of the source code:

```bash
cargo build --release --locked
sudo install -m 0755 target/release/celata /usr/local/bin/celata
celata --version
```

## 2. Create the group and add yourself

Only members of this group may control the service.

```bash
sudo groupadd --system celata
sudo usermod -aG celata "$USER"
```

**Log out and back in once.** Only then does the new group apply. Check it with:

```bash
id -nG | grep -w celata
```

## 3. Install the service unit

If the hide.me client is installed in `/opt/hide.me` (the official way):

```bash
sudo install -m 0644 packaging/systemd/celata-hide.me.service /etc/systemd/system/
sudo systemctl daemon-reload
```

If it is installed from the AUR package `hide-client` on Arch Linux (`/usr/bin/hide.me`, `/etc/hide.me`):

```bash
sudo install -m 0644 packaging/arch/celata-hide.me.service /etc/systemd/system/
sudo systemctl daemon-reload
```

## 4. Let Celata start the service

(This is also the one step to do when you update a setup by hand from version 0.8.1: version 0.8.1 did not have the
rule.)

A rule for polkit lets members of the group `celata` start this one service without a password, in a session
directly at this computer. Nothing else: stopping, restarting, enabling or disabling it still needs `sudo`.
With the rule Celata starts the service by itself when it is not running, at every start of Celata and when you
connect after the service was stopped. Celata never connects by itself.

```bash
sudo install -m 0644 packaging/polkit/49-celata.rules /etc/polkit-1/rules.d/49-celata.rules
```

Or, from any directory, with the rule built into Celata:

```bash
celata --polkit-rule | sudo tee /etc/polkit-1/rules.d/49-celata.rules >/dev/null
```

Check that it works. The service must be stopped first; the second command then starts it without a password:

```bash
sudo systemctl stop celata-hide.me.service
systemctl --no-ask-password start celata-hide.me.service
```

No output means the rule works. "Interactive authentication required." means it does not: check the group
(step 2, log out and back in) and that you are logged in at this computer, not over the network.

To remove the rule again:

```bash
sudo rm /etc/polkit-1/rules.d/49-celata.rules
```

Without the rule Celata shows the button "Start service", which asks for the administrator password, and the
command for the rule.

## 5. Start the service by hand (only without the rule)

With the rule from step 4 you can skip this step. Without it, start the service yourself:

```bash
sudo systemctl start celata-hide.me.service
```

Started this way, the service runs until the next shutdown. If you want it to start with the computer
instead, enable it once:

```bash
sudo systemctl enable --now celata-hide.me.service
```

Either way the service only waits for Celata. It never connects on its own. To undo the second command:
`sudo systemctl disable --now celata-hide.me.service`.

## 6. Check the result

```bash
ls -l /run/hide.me/control.sock
```

The line must start with `srw-rw----` and name `root celata`.

## 7. Start Celata

```bash
celata
```

On its first start Celata creates its own desktop entry and icon in your user directory
(`~/.local/share/applications/celata.desktop`, `~/.local/share/icons/hicolor/scalable/apps/celata.svg`), so
that it appears in the application menu. Some desktops show it only after you log in again.

## Stopping the service

This also ends a running connection and lifts an internet block:

```bash
sudo systemctl stop celata-hide.me.service
```

With the rule from step 4, Celata starts the service again when you connect the next time, or when Celata
starts. To keep the service off, quit Celata first.

## Removing the setup

```bash
sudo systemctl disable --now celata-hide.me.service
sudo rm -f /etc/polkit-1/rules.d/49-celata.rules
sudo rm /etc/systemd/system/celata-hide.me.service
sudo systemctl daemon-reload
sudo gpasswd -d "$USER" celata
sudo groupdel celata
sudo rm /usr/local/bin/celata
rm -f ~/.local/share/applications/celata.desktop ~/.local/share/icons/hicolor/scalable/apps/celata.svg
```

Your settings stay in `~/.config/celata/` and the cached server list in `~/.cache/celata/`. Delete these
directories if you no longer need them.
