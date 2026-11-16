# Changelog

All notable changes to Celata are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and Celata adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
