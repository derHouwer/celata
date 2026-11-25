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
| `state` | The states Celata can show (`DisplayState`) and how they follow from what the service reports and from the routes of the system (`Monitor`), including the start of the service: "Starting service …", the reason when a start fails, and the target of a wish to connect that waits for the first answer. Also the commands shown for the terminal. |
| `unit` | Starting the service through systemd when Celata needs it: the decision before a start (state of the unit, group of the process), the meaning of the answers, the progress of a start, the command for the rule for polkit. The only module that talks to the system bus; it only ever calls `StartUnit` for `celata-hide.me.service`, never stops, restarts, enables or disables it, and asks for a password only after the button "Start service". |
| `switch` | Switching to another server while connected, and reconnecting after a change of settings: disconnect, configure, connect, and the way back to the previous server if the new one fails. |
| `killswitch` | Recognizes a blocked internet connection from the routing table of the service, read with `ip`. |
| `settings` | The settings of the hide.me service that Celata manages: validation, the settings file, what is sent with `POST /configuration`, warnings and hints. The service forgets its configuration on every restart, so Celata stores the settings itself and sends them before every connection. |
| `servers` | The public server list of hide.me: fetching, validating, caching. |
| `streaming` | A fixed table of streaming servers and the check at start that each name still exists. |
| `best`, `zone` | "Best location": the nearest single location of the loaded list (`best`), measured from the place the time zone database of the system assigns to the configured time zone, or from the country of the system language (`zone`). Read only when connecting through "Best location"; no request, nothing stored. |
| `continent` | The continent of every country, for the location menu of the tray icon: a fixed table of the known countries first, then the value in the server list, then the nearest country, and Europe if nothing else is known. Pure logic: no network, no files, nothing stored. `cargo test --test erdteile` shows where the table and the list differ. |
| `favorites` | Favorite locations: which host names are favorites (newest first), switching a star, and whether a favorite can be chosen right now. |
| `trayicon` | The logo of the tray icon, drawn from `ui/icons/celata.svg` in the colour of the state (blue, green, red; a closed ring while connected), and the short text of its tooltip. |
| `categories` | The public list of filter categories (unused while the filters are hidden). |
| `dnsleak` | Recognizes a system where DNS requests can bypass the VPN, from a few local files and directories. |
| `desktop` | Desktop entry and icon in the user's directory, unless a package already provides them, and the path of the running program for such entries (after an update of the package, the path without the suffix " (deleted)" that the kernel adds). |
| `donation` | The donation link and handing it to the browser with `xdg-open`. |
| `look` | Derives what the interface shows from state and lists: colours of the status dot, the main button, rows of the location list, the menu of the tray icon (`tray_menu`) and what a choice in that menu does in the current state (`menu_decision`). The interface itself decides nothing that depends on state. |
| `presence` | Whether the system tray is there and lists the icon of this process, asked on the session bus every two seconds. The icon is created only once the tray exists. |
| `background` | What closing the window does with "Keep running after closing", how a start at login begins, and when a hidden window has to come back because the icon is gone. |
| `notify` | Which change of state is worth a desktop notification and its text (country and place, never a host name), and the thread that sends it on the session bus. |
| `autostart` | The own entry in `~/.config/autostart` for "Start at login"; the file itself is the state of the switch, a foreign entry is never touched. |
| `instance` | Only one running window: a name on the session bus with the single method `Activate()`; a second start calls it and ends. |
| `unblock` | `celata --unblock`: lifts a block like "Unblock internet" in the window, without the window. |
| `update` | The optional check for a new version (off by default): parsing the answer of GitHub, the daily schedule, never past a block. |
| `about` | The addresses of the project page and of the setup guide, and the version line for "About Celata". |
| `i18n` | Languages and texts. Every visible text from Rust is a `Key` with one text per language; a missing text is a compile error. Stored messages are `Phrase`s and are translated only when shown. |
| `names`, `flags` | Country codes and the embedded flag images. |
| `net` | The one way Celata fetches public data: HTTPS only, limited in time and size. |
| `process` | The one way Celata runs system tools (`ip` to read routes, `systemctl show` to read the state of the unit): fixed path, no shell, empty environment, limited output. |
| `files` | Files in the user's directories: locations, atomic writing with exact permissions, reading with a size limit. |
| `text` | Cleaning text that comes from outside before it is shown. |
| `cli` | The options `--version`, `--help`, `--unblock`, `--polkit-rule` (prints the built-in rule for polkit) and `--autostart` (for the entry of the start at login), and the message for a start without a graphical session. |

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
- The event loop runs until Celata quits (`run_event_loop_until_quit`), so that the window can be hidden while
  Celata keeps running with its icon. Every way of quitting goes through one function that hides window and icon
  and stops the loop.
- The menu of the tray icon is built from the same parts as the window and handed over only when it changed.

## Interface (`ui/`)

| File | Content |
|---|---|
| `main.slint` | The window: top bar, sidebar, pages, dialogs (also "About Celata") |
| `tray.slint` | The icon in the system tray and its menu; every text comes from Rust |
| `overview.slint`, `locations.slint`, `settings.slint` | The three pages |
| `widgets.slint` | Shared components (buttons, cards, switches, input fields, messages) |
| `theme.slint` | The only place for colours and font sizes; every colour has a value for each appearance |
| `translations/<code>/LC_MESSAGES/celata-ui.po` | Translations of the texts in `@tr("…")`; bundled into the program at build time |

### Appearance (light and dark)

Celata has two appearances. Every colour in `theme.slint` is one line `pick(#light, #dark)`; the light values are
the ones the program always had. The appearance itself lives in one place only: Slint's `Palette.color-scheme`,
which also drives the built-in controls (drop-down list, text field, scroll bars; the style stays
`fluent-light`, so the start is always light). `Theme.is-dark` is derived from it and never set. Only
`MainWindow.set-appearance` assigns it: Rust calls it with the stored value before the window is shown (also for
a start without a window), and the button in the top bar calls it and reports the new value for saving
(`app.appearance` in the settings file, `"light"` or `"dark"`; never sent to the service). A colour that is
needed both as an area under white text and as text on a card has two names (`accent` and `accent-fg`, likewise
`tone-alert`, `connected`, `topbar`); in the light appearance both have the same value. The icon in the system
tray, its menu, the notifications and the title bar of the window do not follow the appearance.
`tests/kontrast.rs` reads `theme.slint` and checks the contrasts for both appearances, keeps the light values as
they were and allows no colour outside the file; `tests/aussehen.rs` checks the wiring in the source.

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
| Celata → systemd | Start of `celata-hide.me.service` (`StartUnit` with the job mode `fail`) and reading its state, over the system bus | Never a stop, restart, enabling or disabling. Whether the start is allowed decides polkit after `49-celata.rules` (only starting this unit, only members of `celata` in an active session at the computer). Without the rule only the button "Start service" lets the system ask for the password; Celata never sees it. One connection per operation, a 25 second limit (300 seconds with the password prompt), no signals. |
| Internet → Celata | The public server list | HTTPS only, certificate checked, limited in size and time, every entry validated, text cleaned. |
| Celata → internet | The request for the list; name lookups of server names; with "Check for new versions" switched on, one request to the GitHub API at start and once a day | Nothing else. No telemetry. The version check sends nothing but the request, uses only three numbers of the answer, and never runs while the internet is blocked. |
| Celata ↔ session bus | Presence of the tray icon, desktop notifications, the name `io.github.derhouwer.Celata` with `Activate()`, the menu of the tray icon (offered by Slint) | Only the session bus of the user, never the system bus; every call with a two second limit, from threads of its own. `Activate()` only brings the window back. Menu and notifications never contain host names or addresses, but every program of the session can read them. |
| Celata → system tools | `ip` (read routes), `systemctl show` (read the state of the unit), `xdg-open` (open the donation page, the project page or the releases page) | Fixed arguments, no shell. Nothing is run with privileges. |
| Files in the user's directories | Settings, cached lists, desktop entry, autostart entry | Written atomically with exact permissions (settings `0600`); read with a size limit; damaged files fall back to defaults. |
| Local system files | `/run/systemd/resolve`, `/etc/nsswitch.conf`, `/etc/resolv.conf` | Read only, with a size limit, for the DNS hint. |
| Local system files | `/proc/self/status`, `/etc/group` | Read only, before a start of the service, to know whether the process is in the group `celata`. |
| Local system files | `/etc/localtime` (link), `/etc/timezone`, `/usr/share/zoneinfo/{zone.tab, zone1970.tab, tzdata.zi}`, the variables `TZ`, `LC_ALL`, `LC_MESSAGES`, `LANG` | Read only, with a size limit, when connecting through "Best location"; the result is not kept. |

## Where things are stored

| Path | Content |
|---|---|
| `~/.config/celata/einstellungen.json` | Settings (mode `0600`). Never contains credentials. |
| `~/.cache/celata/` | Cached public lists and the record of the settings last applied to the service |
| `~/.local/share/applications/celata.desktop`, `~/.local/share/icons/hicolor/scalable/apps/celata.svg` | Only when Celata was not installed from a package |
| `~/.config/autostart/celata.desktop` | Only while "Start at login" is on (mode `0644`) |

`XDG_CONFIG_HOME`, `XDG_CACHE_HOME` and `XDG_DATA_HOME` are respected.

## Tests

- Unit tests live next to the code in each module.
- `tests/daemon_ablauf.rs` and `tests/einstellungen.rs` run the logic against a fake service on a test
  socket (`tests/support`).
- `tests/sprachen.rs` checks the translations, `tests/kontrast.rs` the colour contrasts of both appearances, `tests/aussehen.rs` the wiring of the appearance,
  `tests/spendenhinweis.rs` that only `src/donation.rs` knows the donation link, `tests/bester_standort.rs`
  and `tests/symbolmenue.rs` that "Best location", the tray menu and the work in the background send nothing
  into the network and that only the version check asks GitHub.
- `tests/dienststart.rs` checks the start of the service: only `src/unit.rs` talks to the system bus, nothing
  stops, restarts, enables or disables the service, the password prompt is allowed in exactly one place, and
  the rule for polkit allows only starting this one unit.
- `tests/release.rs` keeps version, license, service unit, desktop entry, packages and documentation
  consistent with each other, and checks that every source file starts with the license header and that
  comments are English.

No test needs the real service, a network connection or a display.
