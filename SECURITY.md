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
- the packages and the build script in `packaging/`.

Out of scope:

- the hide.me command-line client and the hide.me servers. Celata is an independent project and is not
  affiliated with hide.me. Please report problems in the client to
  [its own project](https://github.com/eventure/hide.client.linux).

## Security model

- **Celata runs without administrator rights.** It never asks for a password and never runs anything as
  root.
- **The VPN is the hide.me client**, started by systemd as root in its "service" mode
  (`celata-hide.me.service`). In this mode it waits for commands and never connects on its own.
- **Celata talks to the service only through a local socket**, `/run/hide.me/control.sock` (owner `root`,
  group `celata`, mode `0660`). There is no TCP port.
- **Celata needs no credentials.** It never reads your Access-Token and never sends credentials to the
  service.
- **Network access of Celata itself** is limited to public lists of hide.me over HTTPS (the server list)
  and to name lookups of server names. Celata contains no telemetry and never contacts the donation page; the
  donation button only hands a fixed link to your browser.

### The group `celata`

The hide.me service does not check who talks to it. It is protected only by the permissions of its control
socket. **Every program that runs under the account of a member of the group `celata` can control the
service just as Celata does**, without a password: connect and disconnect, switch the server, lift the
internet block, change the settings of the service (for example turn the kill switch off or route networks
around the VPN), and read the configuration of the service, which includes the Access-Token of your hide.me
account. Such a program can also make the service, which runs as root, overwrite a file in `/etc` with the
content of `resolv.conf`, by choosing where the service keeps its backup of that file. Celata itself only
accepts a location in `/run/hide.me`.

This is the price for running Celata without administrator rights. To keep the risk small:

- Add only your own account to the group, never accounts of other people or of services.
- Start the service only when you need it. If you enable it to start with the computer, the socket is
  reachable for the group all the time.
- Leave the group when you stop using Celata: `sudo gpasswd -d "$USER" celata`.

The service can write only to the directory of the hide.me client, to `/etc`, to `/run/hide.me` and to the
runtime directory of systemd-resolved; the rest of the system is read-only for it. Write access to `/etc` is
the same as in the unit shipped with the hide.me client. It cannot be narrowed to `/etc/resolv.conf`: the
system replaces that file whenever the network changes, and the client could then no longer connect.

## State of review

Version 0.8.1 has been through a structured self-review of all hand-written code (clean code, outdated
elements, security) with automated tools and an AI assistant; every finding was fixed or is listed as
deferred. Details and the sensitive places of the code are described in
[docs/REVIEW-GUIDE.md](docs/REVIEW-GUIDE.md).

Every change is checked with `cargo test`, `cargo clippy`, `cargo fmt --check`, `cargo audit` and
`cargo deny`. Hand-written code forbids `unsafe`.
