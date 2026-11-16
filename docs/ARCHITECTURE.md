# Architecture

This page describes how Celata is built: which parts exist, how data flows between them, and where the
trust boundaries are. It is meant for reviewers and anyone who wants to read the code.

## The big picture

```text
 user session (no administrator rights)            │  system (root)
                                                   │
 ┌──────────────┐  values, callbacks  ┌──────────┐ │ ┌───────────────────────────┐
 │  interface   │◄───────────────────►│  wiring  │ │ │ celata-hide.me.service    │
 │  ui/*.slint  │                     │ main.rs  │ │ │ hide.me client, "service" │
 └──────────────┘                     └────┬─────┘ │ │ mode, started by systemd  │
                                           │       │ └────────────▲──────────────┘
                              ┌────────────▼─────┐ │              │
                              │  program logic   │ │   HTTP over a Unix socket
                              │  src/*.rs (lib)  ├─┼──────────────┘
                              └───┬──────┬───────┘ │   /run/hide.me/control.sock
                                  │      │         │   root:celata 0660
              files in the user's │      │ HTTPS, name lookups
              directories         │      ▼         │
                                  ▼   public lists of hide.me, DNS
```

Celata is only the graphical interface. The VPN itself is the hide.me command-line client, which systemd
runs as root in its "service" mode. Celata talks to it through a local control socket and to nothing else
on the system that needs privileges.

## Crates

| Crate | Content |
|---|---|
| `celata` (root) | `src/lib.rs` with the program logic, `src/main.rs` with the wiring |
| `celata-ui` (`crates/celata-ui`) | only the code Slint generates from `ui/*.slint` |

Hand-written code is built with `#![forbid(unsafe_code)]`. The generated code needs `unsafe` internally,
so `celata-ui` uses `#![deny(unsafe_code)]` and contains nothing else. Keeping it in its own crate also
keeps the stricter lints of the root crate away from code Celata cannot change.

## Modules of the program logic

All of these are plain Rust without any dependency on the interface, so they can be tested without a
window.

| Module | Purpose |
|---|---|
| `daemon` | The control connection. `http.rs` is a minimal HTTP/1.1 client over the Unix socket with limits for size and time. `client.rs` offers exactly the calls Celata is allowed to make; there is deliberately no function for arbitrary paths. |
| `state` | The states Celata can show (`DisplayState`) and how they follow from what the service reports and from the routes of the system (`Monitor`). Also the commands shown for the terminal. |
| `switch` | Switching to another server while connected, and reconnecting after a change of settings: disconnect, configure, connect, and the way back to the previous server if the new one fails. |
| `killswitch` | Recognizes a blocked internet connection from the routing table of the service, read with `ip`. |
| `settings` | The settings of the hide.me service that Celata manages: validation, the settings file, what is sent with `POST /configuration`, warnings and hints. The service forgets its configuration on every restart, so Celata stores the settings itself and sends them before every connection. |
| `servers` | The public server list of hide.me: fetching, validating, caching. |
| `streaming` | A fixed table of streaming servers and the check at start that each name still exists. |
| `categories` | The public list of filter categories (unused while the filters are hidden). |
| `dnsleak` | Recognizes a system where DNS requests can bypass the VPN, from a few local files and directories. |
| `desktop` | Desktop entry and icon in the user's directory, unless a package already provides them. |
| `donation` | The donation link and handing it to the browser with `xdg-open`. |
| `look` | Derives what the interface shows from state and lists: colours of the status dot, the main button, rows of the location list. The interface itself decides nothing that depends on state. |
| `i18n` | Languages and texts. Every visible text from Rust is a `Key` with one text per language; a missing text is a compile error. Stored messages are `Phrase`s and are translated only when shown. |
| `names`, `flags` | Country codes and the embedded flag images. |
| `net` | The one way Celata fetches public data: HTTPS only, limited in time and size. |
| `process` | The one way Celata runs system tools (`ip` to read routes, `systemctl show` to read the state of the unit): fixed path, no shell, empty environment, limited output. |
| `files` | Files in the user's directories: locations, atomic writing with exact permissions, reading with a size limit. |
| `text` | Cleaning text that comes from outside before it is shown. |
| `cli` | `--version` and the message for a start without a graphical session. |

## Wiring (`src/main.rs`)

`main.rs` connects the interface with the logic and contains no decisions of its own where it can be
avoided:

- Every call to the service, to the public lists and to the name resolution runs in its own thread, so that
  the window never blocks. Results return to the interface with `slint::invoke_from_event_loop`.
- One thread observes the service permanently (`/watch`) and feeds `Monitor`; the interface is refreshed
  from the result. Celata checks again by itself every second while the service is unreachable.
- Shared state (`Monitor`, server list, settings) lives behind mutexes. A poisoned mutex is used anyway, so
  that a crashed thread cannot freeze the display.
- Timers in the window thread apply pending changes of settings, expire messages, and look at the DNS
  situation every ten seconds.

## Interface (`ui/`)

| File | Content |
|---|---|
| `main.slint` | The window: top bar, sidebar, pages, dialogs |
| `overview.slint`, `locations.slint`, `settings.slint` | The three pages |
| `widgets.slint` | Shared components (buttons, cards, switches, input fields, messages) |
| `theme.slint` | The only place for colours and font sizes |
| `translations/<code>/LC_MESSAGES/celata-ui.po` | Translations of the texts in `@tr("…")`; bundled into the program at build time |

## Data flow of a connection

1. The user chooses a location and presses the main button.
2. `main.rs` asks `Monitor` whether connecting is allowed in the current state and starts a thread.
3. The thread sends the settings and the chosen host with `POST /configuration`, then `GET /connect`.
4. The service reports progress and the result through `/watch`; `Monitor` turns each report into a display
   state; `look` derives colours and labels; the interface shows them.
5. After every report Celata reads the routing table to see whether the internet is blocked.

Celata never asks the service for its configuration (`GET /configuration` would return credentials) and
never uses `GET /serverList` (it overwrites the configuration of the service).

## Trust boundaries

| Boundary | What crosses it | How Celata treats it |
|---|---|---|
| User → Celata | Clicks, text in input fields | Every input is validated before it is stored or sent (`settings::apply_edit`); host names must come from the server list or the streaming table. |
| Celata → service | Commands and settings, never credentials | Only the calls in `daemon/client.rs`, only over the path socket. |
| Service → Celata | State reports, which can contain connection keys | Parsed into fixed structures; text is cleaned before display; answers are limited in size and time and never logged. |
| Group `celata` → service | Anything the service offers | Outside Celata's control: the service does not check its callers. See [SECURITY.md](../SECURITY.md). |
| Internet → Celata | The public server list | HTTPS only, certificate checked, limited in size and time, every entry validated, text cleaned. |
| Celata → internet | The request for the list; name lookups of server names | Nothing else. No telemetry. |
| Celata → system tools | `ip` (read routes), `systemctl show` (read the state of the unit), `xdg-open` (open the donation page) | Fixed arguments, no shell. Nothing is run with privileges. |
| Files in the user's directories | Settings, cached lists, desktop entry | Written atomically with exact permissions (settings `0600`); read with a size limit; damaged files fall back to defaults. |
| Local system files | `/run/systemd/resolve`, `/etc/nsswitch.conf`, `/etc/resolv.conf` | Read only, with a size limit, for the DNS hint. |

## Where things are stored

| Path | Content |
|---|---|
| `~/.config/celata/einstellungen.json` | Settings (mode `0600`). Never contains credentials. |
| `~/.cache/celata/` | Cached public lists and the record of the settings last applied to the service |
| `~/.local/share/applications/celata.desktop`, `~/.local/share/icons/hicolor/scalable/apps/celata.svg` | Only when Celata was not installed from a package |

`XDG_CONFIG_HOME`, `XDG_CACHE_HOME` and `XDG_DATA_HOME` are respected.

## Tests

- Unit tests live next to the code in each module.
- `tests/daemon_ablauf.rs` and `tests/einstellungen.rs` run the logic against a fake service on a test
  socket (`tests/support`).
- `tests/sprachen.rs` checks the translations, `tests/kontrast.rs` the colour contrasts,
  `tests/spendenhinweis.rs` that only `src/donation.rs` knows the donation link.
- `tests/release.rs` keeps version, license, service unit, desktop entry, packages and documentation
  consistent with each other, and checks that every source file starts with the license header and that
  comments are English.

No test needs the real service, a network connection or a display.
