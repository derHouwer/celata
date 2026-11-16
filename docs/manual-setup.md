# Manual setup

This page describes how to set up Celata **without a package**: building it from source and installing the
service unit and the group by hand.

If there is a package for your distribution, use it instead. It does all of this for you; see "Install" in
the [README](../README.md).

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

## 4. Start the service

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

## 5. Check the result

```bash
ls -l /run/hide.me/control.sock
```

The line must start with `srw-rw----` and name `root celata`.

## 6. Start Celata

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

## Removing the setup

```bash
sudo systemctl disable --now celata-hide.me.service
sudo rm /etc/systemd/system/celata-hide.me.service
sudo systemctl daemon-reload
sudo gpasswd -d "$USER" celata
sudo groupdel celata
sudo rm /usr/local/bin/celata
rm -f ~/.local/share/applications/celata.desktop ~/.local/share/icons/hicolor/scalable/apps/celata.svg
```

Your settings stay in `~/.config/celata/` and the cached server list in `~/.cache/celata/`. Delete these
directories if you no longer need them.
