# Security policy

## Supported versions

Only the newest release of Celata receives fixes. There are no automatic updates: watch the releases of
this repository to learn about new versions ("Watch" → "Custom" → "Releases").

## Reporting a vulnerability

Please report security problems **privately**, not in a public issue:

1. Open the "Security" tab of this repository.
2. Choose "Report a vulnerability".
3. Describe what you found, how to reproduce it, and which version and distribution you use.

Do not include your Access-Token, passwords, or logs that show your addresses.

You will get an answer through the same form. Celata is maintained by one person in their spare time, so
please allow a few days.

## Scope

In scope:

- Celata itself (the program in this repository),
- the service unit `celata-hide.me.service`,
- the packages, the setup script and the build script in `packaging/`.

Out of scope:

- the hide.me command-line client and the hide.me servers. Celata is an independent project and is not
  affiliated with hide.me. Please report problems in the client to
  [its own project](https://github.com/eventure/hide.client.linux).

## Security model

- **Celata runs without administrator rights.** It never sees a password and never runs anything as root. Only
  when you click "Start service" (shown when the rule for polkit below is missing) does the system itself ask for
  the administrator password, through the password agent of your desktop.
- **The setup script `celata-setup.sh`** runs as your user and uses `sudo` for single commands only. It checks
  the package it installs, and itself, against `SHA256SUMS`; the checksums are not a signature, so they do not
  prove who built the files. It never reads your Access-Token, never changes DNS settings, and refuses to use a
  hide.me client at a place that others can change, because the service runs it as root.
- **The VPN is the hide.me client**, started by systemd as root in its "service" mode
  (`celata-hide.me.service`). In this mode it waits for commands and never connects on its own.
- **Celata talks to the service only through a local socket**, `/run/hide.me/control.sock` (owner `root`,
  group `celata`, mode `0660`). There is no TCP port.
- **Celata needs no credentials.** It never reads your Access-Token and never sends credentials to the
  service.
- **Network access of Celata itself** is limited to public lists of hide.me over HTTPS (the server list)
  and to name lookups of server names. Celata contains no telemetry and never contacts the donation page; the
  donation button only hands a fixed link to your browser.
- **Favorite locations** are stored as host names in the local settings file (mode `0600`) and are never
  sent to the service or the network.
- **The icon in the system tray** shows the state and its hover text names it, for example "Celata –
  Connected" or, while switching, the target location. Every program of your own account can read this over
  the session bus, also without the group `celata`. It never contains host names or addresses. A click on
  the icon only brings the window back.

### The group `celata`

The hide.me service does not check who talks to it. It is protected only by the permissions of its control
socket. **Every program that runs under the account of a member of the group `celata` can control the
service just as Celata does**, without a password: connect and disconnect, switch the server, lift the
internet block, change the settings of the service (for example turn the kill switch off or route networks
around the VPN), and read the configuration of the service, which includes the Access-Token of your hide.me
account. Such a program can also make the service, which runs as root, overwrite a file in `/etc` with the
content of `resolv.conf`, by choosing where the service keeps its backup of that file. Celata itself only
accepts a location in `/run/hide.me`.

Celata starts the service itself when it is not running, so the service now usually runs as long as Celata
runs. A rule for polkit (`49-celata.rules`, installed with the packages) lets members of the group start this one
service without a password, in an active session directly at this computer. **Every program of a member of the
group can therefore start the service without a password, and then control it as described above.** The rule
allows only starting: stopping, restarting, enabling or disabling the service, and every other unit, keep the
usual rules of the system and need the administrator password.

This is the price for running Celata without administrator rights. To keep the risk small:

- Add only your own account to the group, never accounts of other people or of services.
- If the service should run only now and then, turn off "Start at login" in Celata, quit Celata and stop the
  service with `sudo systemctl stop celata-hide.me.service`. If you enable the service to start with the
  computer, the socket is reachable for the group all the time.
- If no program should start the service without a password, remove the rule (command in
  [docs/manual-setup.md](docs/manual-setup.md), step 4; with a package, put an empty file with the same name in
  `/etc/polkit-1/rules.d/`). Celata then offers only the button "Start service", which asks for the
  administrator password.
- Leave the group when you stop using Celata: `sudo gpasswd -d "$USER" celata`.

The service can write only to the directory of the hide.me client, to `/etc`, to `/run/hide.me` and to the
runtime directory of systemd-resolved; the rest of the system is read-only for it. Write access to `/etc` is
the same as in the unit shipped with the hide.me client. It cannot be narrowed to `/etc/resolv.conf`: the
system replaces that file whenever the network changes, and the client could then no longer connect.

## State of review

Version 0.8.1 has been through a structured self-review of all hand-written code (clean code, outdated
elements, security) with automated tools and an AI assistant; every finding was fixed or is listed as
deferred. The changes since then (version 0.9.0) have been through a security review of their own, both with
automated tools and an AI assistant: the rule for polkit and the start of the service, the setup script, the use
of the session bus (menu, notifications, a second start), the check for new versions, the entry for the start
at login, the command line and the new third-party libraries. Details and the sensitive places of the code are
described in [docs/REVIEW-GUIDE.md](docs/REVIEW-GUIDE.md).

Every change is checked with `cargo test`, `cargo clippy`, `cargo fmt --check`, `cargo audit` and
`cargo deny`. Hand-written code forbids `unsafe`.
