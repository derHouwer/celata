# Changelog

All notable changes to Celata are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and Celata adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.9.0] - 2026-11-26

Updating from 0.8.1: install the new package or run the setup script of this version over it. Your settings, the
group `celata` and a running service stay. If you set Celata up by hand, run one command once; see
[Updating from 0.8.1](README.md#updating-from-081) in the README.

### Added

- The state of the connection in colour: the main button is blue when not connected, green when connected and red
  when the internet is blocked.
- An icon in the system tray that shows the state in the same colours, with the state as its tooltip. A left click
  brings the window back.
- A menu at the icon in the system tray (right click): the state, connect and disconnect, "Connect to best
  location", block and unblock the internet, your favorite locations, all locations, "About Celata" and "Quit".
- The locations in that menu are sorted by continent (Europe, Asia, North America, South America, Africa and
  Oceania), then by country and city; the streaming servers come last.
- Desktop notifications when the connection changes while the window is not in front.
- Favorite locations: a star at every location puts it at the top of the list and in "Favorite locations" on the
  overview.
- "Best location": Celata connects to the nearest location by the time zone of your computer, or by the country of
  the system language. Nothing leaves your computer to find it. While connecting, Celata names the target.
- Celata starts the hide.me service itself when it is not running, so you no longer start it by hand. The packages
  bring a rule for polkit that allows this for members of the group `celata`, only for this one service, only in
  a session at the computer. Without the rule, the button "Start service" asks for the administrator password.
- Settings in the new section "Application": "Show notifications", "Keep running after closing" (closing the
  window leaves Celata in the system tray), "Start at login" (never connects by itself) and "Check for new
  versions" (off by default; it asks GitHub once at start and once a day, never while the internet is blocked, and
  only shows a hint).
- A light and a dark appearance. The button at the right end of the top bar switches at once; Celata remembers
  the choice and starts light.
- Only one running window: starting Celata a second time brings the running window to the front.
- `celata --help` and `celata --unblock`; the second lifts a block of the internet from a terminal, without the
  window.
- The setup script `celata-setup.sh` on the releases page. It recognizes the system, finds the hide.me client,
  installs the Celata package of the same version after checking it against `SHA256SUMS`, adds a drop-in file
  for the service if the client lies somewhere else than the package expects, and adds you to the group
  `celata`. `--check` shows what is in place, `--dry-run` shows what would be done, `--uninstall` removes
  what the script set up. It never installs the hide.me client, never touches DNS, never starts the service
  and never connects.
- Linux Mint 22 as a tested system (it uses the package of Ubuntu 24.04).

### Changed

- The list of locations has one row per country. Countries with several locations are folded and open with an
  arrow.
- "Best location" is preselected on the overview instead of "Choose location".
- The card "Connection" on the overview is always shown, with a dash where a value is not known yet.
- The packages depend on `polkit` and on the time zone data (`tzdata`).

### Fixed

- The example for a client in `/opt/hide.me` in "Troubleshooting" gave the service write access to
  `/etc/resolv.conf` only. It now allows `/etc`, like the service file of the packages.
- The library `url` was updated to 2.5.9 because of the security notice RUSTSEC-2026-0592.

## [0.8.1] - 2026-11-16

First public version.

### Added

- Overview with the state of the VPN connection, the chosen location and one main button to connect and
  disconnect.
- Location list by country and city with flags, search and a "Free only" filter, plus a separate tab with
  20 streaming servers.
- Switching to another server while connected. The internet stays blocked in between instead of unprotected,
  and Celata returns to the previous server if the new one fails.
- Kill switch: Celata recognizes a blocked internet connection and offers "Block internet" and
  "Unblock internet" on the main button.
- Connection details: duration, data volume, addresses inside the tunnel and the public address.
- Settings of the hide.me service: split tunneling, DNS, connection and advanced values. While connected,
  changes are applied automatically.
- Hint in the overview when DNS requests can bypass the VPN because the system resolves names through
  systemd-resolved (see `docs/dns-leak.md`).
- English and German interface; the language can be changed in the settings without a restart.
- Desktop integration: application ID, desktop entry and icon. `celata --version` prints the version.
- Wayland sessions are supported. X11 is enabled but untested.
- Packages for Debian/Ubuntu and Fedora and a `PKGBUILD` for Arch Linux. They install the service unit
  (not started, not enabled) and create the group `celata`.
- Optional donation hint, which can be turned off in the settings.
