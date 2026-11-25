# Review guide

This page is for anyone who wants to review Celata. It says what Celata is meant to do, where the
sensitive places are, how to build and check it, and what the author's own review found. Findings are
welcome; please report them as described in [SECURITY.md](../SECURITY.md).

## What Celata is, and what it is not

Celata is a graphical interface for the hide.me VPN command-line client. It runs in the user's session
without administrator rights. The VPN itself is the hide.me client, which systemd runs as root in its
"service" mode; Celata sends it commands through a local socket.

Celata is **not** a VPN, contains no cryptography of its own, does not handle credentials, and does not
change the network configuration itself. What it must get right:

1. **Honest display.** What the window shows about connection and kill switch must match reality, also when
   something fails.
2. **Never weaken the protection silently.** In particular, never lift the internet block as a side effect.
3. **Never widen access to the service.** Only the calls Celata needs, only through the protected socket.
4. **Treat everything from outside as untrusted**: answers of the service, public lists, files.
5. **Do no harm when installed or removed**: the packages run a few commands as root.

The overall structure is described in [ARCHITECTURE.md](ARCHITECTURE.md), the security model for users in
[SECURITY.md](../SECURITY.md).

## Trust boundaries in one paragraph

The user is trusted. The hide.me service runs as root and is trusted to do its job, but its answers are
parsed defensively. Other programs under the user's account are *not* kept away from the service: every
member of the group `celata` can control it, which is a documented limitation and not something Celata can
change. The internet is untrusted; Celata fetches only one public list over HTTPS.

## Sensitive places

### The control connection

| Where | What to look at |
|---|---|
| `src/daemon/client.rs` | The complete list of calls Celata makes. There is no function for arbitrary paths. `GET /configuration` (returns credentials), `/token`, `/log` and `GET /serverList` (overwrites the configuration of the service) must never be called. Only the path socket `/run/hide.me/control.sock`; no TCP, no abstract socket. |
| `src/daemon/http.rs` | A hand-written minimal HTTP/1.1 client over a Unix socket: limits for the size of answers and headers and for time, behavior on truncated or malformed answers, chunked encoding. Answers are never logged, because they can contain connection keys. |
| `src/settings.rs`, `service_value` | What `POST /configuration` sends. It must never contain credentials or paths outside `/run/hide.me`; the field names must match the client (a renamed field is ignored silently). |
| `src/settings.rs`, `apply_edit`, `validate` | Validation of every setting the user can enter (networks, addresses, ports, file names, interface names). |

### The internet block (kill switch)

| Where | What to look at |
|---|---|
| `src/state.rs`, `Monitor` | The state machine. For every failure: does the block stay, or does the window say clearly that it is gone and name the way out? "Disconnect" and "Unblock internet" use `/destroy`; nothing else may. |
| `src/switch.rs` | Switching servers: disconnect (the block stays), configure, connect, and the way back to the previous server. `/destroy` must never follow on its own. |
| `src/killswitch.rs` | Recognizing the block from the routing table read with `ip`. Known limitation: if the table cannot be read, the internet counts as free. |
| `src/unit.rs` | Starting the service through systemd: only `StartUnit` with the job mode `fail`, only for `celata-hide.me.service`, never a stop, restart, enabling or disabling; the password prompt (`AllowInteractiveAuth`) only in `start_unit_interactive`, which only the button "Start service" calls; no program is started, no `sudo`; the group is read from `/proc/self/status` and `/etc/group` without `unsafe`. A start never repeats by itself. |
| `src/main.rs` | The wiring: threads, locks, and the order in which results reach the window. This file has no automated tests of its own and is the largest one. |

### What runs as root

Celata itself runs nothing as root. These files do, at installation time or as part of the service:

| Where | What to look at |
|---|---|
| `packaging/systemd/celata-hide.me.service`, `packaging/arch/celata-hide.me.service` | The service unit: the socket path, group and mode of the socket, the hardening options, the paths the service may write to. The hardening options follow the unit shipped with the hide.me client. The unit must never contain a `connect` command. |
| `packaging/debian/celata.prerm`, `packaging/debian/rules` | The Debian package: what happens on install, update and removal. |
| `packaging/rpm/celata.spec` | The Fedora package: the scriptlets `%post` and `%preun`. |
| `packaging/arch/PKGBUILD`, `packaging/arch/celata.install` | The Arch package. |
| `packaging/sysusers/celata.conf` | Creates the group. It must not add members. |
| `packaging/polkit/49-celata.rules` | The rule for polkit: only the action `org.freedesktop.systemd1.manage-units`, only the unit `celata-hide.me.service`, only the verb `start`, only an active session at the computer (`subject.local`, `subject.active`), only members of `celata`, and only the answer `YES`. Anything wider would let members of the group stop the service (and with it lift a block) or start other units without a password. |
| `packaging/build-packages.sh` | Builds the packages in containers. Base images and the Rust installer are fetched by name over HTTPS, not by checksum; the crates are pinned by `Cargo.lock`, and the builds themselves run without network access. |

### Input from outside

| Where | What to look at |
|---|---|
| `src/servers.rs` | The public server list: size limit, validation of host names (they end up in `POST /configuration`), cleaning of names before display, the cache file. |
| `src/net.rs` | The only HTTP client for the internet: HTTPS only, no redirect to plain HTTP, limits for time and size. Certificates are checked against the bundled list of certificate authorities, not the one of the system. |
| `src/streaming.rs` | Name lookups of a fixed list of server names at start. |
| `src/dnsleak.rs` | Looks at four local places and reads two system files with a size limit; must not start a program, send anything or write. |
| `src/files.rs`, `src/settings.rs` (`load`) | Reading and writing files in the user's directories: permissions, atomic writing, behavior with damaged or foreign files, symbolic links. |
| `src/desktop.rs` | Writes and removes the desktop entry and icon in the user's directory. It must never touch an entry it did not create. |
| `src/process.rs`, `src/donation.rs` | Starting other programs (`ip`, `systemctl`, `xdg-open`): fixed paths or fixed arguments, no shell. `xdg-open` receives one fixed link. |
| `src/text.rs` | Cleaning text from outside before it is shown. |

## Building and checking

You need Rust 1.92 or newer, the development files of fontconfig, `cargo-audit` and `cargo-deny`.

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo audit
cargo deny check
```

All five must pass without findings. The exceptions in `deny.toml` and `.cargo/audit.toml` concern
unmaintained crates that come in through Slint; each has its reason next to it.

No test needs the real service, a network connection or a display. To try Celata against the real service
you need a hide.me account; keep `sudo systemctl stop celata-hide.me.service` at hand, because the kill
switch can block your connection.

## What the author's own review found

The review of version 0.8.1 is described first, the review of the changes in version 0.9.0 after it.

All hand-written code of version 0.8.1 was reviewed file by file from three angles: clean code, outdated
elements, and security. The review was done by the author with automated tools and an AI assistant.

Areas: control connection; state and flows; settings and files; desktop integration, donation hint and
names; wiring; texts; interface; tests; build, checks and service unit; and, in a second pass, everything
that was added afterwards (DNS hint, packages, build script).

| Severity | Meaning | Findings |
|---|---|---|
| critical | damage to security or data without the user doing anything | 0 |
| high | damage under usual conditions, or the kill switch fails | 1 |
| medium | misbehavior in edge cases, or maintenance clearly harder | 1 |
| low | untidiness without consequence for users in normal operation | 28 |
| note | taste, idea, small improvement | 22 |

Of these 52 findings, 34 are fixed, 6 need no action, and 12 are deferred.

The medium finding is **not fixed**: the service may write to all of `/etc`, as in the unit shipped with the
hide.me client, and a member of the group can use the configurable backup path of `resolv.conf` to make it
overwrite a file there with the content of `resolv.conf`. Celata itself only accepts a backup path in
`/run/hide.me`, but the limit sits in the wrong program.

The high finding was introduced and removed during this review, and never released: an attempt to fix the
medium finding by granting write access to the single file `/etc/resolv.conf` instead of `/etc`. The grant
belongs to the file as it exists when the service starts. NetworkManager replaces the file whenever the
network changes, and from then on the client could not connect ("read-only file system"). A test on a real
system showed this; the unit is back to the write access of the client's own unit. A way to protect `/etc`
that survives the replacement of the file is an open question, and ideas are welcome.

Deferred, and worth a second opinion:

- Write access of the service to `/etc`, see above.
- If the routing table cannot be read, Celata does not recognize a blocked connection and shows no warning.
- When connecting from a blocked state while a server name cannot be resolved, the hide.me client connects
  to the previous server, and Celata shows the new name.
- Structure: `src/main.rs` and `src/settings.rs` are large, `run_reconnect` in `src/switch.rs` takes eight
  parameters, `StatusView` in `src/state.rs` has 19 fields, `state` and `look` depend on each other, and
  21 constants only give a text key a second name. None of this was restructured shortly before the
  release, because the flows are sensitive and the wiring has no automated tests.
- When connecting from a blocked state, the window thread waits for `systemctl`, usually a few
  milliseconds.
- While a dialog is open, the tab key also reaches buttons behind it.
- Three dialogs are built alike but written three times.

Not covered by the author's review, or only partly:

- Use with a real VPN connection on anything but one system (Fedora 44 family, KDE Plasma, Wayland).
- X11 beyond "the window appears".
- The hide.me client itself, and the generated Slint code.
- Whether the English comments, translated from German for this release, say everywhere what the code does.

### The review of version 0.9.0

The changes since 0.8.1 were reviewed for security, again with automated tools and an AI assistant, in seven
areas: the rule for polkit and the start of the service (`packaging/polkit/49-celata.rules`, `src/unit.rs`, the
packages); the setup script; the use of the session bus (`src/presence.rs`, `src/notify.rs`, `src/instance.rs`,
the menu in `ui/tray.slint`); the check for new versions (`src/update.rs`); the entry for the start at login and
the desktop entry (`src/autostart.rs`, `src/desktop.rs`); the command line (`src/cli.rs`, `src/unblock.rs`); and
the third-party libraries that came with it. Files that read data from the system or from the list of servers
(`src/best.rs`, `src/zone.rs`, `src/continent.rs`, `src/favorites.rs`) were read for limits and crashes.

Result: no critical, high or medium finding. One low finding was fixed: after an update of the package while Celata
was running, the path of the program carried the suffix " (deleted)" and could end up in the entry for the start at
login. Two notes need no action: a program of the same user on the session bus can claim the name of Celata, imitate
the system tray or trigger the entries of the menu, which a program of that user (in the group `celata`) can do
through the control socket anyway; and the rule for polkit applies only to calls that name the unit and the verb
`start`.

The exceptions for the tools (`cargo audit`, `cargo deny`) were checked again: all are notices about crates that
are no longer maintained, none reports a vulnerability. `zvariant`, `zbus_names` and `zvariant_derive` also come in
through `zbus` 5, which Celata uses for the session and the system bus; only `zbus` 6 replaces them.

## How to report findings

- Security problems: privately, through "Report a vulnerability" in the "Security" tab of the repository.
- Everything else: as an issue, one finding per issue if possible.

For each finding, the place (file and function), what can go wrong, and how serious you consider it are
most helpful. A failing test is the best report of all.
