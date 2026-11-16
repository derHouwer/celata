// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Entry point of Celata: connects the user interface and the program logic.
//!
//! All calls to the service, to the server list and to the name resolution run in threads of their own,
//! so that the window never blocks. The results get back into the user interface through
//! `slint::invoke_from_event_loop`.
#![forbid(unsafe_code)]

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use celata::categories::{self, Category};
use celata::daemon::{Client, Flow, Observation};
use celata::desktop;
use celata::donation;
use celata::i18n::{self, Key, Language, Phrase};
use celata::look::{self, LocationRow, MainButton};
use celata::servers::{self, Country, Source};
use celata::settings::Class;
use celata::settings::{
    self, AppSettings, Applied, Edit, Environment, Field, FieldError, Filter, IpVersion, Ledger, PendingChange,
    Settings,
};
use celata::state::{
    CloseDecision, CloseDialog, DisconnectStart, DisplayState, Monitor, ROUTE_HINT, ReapplyGate, Start, StatusView,
    UNBLOCK_WARNING, reapply_notice, run_connect, run_disconnect, run_route, run_unblock,
};
use celata::streaming::{self, Check};
use celata::switch::{self, SwitchResult, Target};
use celata::{cli, dnsleak, flags, killswitch};
use celata_ui::{
    CategoryRow, ConfirmRow, DetailsRow, DonationTexts, HintRow, MainWindow, ServerRow, SettingsErrors, SettingsForm,
};
use slint::{CloseRequestResponse, ComponentHandle, Image, ModelRc, SharedString, VecModel};

/// Settings and everything that belongs to applying them.
struct SettingsStore {
    /// Stored set; applies from the next connection or after reconnecting.
    current: Settings,
    /// Settings for Celata only: show the donation hint, language. Never sent to the service.
    app: AppSettings,
    /// Stored selection of hidden settings: it has no effect anywhere and is only written back when
    /// saving.
    kept: Filter,
    /// What last went to the service.
    applied: Applied,
    settings_path: Option<PathBuf>,
    applied_path: Option<PathBuf>,
    /// Loaded category list; empty as long as "More filters" has not been opened.
    categories: Vec<Category>,
    /// Counts up when the input fields should show the stored value again (`SettingsForm.revision`).
    revision: i32,
    /// Search term for categories, so that the hits are recalculated after blocking one.
    category_query: String,
    /// Waits for the confirmation in the dialog.
    awaiting: Option<Awaiting>,
    /// Changes made while a connection exists; they are bundled and applied by reconnecting.
    pending: Option<PendingChange>,
}

/// What happens after the confirmation in the dialog.
enum Awaiting {
    /// Save these settings. The second field is only set for "Reset all settings to defaults": the new
    /// held-back selection from `settings::reset_all`.
    Settings(Box<Settings>, Option<Filter>),
    /// Connect from "Internet blocked", with a short gap for rebuilding the routes.
    Connect(String),
}

impl SettingsStore {
    fn save_applied(&self) {
        if let Some(path) = &self.applied_path
            && settings::save_applied(path, &self.applied).is_err()
        {
            report(Phrase::key(Key::TerminalLedgerSaveFailed));
        }
    }
}

/// Shared state of observation, connecting and user interface.
#[derive(Clone)]
struct Shared {
    window: slint::Weak<MainWindow>,
    monitor: Arc<Mutex<Monitor>>,
    countries: Arc<Mutex<Vec<Country>>>,
    /// Result of checking the streaming names at startup.
    streaming: Arc<Mutex<Check>>,
    settings: Arc<Mutex<SettingsStore>>,
    client: Client,
}

/// Flags as images, converted once per country. Only use it in the window thread (`slint::Image` is
/// not `Send`), which is why the cache lives in a `thread_local`.
#[derive(Default)]
struct Flags(RefCell<HashMap<String, Image>>);

impl Flags {
    /// Flag for the country code; unknown or not loadable yields the empty image, and the user interface
    /// then shows a grey placeholder.
    fn flag(&self, code: &str) -> Image {
        self.0
            .borrow_mut()
            .entry(code.to_owned())
            .or_insert_with(|| {
                flags::svg(code)
                    .and_then(|data| Image::load_from_svg_data(data).ok())
                    .unwrap_or_default()
            })
            .clone()
    }
}

thread_local! {
    static FLAGS: Flags = Flags::default();
}

fn flag(code: &str) -> Image {
    FLAGS.with(|flags| flags.flag(code))
}

/// Texts from Rust that are currently visible and are not created anew on every display. They stay
/// stored independent of the language and are translated again after a change of language. Only use it
/// in the window thread.
#[derive(Default)]
struct Shown {
    /// Info message at the bottom; its time limit keeps running independently.
    toast: Option<Phrase>,
    /// Open question: title, text, label of the confirming button.
    confirm: Option<(Phrase, Phrase, Phrase)>,
    /// Question on closing that was opened last; valid as long as `dialog-open` is true.
    close: Option<CloseDialog>,
    /// Message above the server list.
    list_message: Option<Phrase>,
    /// Message about the category list.
    categories_message: Option<Phrase>,
    /// Notice about the settings file.
    file_notice: Option<Phrase>,
}

thread_local! {
    static SHOWN: RefCell<Shown> = RefCell::default();
    /// Result of the last look at the name resolution of this system. Only used in the window thread.
    static DNS: Cell<dnsleak::Assessment> = const { Cell::new(dnsleak::Assessment::Unknown) };
}

/// How often Celata looks again whether DNS requests can bypass the VPN.
const DNS_CHECK_INTERVAL: Duration = Duration::from_secs(10);

/// Looks at a few local files and directories; no program is started and nothing is sent.
fn dns_assessment() -> dnsleak::Assessment {
    dnsleak::assess(&dnsleak::observe(Path::new("/")))
}

/// Text in the language in force.
fn tr(phrase: &Phrase) -> SharedString {
    phrase.render(i18n::current()).into()
}

/// Text of a key in the language in force.
fn tk(key: Key) -> SharedString {
    i18n::text(i18n::current(), key).into()
}

/// Message in the terminal in the language in force (English before the settings are loaded).
fn report(phrase: Phrase) {
    eprintln!(
        "{}",
        Phrase::fill(Key::TerminalPrefix, [phrase]).render(i18n::current())
    );
}

/// Registers Celata with the desktop, so that task bar and title bar show the logo. The application ID
/// can only be set after `MainWindow::new()`, because Slint has no platform before that, and must be set
/// before the window appears. Errors go to the terminal only; Celata keeps running normally.
fn register_desktop() {
    if let Err(error) = slint::set_xdg_app_id(desktop::APP_ID) {
        report(Phrase::fill(Key::TerminalAppIdFailed, [Phrase::raw(error.to_string())]));
    }
    let installed = std::env::current_exe().and_then(|exe| {
        let data_home = desktop::data_home().ok_or_else(|| std::io::Error::other("no data directory"))?;
        desktop::install(&exe, &data_home, &desktop::system_data_dirs())
    });
    if let Err(error) = installed {
        report(Phrase::fill(
            Key::TerminalDesktopEntryFailed,
            [Phrase::raw(error.to_string())],
        ));
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A crashed thread must not block the display permanently.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// If the routing table cannot be read, the internet counts as free. "Unblock internet" is thus never
/// offered without cause; the state of the service is shown anyway.
fn blocked() -> bool {
    killswitch::is_blocked().unwrap_or(false)
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if thread::Builder::new().name(name.to_owned()).spawn(work).is_err() {
        report(Phrase::fill(Key::TerminalThreadFailed, [Phrase::raw(name)]));
    }
}

fn main() -> ExitCode {
    // `celata --version` answers before anything else happens: no settings, no desktop entry, no
    // window.
    // `args_os`: an argument that is not valid Unicode must not abort the start.
    if cli::wants_version(
        std::env::args_os()
            .skip(1)
            .map(|argument| argument.to_string_lossy().into_owned()),
    ) {
        println!("{}", cli::version_line());
        return ExitCode::SUCCESS;
    }
    // Load the settings before the window, so that even the first picture appears in the stored
    // language.
    let settings_path = settings::config_path();
    let applied_path = settings::applied_path();
    let loaded = settings::load(settings_path.as_deref());
    let language = loaded.app.language;
    i18n::set_current(language);
    // Without a graphical session (neither Wayland nor X11 reachable) there is no window: a clear message
    // in the stored language instead of only the technical cause.
    let window = match MainWindow::new() {
        Ok(window) => window,
        Err(error) => {
            // Without the prefix "Celata:", the sentence already starts with the name.
            eprintln!(
                "{}",
                cli::no_session_message(&error.to_string()).render(i18n::current())
            );
            return ExitCode::FAILURE;
        }
    };
    register_desktop();
    let store = SettingsStore {
        current: loaded.settings,
        kept: loaded.kept,
        app: loaded.app,
        applied: settings::load_applied(applied_path.as_deref()),
        settings_path,
        applied_path,
        categories: Vec::new(),
        revision: 0,
        category_query: String::new(),
        awaiting: None,
        pending: None,
    };
    SHOWN.with(|shown| shown.borrow_mut().file_notice = loaded.notice.map(Phrase::key));
    window.set_filters_available(settings::FILTERS_WORK);
    // Before the first picture, so that the hint is there from the start.
    DNS.set(dns_assessment());
    let shared = Shared {
        window: window.as_weak(),
        monitor: Arc::new(Mutex::new(Monitor::new())),
        countries: Arc::new(Mutex::new(Vec::new())),
        streaming: Arc::new(Mutex::new(Check::Pending)),
        settings: Arc::new(Mutex::new(store)),
        client: Client::default(),
    };
    connect_settings(&window, &shared);
    connect_donation(&window, &shared);
    connect_language(&window, &shared);
    // Checks regularly whether a bundle of changes is due and whether the public address should be
    // determined or has exceeded its time limit.
    let bundle_timer = slint::Timer::default();
    let bundle = shared.clone();
    bundle_timer.start(slint::TimerMode::Repeated, Duration::from_millis(250), move || {
        check_pending(&bundle);
        let mut monitor = lock(&bundle.monitor);
        if monitor.expire_external_ip(Instant::now()) {
            let _ = push(&bundle, &monitor);
        }
        drop(monitor);
        request_external_ip(&bundle);
    });
    // The DNS hint follows the system without any action: it depends neither on the connection nor on a
    // setting, is not stored and never reaches the service.
    let dns_timer = slint::Timer::default();
    let dns = shared.clone();
    dns_timer.start(slint::TimerMode::Repeated, DNS_CHECK_INTERVAL, move || {
        let assessment = dns_assessment();
        if DNS.replace(assessment) != assessment
            && let Some(window) = dns.window.upgrade()
        {
            show_hints(&window, &lock(&dns.settings));
        }
    });
    start_detail_polling(&shared);
    let route = shared.clone();
    // "Block internet" takes effect immediately, without a question.
    window.on_route_request(move || start_route(&route));

    let search = shared.clone();
    window.on_search_changed(move |text| {
        if let Some(window) = search.window.upgrade() {
            show_servers(&window, &lock(&search.countries), &text);
            show_streaming(&window, &lock(&search.streaming), &text);
        }
    });
    // The location of the overview follows the selection in the list.
    let selection = shared.clone();
    window.on_selection_changed(move |_host| {
        if let Some(window) = selection.window.upgrade() {
            show_location(&window, &lock(&selection.countries));
        }
    });
    let reload = shared.clone();
    window.on_reload_servers(move || load_servers(&reload));
    let free = shared.clone();
    window.on_free_only_changed(move |_| {
        if let Some(window) = free.window.upgrade() {
            show_servers(&window, &lock(&free.countries), &window.get_search_text());
        }
    });
    let connect = shared.clone();
    window.on_connect(move |host| start_connect(&connect, host.to_string()));
    let disconnect = shared.clone();
    window.on_disconnect(move || {
        start_disconnect(&disconnect, false);
    });

    // Unblock internet: first the warning, then `/destroy` after confirmation.
    let unblock = shared.clone();
    window.on_unblock_request(move || {
        let mut monitor = lock(&unblock.monitor);
        monitor.ask_unblock();
        let _ = push(&unblock, &monitor);
    });
    let unblock = shared.clone();
    window.on_unblock_cancel(move || {
        let mut monitor = lock(&unblock.monitor);
        monitor.cancel_unblock();
        let _ = push(&unblock, &monitor);
    });
    let unblock = shared.clone();
    window.on_unblock_confirm(move || start_unblock(&unblock));

    // Closing: immediately or only after a question, depending on the state.
    let close = shared.clone();
    window.window().on_close_requested(move || request_close(&close));
    let close_disconnect = shared.clone();
    window.on_close_disconnect(move || {
        let Some(window) = close_disconnect.window.upgrade() else {
            return;
        };
        window.set_dialog_open(false);
        // If the state changed while the dialog was open (for example because the service was stopped),
        // there is nothing left to disconnect: then decide anew as with normal closing.
        if !start_disconnect(&close_disconnect, true)
            && request_close(&close_disconnect) == CloseRequestResponse::HideWindow
        {
            let _ = window.hide();
        }
    });
    // "Stay connected and close" does not call the service.
    let close_keep = shared.clone();
    window.on_close_keep(move || {
        if let Some(window) = close_keep.window.upgrade() {
            let _ = window.hide();
        }
    });

    // Set all texts in the stored language before the window appears. The streaming list also appears
    // complete at once, without waiting for the network or the server list.
    apply_language(&window, &shared, language);
    start_observer(&shared);
    load_servers(&shared);
    check_streaming(&shared);

    match window.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report(Phrase::raw(error.to_string()));
            ExitCode::FAILURE
        }
    }
}

/// Transfers the current display into the window. It is called under the lock of the monitor, so that
/// the displays arrive in the right order; `invoke_from_event_loop` does not wait.
fn push(shared: &Shared, monitor: &Monitor) -> Result<(), slint::EventLoopError> {
    let view = monitor.view();
    let tone = look::tone(monitor.display()).as_str();
    let button = look::main_button(monitor.display(), &view);
    let connected = matches!(monitor.display(), Some(DisplayState::Connected { .. }));
    let shared = shared.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(window) = shared.window.upgrade() {
            show(&window, &view, tone, button);
            show_location(&window, &lock(&shared.countries));
            show_settings_state(&window, &lock(&shared.settings), connected);
        }
    })
}

/// Starts the permanent observation of the service. It checks again by itself, which is why there is
/// no "check again" button.
fn start_observer(shared: &Shared) {
    let shared = shared.clone();
    spawn("observer", move || {
        shared.client.observe(blocked, |observation| {
            let mut monitor = lock(&shared.monitor);
            let (flow, changed) = monitor.observe(observation);
            // The event loop is no longer running: the window is closed.
            if changed && push(&shared, &monitor).is_err() {
                return Flow::Stop;
            }
            flow
        });
    });
}

/// Connects in the background, or switches the server while a connection exists. A second click during
/// an operation does nothing. If the routes have to be rebuilt from "Internet blocked", Celata asks
/// first, because the internet is free for a moment while doing so.
fn start_connect(shared: &Shared, host: String) {
    let blocked_start = {
        let monitor = lock(&shared.monitor);
        monitor.view().can_connect && monitor.display() == Some(&DisplayState::Blocked)
    };
    if blocked_start {
        let store = lock(&shared.settings);
        let ledger = Ledger::new(store.applied.clone(), settings::invocation_id());
        if ledger.needs_rebuild(&store.current) {
            drop(store);
            // Text: the routes have to be rebuilt, the internet is unprotected for a moment while doing so.
            ask(
                shared,
                Awaiting::Connect(host),
                Phrase::key(Key::ConfirmRebuildTitle),
                Phrase::key(Key::ConfirmRebuildText),
                Phrase::key(Key::ConfirmRebuildOk),
            );
            return;
        }
    }
    begin_connect(shared, host, false);
}

/// Starts connecting. `confirmed`: the question about the rebuild was confirmed.
fn begin_connect(shared: &Shared, host: String, confirmed: bool) {
    let start = {
        let mut monitor = lock(&shared.monitor);
        let Some(start) = monitor.begin_connect(&host) else {
            return;
        };
        let _ = push(shared, &monitor);
        start
    };
    let was_free = match start {
        Start::Connect { was_free } => was_free,
        Start::Switch { from } => return start_switch(shared, from, host),
    };
    let shared = shared.clone();
    spawn("connect", move || {
        let (target, mut ledger) = {
            let store = lock(&shared.settings);
            (
                Target::new(&host, store.current.clone()),
                Ledger::new(store.applied.clone(), settings::invocation_id()),
            )
        };
        // Nothing is ever rebuilt without confirmation; the route values then apply from the next rebuild.
        let rebuild = !was_free && confirmed && ledger.needs_rebuild(&target.settings);
        let result = run_connect(&shared.client, &mut ledger, &target, was_free, rebuild);
        remember(&shared, ledger);
        {
            let mut monitor = lock(&shared.monitor);
            monitor.finish_connect(&result);
            let _ = push(&shared, &monitor);
        }
        refresh(&shared);
    });
}

/// Switches from `from` to `to` in the background. The same thread also carries out a requested
/// "Disconnect", so that no `/connect` follows after `/destroy`.
fn start_switch(shared: &Shared, from: String, to: String) {
    let shared = shared.clone();
    spawn("switch", move || {
        let (current, mut ledger) = {
            let store = lock(&shared.settings);
            (
                store.current.clone(),
                Ledger::new(store.applied.clone(), settings::invocation_id()),
            )
        };
        // A switch does not change the settings (switch.rs, `run_reconnect`).
        let result = switch::run_reconnect(
            &shared.client,
            &mut ledger,
            &Target::new(&from, current.clone()),
            &Target::new(&to, current),
            Class::Reconnect,
            // While a connection exists, `Ok(true)` here means "block ready".
            killswitch::is_blocked,
            streaming::resolves,
            |phase| {
                let mut monitor = lock(&shared.monitor);
                let proceed = monitor.switch_proceed(phase);
                let _ = push(&shared, &monitor);
                proceed
            },
        );
        remember(&shared, ledger);
        if let SwitchResult::Stopped(destroyed) = result {
            // `/destroy` has already run; now finish as after "Disconnect".
            finish_disconnect(&shared, &destroyed);
            return;
        }
        {
            let mut monitor = lock(&shared.monitor);
            monitor.finish_switch(&result);
            let _ = push(&shared, &monitor);
        }
        refresh(&shared);
    });
}

/// Queries the state again after an own action, because `/watch` may have reported it before already.
fn refresh(shared: &Shared) {
    let observation = match shared.client.state() {
        Ok(state) => Observation::State {
            state,
            blocked: blocked(),
        },
        Err(error) => Observation::Failed {
            error,
            blocked: blocked(),
        },
    };
    let mut monitor = lock(&shared.monitor);
    monitor.observe(observation);
    let _ = push(shared, &monitor);
}

/// Disconnects in the background with `/destroy`. With `closing` the window only closes after success;
/// if it fails, the window stays open and shows the error and the emergency exit. During a switch its
/// thread takes over the disconnect after the running step.
/// Returns `false` if disconnecting is not possible in the current state.
fn start_disconnect(shared: &Shared, closing: bool) -> bool {
    {
        let mut monitor = lock(&shared.monitor);
        let start = monitor.begin_disconnect(closing);
        let _ = push(shared, &monitor);
        match start {
            DisconnectStart::Now => {}
            DisconnectStart::Deferred => return true,
            DisconnectStart::No => return false,
        }
    }
    let shared = shared.clone();
    spawn("disconnect", move || {
        let result = run_disconnect(&shared.client);
        finish_disconnect(&shared, &result);
    });
    true
}

/// Finishes the disconnect: close the window after a successful "Disconnect and close", otherwise query anew.
fn finish_disconnect(shared: &Shared, result: &Result<(), celata::daemon::Error>) {
    let close = {
        let mut monitor = lock(&shared.monitor);
        let close = monitor.finish_disconnect(result);
        let _ = push(shared, &monitor);
        close
    };
    if close {
        let window = shared.window.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(window) = window.upgrade() {
                let _ = window.hide();
            }
        });
    } else {
        refresh(shared);
    }
}

/// Lifts the block in the background, but only after the confirmed warning.
fn start_unblock(shared: &Shared) {
    {
        let mut monitor = lock(&shared.monitor);
        let confirmed = monitor.confirm_unblock();
        let _ = push(shared, &monitor);
        if !confirmed {
            return;
        }
    }
    let shared = shared.clone();
    spawn("unblock", move || {
        let result = run_unblock(&shared.client);
        {
            let mut monitor = lock(&shared.monitor);
            monitor.finish_unblock(&result);
            let _ = push(&shared, &monitor);
        }
        refresh(&shared);
    });
}

/// Decides on closing the window whether to ask.
fn request_close(shared: &Shared) -> CloseRequestResponse {
    let decision = lock(&shared.monitor).close_decision();
    let CloseDecision::Ask(dialog) = decision else {
        return CloseRequestResponse::HideWindow;
    };
    SHOWN.with(|shown| shown.borrow_mut().close = Some(dialog));
    if let Some(window) = shared.window.upgrade() {
        window.set_dialog_open(true);
        show_close_dialog(&window);
    }
    CloseRequestResponse::KeepWindowShown
}

/// Shows the texts of the open question on closing in the language in force.
fn show_close_dialog(window: &MainWindow) {
    if !window.get_dialog_open() {
        return;
    }
    SHOWN.with(|shown| {
        if let Some(dialog) = &shown.borrow().close {
            window.set_dialog_text(tk(dialog.text));
            window.set_dialog_offers_disconnect(dialog.offers_disconnect);
            window.set_dialog_keep_label(tk(dialog.keep_label));
            window.set_dialog_command(dialog.command.unwrap_or_default().into());
        }
    });
}

/// Loads the server list in the background, from the cache or from hide.me.
fn load_servers(shared: &Shared) {
    if let Some(window) = shared.window.upgrade() {
        window.set_list_state("loading".into());
        set_list_message(&window, None);
    }
    let shared = shared.clone();
    spawn("server-list", move || {
        let cache = servers::cache_path();
        let loaded = servers::load(cache.as_deref(), SystemTime::now(), servers::fetch_list);
        let _ = slint::invoke_from_event_loop(move || {
            let Some(window) = shared.window.upgrade() else { return };
            match loaded {
                Ok(loaded) => {
                    let message = match loaded.source {
                        Source::Stale => Some(Phrase::key(Key::ListStale)),
                        Source::Fresh | Source::Cached => None,
                    };
                    set_list_message(&window, message);
                    window.set_list_state("ready".into());
                    *lock(&shared.countries) = loaded.countries;
                }
                Err(error) => {
                    set_list_message(&window, Some(error.phrase()));
                    window.set_list_state("error".into());
                }
            }
            let countries = lock(&shared.countries);
            show_servers(&window, &countries, &window.get_search_text());
            show_location(&window, &countries);
        });
    });
}

/// Sets the message above the server list and remembers it for a change of language.
fn set_list_message(window: &MainWindow, message: Option<Phrase>) {
    window.set_list_message(message.as_ref().map(tr).unwrap_or_default());
    SHOWN.with(|shown| shown.borrow_mut().list_message = message);
}

/// Sets the notice about the settings file and remembers it for a change of language.
fn set_file_notice(window: &MainWindow, notice: Option<Phrase>) {
    window.set_settings_file_notice(notice.as_ref().map(tr).unwrap_or_default());
    SHOWN.with(|shown| shown.borrow_mut().file_notice = notice);
}

/// Sets the message about the category list and remembers it for a change of language.
fn set_categories_message(window: &MainWindow, message: Option<Phrase>) {
    window.set_categories_message(message.as_ref().map(tr).unwrap_or_default());
    SHOWN.with(|shown| shown.borrow_mut().categories_message = message);
}

/// Converts a row from `look` into the type of the user interface, with the flag from the image cache.
fn server_row(row: LocationRow) -> ServerRow {
    ServerRow {
        flag: flag(&row.code),
        country: row.country.into(),
        place: row.place.into(),
        host: row.host.into(),
        free: row.free,
        choice: row.choice.into(),
    }
}

/// Shows the servers matching the search term, one row per location; with "Free only" only those.
fn show_servers(window: &MainWindow, countries: &[Country], query: &str) {
    let mut rows = look::location_rows(&servers::search(countries, query), i18n::current());
    if window.get_free_only() {
        rows = look::free_only(rows);
    }
    let rows: Vec<ServerRow> = rows.into_iter().map(server_row).collect();
    window.set_servers(ModelRc::from(Rc::new(VecModel::from(rows))));
}

/// Checks once per program start which streaming names still exist. The result is neither stored nor
/// logged.
fn check_streaming(shared: &Shared) {
    let shared = shared.clone();
    spawn("streaming-check", move || {
        let result = streaming::check(streaming::resolves);
        let _ = slint::invoke_from_event_loop(move || {
            *lock(&shared.streaming) = result;
            if let Some(window) = shared.window.upgrade() {
                show_streaming(&window, &lock(&shared.streaming), &window.get_search_text());
            }
        });
    });
}

/// Shows the streaming servers that are visible and match the search term, one row per country.
fn show_streaming(window: &MainWindow, check: &Check, query: &str) {
    let rows: Vec<ServerRow> =
        look::streaming_rows(&streaming::search(&streaming::visible(check), query), i18n::current())
            .into_iter()
            .map(server_row)
            .collect();
    window.set_streaming_servers(ModelRc::from(Rc::new(VecModel::from(rows))));
}

/// Sets the location of the overview by `look::overview_location` from the current values of the window.
fn show_location(window: &MainWindow, countries: &[Country]) {
    let location = look::overview_location(
        &window.get_connected_host(),
        &window.get_selected_host(),
        &window.get_selected_name(),
        countries,
        i18n::current(),
    );
    window.set_location_label(location.label.into());
    window.set_location_flag(location.code.as_deref().map(flag).unwrap_or_default());
}

/// Transfers the display into the window.
fn show(window: &MainWindow, view: &StatusView, tone: &str, button: MainButton) {
    let language = i18n::current();
    // During a switch: "Switching to de.hideservers.net …" or "Returning to … …"; the server stands for
    // `{0}` in the title.
    let title = match &view.switch_target {
        Some(target) => Phrase::fill(view.message.title, [streaming::server_label(target)]),
        None => Phrase::key(view.message.title),
    };
    window.set_status_title(tr(&title));
    // Sidebar: "Switching to Streaming Japan …" instead of with the host name in brackets, so that the
    // whole state stays readable there.
    let short = match &view.switch_target {
        Some(target) => Phrase::fill(view.message.title, [streaming::short_label(target)]),
        None => Phrase::key(view.message.title),
    };
    window.set_status_short(tr(&short));
    window.set_status_text(tk(view.message.text));
    window.set_server(
        view.server
            .as_deref()
            .map(|host| tr(&streaming::server_label(host)))
            .unwrap_or_default(),
    );
    window.set_detail(view.detail.as_ref().map(tr).unwrap_or_default());
    window.set_notice(view.notice.as_ref().map(|notice| tr(&notice.text)).unwrap_or_default());
    window.set_version_warning(view.version_warning.as_ref().map(tr).unwrap_or_default());
    window.set_command(view.message.command.unwrap_or_default().into());
    window.set_warning(view.message.warning);
    window.set_tone(tone.into());
    window.set_main_label(tk(button.label));
    window.set_main_action(button.action.as_str().into());
    window.set_main_side(button.side.as_str().into());
    window.set_main_side_label(button.side_label.map(tk).unwrap_or_default());
    window.set_can_connect(view.can_connect);
    window.set_connecting(view.connecting);
    window.set_can_disconnect(view.can_disconnect);
    window.set_can_switch(view.can_switch);
    window.set_connected_host(view.connected_host.as_str().into());
    window.set_switching(view.switching);
    let details = view
        .service
        .as_ref()
        .map(|state| look::service_details(state, SystemTime::now(), language))
        .unwrap_or_default();
    window.set_details(DetailsRow {
        duration: details.duration.into(),
        received: details.received.into(),
        sent: details.sent.into(),
        address_v4: details.address_v4.into(),
        address_v6: details.address_v6.into(),
        dns_v4: details.dns_v4.into(),
        dns_v6: details.dns_v6.into(),
        public_v4: view.external_ip.column(false, language).into(),
        public_v6: view.external_ip.column(true, language).into(),
        // Block active (internet blocked, also while reconnecting or without the service): no query, the red
        // notice instead.
        public_blocked: view.message.warning,
    });
    window.set_unblock_asked(view.unblock_asked);
}

// ---------------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------------

/// Takes over the applied state after an operation and shows the state of the settings anew.
fn remember(shared: &Shared, ledger: Ledger) {
    let mut store = lock(&shared.settings);
    store.applied = ledger.applied;
    store.save_applied();
    drop(store);
    refresh_settings(shared);
}

/// Shows settings and their state anew in the window thread.
fn refresh_settings(shared: &Shared) {
    let shared = shared.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(window) = shared.window.upgrade() {
            let connected = matches!(lock(&shared.monitor).display(), Some(DisplayState::Connected { .. }));
            show_settings(&window, &lock(&shared.settings), connected);
        }
    });
}

fn strings(list: &[String]) -> ModelRc<SharedString> {
    ModelRc::from(Rc::new(VecModel::from(
        list.iter().map(SharedString::from).collect::<Vec<_>>(),
    )))
}

/// Converts the stored set into the display values of the page.
fn settings_form(store: &SettingsStore) -> SettingsForm {
    let settings = &store.current;
    let filter = &settings.filter;
    SettingsForm {
        ads: filter.ads,
        trackers: filter.trackers,
        malware: filter.malware,
        malicious: filter.malicious,
        pg: match filter.pg {
            12 => 1,
            18 => 2,
            21 => 3,
            _ => 0,
        },
        safe_search: filter.safe_search,
        risk_level: i32::from(settings::risk_level(&filter.risk)),
        illegal: !filter.illegal.is_empty(),
        whitelist: strings(&filter.whitelist),
        blacklist: strings(&filter.blacklist),
        force_dns: filter.force_dns,
        use_doh: settings.use_doh,
        plain_dns: strings(&settings.plain_dns),
        split_tunnel: strings(&settings.split_tunnel),
        port_forward: settings.port_forward,
        kill_switch: settings.kill_switch,
        ip_version: match settings.ip_version {
            IpVersion::Both => 0,
            IpVersion::V4Only => 1,
            IpVersion::V6Only => 2,
        },
        port: settings.port.to_string().into(),
        dpd_secs: settings.dpd_secs.to_string().into(),
        rest_timeout_secs: settings.rest_timeout_secs.to_string().into(),
        reconnect_wait_secs: settings.reconnect_wait_secs.to_string().into(),
        interface: settings.interface.as_str().into(),
        listen_port: settings.listen_port.to_string().into(),
        mark: settings.mark.to_string().into(),
        routing_table: settings.routing_table.to_string().into(),
        rule_priority: settings.rule_priority.to_string().into(),
        resolv_backup: settings.resolv_backup.as_str().into(),
        doh_servers: strings(&settings.doh_servers),
        doh_summary: doh_summary(settings),
        revision: store.revision,
    }
}

/// Summary of the DNS-over-HTTPS servers instead of 18 long stamps.
fn doh_summary(settings: &Settings) -> SharedString {
    let count = Phrase::raw(settings.doh_servers.len().to_string());
    if settings.doh_servers == Settings::default().doh_servers {
        tr(&Phrase::fill(Key::DohDefault, [count]))
    } else if settings.doh_servers.is_empty() {
        tk(Key::DohNone)
    } else {
        tr(&Phrase::fill(Key::DohCustom, [count]))
    }
}

/// Assigns errors to the fields of the page.
fn settings_errors(errors: &[FieldError]) -> SettingsErrors {
    let mut shown = SettingsErrors::default();
    for error in errors {
        let message = tr(&error.message);
        let slot = match error.field {
            Field::Ads
            | Field::Trackers
            | Field::Malware
            | Field::Malicious
            | Field::Pg
            | Field::SafeSearch
            | Field::Risk
            | Field::Illegal
            | Field::Categories => &mut shown.filter,
            Field::Whitelist => &mut shown.whitelist,
            Field::Blacklist => &mut shown.blacklist,
            Field::PlainDns => &mut shown.plain_dns,
            Field::SplitTunnel => &mut shown.split_tunnel,
            Field::IpVersion => &mut shown.ip_version,
            Field::Port => &mut shown.port,
            Field::DpdSecs => &mut shown.dpd_secs,
            Field::RestTimeoutSecs => &mut shown.rest_timeout_secs,
            Field::ReconnectWaitSecs => &mut shown.reconnect_wait_secs,
            Field::Interface => &mut shown.interface,
            Field::ListenPort => &mut shown.listen_port,
            Field::Mark => &mut shown.mark,
            Field::RoutingTable => &mut shown.routing_table,
            Field::RulePriority => &mut shown.rule_priority,
            Field::ResolvBackup => &mut shown.resolv_backup,
            Field::DohServers => &mut shown.doh_servers,
            Field::ForceDns | Field::UseDoh | Field::PortForward | Field::KillSwitch => &mut shown.general,
        };
        if slot.is_empty() {
            *slot = message;
        }
    }
    shown
}

/// Shows settings, their state and the categories.
fn show_settings(window: &MainWindow, store: &SettingsStore, connected: bool) {
    window.set_settings(settings_form(store));
    show_settings_state(window, store, connected);
    show_categories(window, store);
    show_hints(window, store);
}

/// Hints of the overview: the DNS hint first, then one per setting that lowers the protection.
fn show_hints(window: &MainWindow, store: &SettingsStore) {
    let hints: Vec<HintRow> = settings::active_hints(&store.current, DNS.get())
        .into_iter()
        .map(|hint| HintRow {
            text: tr(&hint.text),
            alert: hint.alert,
        })
        .collect();
    window.set_hints(ModelRc::from(Rc::new(VecModel::from(hints))));
}

/// "Applies to the current connection.", "Applies from the next connection." or "Applying …".
fn show_settings_state(window: &MainWindow, store: &SettingsStore, connected: bool) {
    let text = if store.pending.is_some() {
        Key::SettingsApplying
    } else {
        // The `InvocationID` is only read before an operation; in between the last known one applies.
        settings::state_text(
            &store.applied,
            &store.current,
            store.applied.invocation.as_deref(),
            connected,
        )
    };
    window.set_settings_state(tk(text));
}

/// Shows blocked categories and the hits of the search.
fn show_categories(window: &MainWindow, store: &SettingsStore) {
    let row = |name: &str, description: &str, selected| CategoryRow {
        name: name.into(),
        description: description.into(),
        selected,
    };
    let chosen = &store.current.filter.categories;
    let blocked: Vec<CategoryRow> = chosen
        .iter()
        .map(|name| {
            let description = store
                .categories
                .iter()
                .find(|category| &category.name == name)
                .map_or("", |category| category.description.as_str());
            row(name, description, true)
        })
        .collect();
    let hits: Vec<CategoryRow> = categories::search(&store.categories, &store.category_query, chosen)
        .into_iter()
        .map(|category| row(&category.name, &category.description, false))
        .collect();
    window.set_blocked_categories(ModelRc::from(Rc::new(VecModel::from(blocked))));
    window.set_categories(ModelRc::from(Rc::new(VecModel::from(hits))));
}

/// Opens the question in the dialog. The texts stay stored as `Phrase`, so that a change of language
/// translates the open question anew.
fn ask(shared: &Shared, awaiting: Awaiting, title: Phrase, text: Phrase, ok_label: Phrase) {
    lock(&shared.settings).awaiting = Some(awaiting);
    SHOWN.with(|shown| shown.borrow_mut().confirm = Some((title, text, ok_label)));
    if let Some(window) = shared.window.upgrade() {
        show_confirm(&window);
    }
}

/// Shows the open question in the language in force, or none.
fn show_confirm(window: &MainWindow) {
    let row = SHOWN.with(|shown| {
        shown
            .borrow()
            .confirm
            .as_ref()
            .map(|(title, text, ok_label)| ConfirmRow {
                open: true,
                title: tr(title),
                text: tr(text),
                danger: true,
                ok_label: tr(ok_label),
            })
    });
    window.set_confirm(row.unwrap_or_default());
}

fn close_confirm(shared: &Shared) -> Option<Awaiting> {
    SHOWN.with(|shown| shown.borrow_mut().confirm = None);
    if let Some(window) = shared.window.upgrade() {
        window.set_confirm(ConfirmRow::default());
    }
    lock(&shared.settings).awaiting.take()
}

/// Checks a change and saves it.
fn handle_edit(shared: &Shared, edit: Edit) {
    let Some(window) = shared.window.upgrade() else { return };
    let mut store = lock(&shared.settings);
    let known: Vec<String> = store.categories.iter().map(|category| category.name.clone()).collect();
    match settings::apply_edit(&store.current, edit, &Environment::from_system(), &known) {
        Err(errors) => {
            window.set_settings_errors(settings_errors(&errors));
            // Show the stored value again, for example at a switch.
            window.set_settings(settings_form(&store));
        }
        Ok(new) => {
            window.set_settings_errors(SettingsErrors::default());
            if new == store.current {
                return;
            }
            let gate = lock(&shared.monitor).reapply_gate();
            let connected = matches!(gate, ReapplyGate::Now(_));
            let warnings = settings::warnings(&store.current, &new, connected);
            if warnings.is_empty() {
                store_settings(&window, &mut store, new, &gate);
            } else {
                // Paragraphs stay `Phrase`, so that the open question changes along with the language.
                let text = warnings
                    .into_iter()
                    .map(|warning| warning.text)
                    .reduce(|first, next| Phrase::fill(Key::ParagraphPair, [first, next]))
                    .unwrap_or_else(|| Phrase::raw(""));
                drop(store);
                ask(
                    shared,
                    Awaiting::Settings(Box::new(new), None),
                    Phrase::key(Key::ConfirmSettingTitle),
                    text,
                    Phrase::key(Key::ConfirmSettingOk),
                );
            }
        }
    }
}

/// Saves the new set and shows it. While a connection exists this starts or extends the 2 second
/// period until reconnecting; without a connection the set applies from the next one.
fn store_settings(window: &MainWindow, store: &mut SettingsStore, new: Settings, gate: &ReapplyGate) {
    let old = std::mem::replace(&mut store.current, new);
    let now = Instant::now();
    match gate {
        ReapplyGate::Never => store.pending = None,
        ReapplyGate::Now(_) | ReapplyGate::Wait => match &mut store.pending {
            Some(pending) => pending.extend(now),
            None => store.pending = Some(PendingChange::new(old, now)),
        },
    }
    let connected = matches!(gate, ReapplyGate::Now(_));
    if let Some(path) = &store.settings_path {
        match settings::save(path, &store.current, &store.kept, &store.app) {
            Ok(()) => set_file_notice(window, None),
            Err(error) => set_file_notice(window, Some(save_failed(&error))),
        }
    }
    show_settings(window, store, connected);
}

/// Connects the callbacks of the page "Settings".
fn connect_settings(window: &MainWindow, shared: &Shared) {
    let edit = shared.clone();
    window.on_setting_toggled(move |key, on| handle_edit(&edit, Edit::Toggle(key.into(), on)));
    let edit = shared.clone();
    window.on_setting_choice(move |key, value| handle_edit(&edit, Edit::Choice(key.into(), value.into())));
    let edit = shared.clone();
    window.on_setting_text(move |key, value| handle_edit(&edit, Edit::Text(key.into(), value.into())));
    let edit = shared.clone();
    window.on_list_add(move |key, value| handle_edit(&edit, Edit::ListAdd(key.into(), value.into())));
    let edit = shared.clone();
    window.on_list_remove(move |key, index| {
        if let Ok(index) = usize::try_from(index) {
            handle_edit(&edit, Edit::ListRemove(key.into(), index));
        }
    });
    let reset = shared.clone();
    window.on_settings_reset(move |section| {
        let Some(window) = reset.window.upgrade() else { return };
        let mut store = lock(&reset.settings);
        // "All" is decided by `settings::reset_all`, because hidden settings belong to it as well. `None`
        // means: everything is at its default already, no question needed.
        let target = if section == "all" {
            settings::reset_all(&store.current, &store.kept).map(|(new, kept)| (new, Some(kept)))
        } else {
            settings::apply_edit(
                &store.current,
                Edit::Reset(section.to_string()),
                &Environment::default(),
                &[],
            )
            .ok()
            .filter(|new| *new != store.current)
            .map(|new| (new, None))
        };
        // Rejected input was never saved: clear the errors and show the stored value everywhere.
        window.set_settings_errors(SettingsErrors::default());
        store.revision += 1;
        window.set_settings(settings_form(&store));
        let Some((new, kept)) = target else { return };
        drop(store);
        let text = if section != "all" {
            Key::ResetAdvancedText
        } else if settings::FILTERS_WORK {
            Key::ResetAllWithFiltersText
        } else {
            Key::ResetAllText
        };
        ask(
            &reset,
            Awaiting::Settings(Box::new(new), kept),
            Phrase::key(Key::ConfirmResetTitle),
            Phrase::key(text),
            Phrase::key(Key::ConfirmResetOk),
        );
    });
    let confirm = shared.clone();
    window.on_confirm_ok(move || {
        let Some(window) = confirm.window.upgrade() else { return };
        match close_confirm(&confirm) {
            Some(Awaiting::Settings(new, kept)) => {
                let new = *new;
                window.set_settings_errors(SettingsErrors::default());
                // The period only starts after the confirmation.
                let gate = lock(&confirm.monitor).reapply_gate();
                let mut store = lock(&confirm.settings);
                if let Some(kept) = kept {
                    store.kept = kept;
                }
                store_settings(&window, &mut store, new, &gate);
            }
            Some(Awaiting::Connect(host)) => begin_connect(&confirm, host, true),
            None => {}
        }
    });
    let cancel = shared.clone();
    window.on_confirm_cancel(move || {
        close_confirm(&cancel);
        // Switches and fields show the stored value again.
        if let Some(window) = cancel.window.upgrade() {
            let mut store = lock(&cancel.settings);
            store.revision += 1;
            window.set_settings(settings_form(&store));
        }
    });
    let more = shared.clone();
    window.on_open_more_filters(move || load_categories(&more));
    let search = shared.clone();
    window.on_category_search(move |text| {
        let Some(window) = search.window.upgrade() else { return };
        let mut store = lock(&search.settings);
        store.category_query = text.to_string();
        show_categories(&window, &store);
    });
}

/// Loads the category list in the background, when "More filters" is expanded for the first time.
fn load_categories(shared: &Shared) {
    // Hidden: do not fetch any data.
    if !settings::FILTERS_WORK {
        return;
    }
    let Some(window) = shared.window.upgrade() else { return };
    if !lock(&shared.settings).categories.is_empty() {
        window.set_categories_state("ready".into());
        return;
    }
    window.set_categories_state("loading".into());
    set_categories_message(&window, None);
    let shared = shared.clone();
    spawn("categories", move || {
        let cache = categories::cache_path();
        let loaded = categories::load(cache.as_deref(), SystemTime::now(), categories::fetch);
        let _ = slint::invoke_from_event_loop(move || {
            let Some(window) = shared.window.upgrade() else { return };
            let mut store = lock(&shared.settings);
            match loaded {
                Ok(list) => {
                    store.categories = list;
                    window.set_categories_state("ready".into());
                }
                Err(error) => {
                    set_categories_message(&window, Some(error.phrase()));
                    window.set_categories_state("error".into());
                }
            }
            show_categories(&window, &store);
        });
    });
}

/// Starts reconnecting when the bundle is due and no dialog is open.
fn check_pending(shared: &Shared) {
    let mut store = lock(&shared.settings);
    let due = store
        .pending
        .as_ref()
        .is_some_and(|pending| pending.due(Instant::now()));
    if !due || store.awaiting.is_some() {
        return;
    }
    let mut monitor = lock(&shared.monitor);
    let host = match monitor.reapply_gate() {
        ReapplyGate::Wait => return,
        ReapplyGate::Never => {
            // The connection is gone meanwhile: applies from the next connection.
            store.pending = None;
            drop(monitor);
            if let Some(window) = shared.window.upgrade() {
                show_settings_state(&window, &store, false);
            }
            return;
        }
        ReapplyGate::Now(host) => host,
    };
    let Some(pending) = store.pending.take() else { return };
    let to = store.current.clone();
    // The starting point is what actually went to the service last; if a change already went along when
    // connecting, nothing is left to do. Hidden values from an earlier version do not count here: hiding
    // alone does not trigger a reconnect.
    let base = store
        .applied
        .sent
        .as_ref()
        .map_or_else(|| pending.old.clone(), settings::visible);
    let Some(class) = settings::changed_class(&base, &to) else {
        // Everything as before again: nothing to do.
        drop(monitor);
        if let Some(window) = shared.window.upgrade() {
            show_settings_state(&window, &store, true);
        }
        return;
    };
    if monitor.begin_reapply().is_none() {
        store.pending = Some(pending);
        return;
    }
    let _ = push(shared, &monitor);
    drop(monitor);
    let applied = store.applied.clone();
    drop(store);
    let from = Target::new(&host, base);
    let to = Target::new(&host, to);
    let shared = shared.clone();
    spawn("reconnect", move || {
        let mut ledger = Ledger::new(applied, settings::invocation_id());
        let result = switch::run_reconnect(
            &shared.client,
            &mut ledger,
            &from,
            &to,
            class,
            killswitch::is_blocked,
            streaming::resolves,
            |phase| {
                let mut monitor = lock(&shared.monitor);
                let proceed = monitor.switch_proceed(phase);
                let _ = push(&shared, &monitor);
                proceed
            },
        );
        remember(&shared, ledger);
        if let SwitchResult::Stopped(destroyed) = result {
            finish_disconnect(&shared, &destroyed);
            return;
        }
        {
            let mut monitor = lock(&shared.monitor);
            monitor.finish_switch(&result);
            let _ = push(&shared, &monitor);
        }
        if let Some(text) = reapply_notice(&result) {
            show_toast(&shared, text, result != SwitchResult::Switched);
        }
        refresh(&shared);
    });
}

// ---------------------------------------------------------------------------------------------------
// Language
// ---------------------------------------------------------------------------------------------------

/// Notice that the settings file could not be saved; the error text of the system stays unchanged.
fn save_failed(error: &std::io::Error) -> Phrase {
    Phrase::fill(Key::SettingsSaveFailed, [Phrase::raw(error.to_string())])
}

/// Connects the language selection. `i18n::choose` makes the decision; nothing goes to the service, and
/// Celata never reconnects because of it.
fn connect_language(window: &MainWindow, shared: &Shared) {
    let names: Vec<SharedString> = Language::ALL
        .into_iter()
        .map(|language| language.native_name().into())
        .collect();
    window.set_language_names(ModelRc::from(Rc::new(VecModel::from(names))));
    let choose = shared.clone();
    window.on_language_chosen(move |index| {
        let Some(window) = choose.window.upgrade() else { return };
        // The drop-down list reports the position in `Language::ALL`; unknown positions do nothing.
        let code = usize::try_from(index)
            .ok()
            .and_then(|index| Language::ALL.get(index))
            .map_or("", |language| language.code());
        let Some(language) = i18n::choose(i18n::current(), code) else {
            return;
        };
        let mut store = lock(&choose.settings);
        store.app.language = language;
        let saved = match &store.settings_path {
            Some(path) => settings::save(path, &store.current, &store.kept, &store.app).is_ok(),
            None => true,
        };
        drop(store);
        // The language applies even if saving fails.
        apply_language(&window, &choose, language);
        if !saved {
            show_toast(&choose, Phrase::key(Key::LanguageSaveFailed), true);
        }
    });
}

/// Switches user interface, Rust texts and terminal messages to `language` together and sets all texts
/// anew. The language changes only here.
fn apply_language(window: &MainWindow, shared: &Shared, language: Language) {
    let language = match slint::select_bundled_translation(language.code()) {
        Ok(()) => language,
        Err(_) => {
            // If the bundled translation is missing, rather everything in English than a mixed user interface.
            report(Phrase::fill(
                Key::TerminalTranslationMissing,
                [Phrase::raw(language.code())],
            ));
            let _ = slint::select_bundled_translation(Language::En.code());
            Language::En
        }
    };
    i18n::set_current(language);
    let index = Language::ALL
        .iter()
        .position(|known| *known == language)
        .unwrap_or_default();
    window.set_language_index(i32::try_from(index).unwrap_or_default());
    refresh_texts(window, shared);
}

/// Sets all texts from Rust anew in the language in force: state, overview, lists, settings, donation
/// texts and the messages and questions currently visible. View, search, selection and time limits
/// stay unchanged.
fn refresh_texts(window: &MainWindow, shared: &Shared) {
    let monitor = lock(&shared.monitor);
    let view = monitor.view();
    let connected = matches!(monitor.display(), Some(DisplayState::Connected { .. }));
    show(
        window,
        &view,
        look::tone(monitor.display()).as_str(),
        look::main_button(monitor.display(), &view),
    );
    drop(monitor);
    show_fixed_texts(window);
    {
        let countries = lock(&shared.countries);
        show_servers(window, &countries, &window.get_search_text());
        show_location(window, &countries);
    }
    show_streaming(window, &lock(&shared.streaming), &window.get_search_text());
    show_settings(window, &lock(&shared.settings), connected);
    show_close_dialog(window);
    show_confirm(window);
    SHOWN.with(|shown| {
        let shown = shown.borrow();
        if let Some(toast) = &shown.toast {
            window.set_toast(tr(toast));
        }
        window.set_list_message(shown.list_message.as_ref().map(tr).unwrap_or_default());
        window.set_categories_message(shown.categories_message.as_ref().map(tr).unwrap_or_default());
        window.set_settings_file_notice(shown.file_notice.as_ref().map(tr).unwrap_or_default());
    });
}

/// Texts that only change with the language: donation hint, warning before unblocking, note on
/// "Block internet".
fn show_fixed_texts(window: &MainWindow) {
    window.set_donation_texts(DonationTexts {
        card_title: tk(Key::DonationCardTitle),
        card_text: tk(Key::DonationCardText),
        button: tk(Key::DonationButton),
        button_label: tk(Key::DonationButtonLabel),
        section_title: tk(Key::DonationSectionTitle),
        section_text: tk(Key::DonationSectionText),
        toggle: tk(Key::DonationToggle),
        toggle_description: tk(Key::DonationToggleDescription),
    });
    window.set_unblock_warning(tk(UNBLOCK_WARNING));
    window.set_route_hint(tk(ROUTE_HINT));
}

/// Duration of the info message.
const TOAST_TIME: Duration = Duration::from_secs(8);

thread_local! {
    /// Counts the info messages, so that an older time limit does not delete a newer message.
    static TOAST_GENERATION: Cell<u64> = const { Cell::new(0) };
}

/// Connects the donation hint: texts from `src/donation.rs`, click on one of the two donation
/// buttons.
fn connect_donation(window: &MainWindow, shared: &Shared) {
    // Switch "Show donation hint": save and show at once. Not through `handle_edit`: no validation, no
    // question, no 2 second period, nothing to the service.
    window.set_donation_hint(lock(&shared.settings).app.donation_hint);
    let toggle = shared.clone();
    window.on_donation_hint_toggled(move |on| {
        let Some(window) = toggle.window.upgrade() else { return };
        let mut store = lock(&toggle.settings);
        store.app.donation_hint = on;
        if let Some(path) = &store.settings_path {
            match settings::save(path, &store.current, &store.kept, &store.app) {
                Ok(()) => set_file_notice(&window, None),
                Err(error) => set_file_notice(&window, Some(save_failed(&error))),
            }
        }
        drop(store);
        // The display follows the switch even if saving fails.
        window.set_donation_hint(on);
    });
    // One protection for both buttons; lives in the window thread like the callback.
    let debounce = Rc::new(RefCell::new(donation::Debounce::default()));
    let click = shared.clone();
    window.on_donate(move || {
        if debounce.borrow_mut().allow(Instant::now()) {
            start_donation(&click);
        }
    });
}

/// Opens the Ko-fi page in the default browser, in a thread of its own, so that the window never waits
/// for `xdg-open`. With blocked internet also a notice; the block stays untouched. If the call fails, a
/// message with the link for copying. Stores and counts nothing.
fn start_donation(shared: &Shared) {
    let blocked = donation::is_blocked(lock(&shared.monitor).display());
    if let Some(text) = donation::click_notice(blocked) {
        show_toast(shared, Phrase::key(text), false);
    }
    let failed = shared.clone();
    spawn("donation", move || {
        if donation::open(donation::KOFI_URL).is_err() {
            show_toast_link(
                &failed,
                Phrase::key(donation::FAILED_NOTICE),
                donation::KOFI_URL.to_owned(),
            );
        }
    });
}

/// Duration of an info message with a link, so that there is enough time to copy it.
const TOAST_LINK_TIME: Duration = Duration::from_secs(30);

/// Shows an info message at the bottom above the content for 8 seconds.
fn show_toast(shared: &Shared, text: Phrase, alert: bool) {
    show_toast_for(shared, text, alert, String::new(), TOAST_TIME);
}

/// Shows a red info message with a link for copying for 30 seconds.
fn show_toast_link(shared: &Shared, text: Phrase, link: String) {
    show_toast_for(shared, text, true, link, TOAST_LINK_TIME);
}

/// Common path of both kinds of message: a counter makes sure that an older time limit does not delete
/// a newer message, whatever its kind. The message stays stored as `Phrase`, so that a change of
/// language translates it anew without changing its time limit.
fn show_toast_for(shared: &Shared, text: Phrase, alert: bool, link: String, time: Duration) {
    let window = shared.window.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(shown) = window.upgrade() else { return };
        shown.set_toast(tr(&text));
        SHOWN.with(|state| state.borrow_mut().toast = Some(text));
        shown.set_toast_alert(alert);
        shown.set_toast_link(link.into());
        let generation = TOAST_GENERATION.with(|counter| {
            counter.set(counter.get() + 1);
            counter.get()
        });
        slint::Timer::single_shot(time, move || {
            if TOAST_GENERATION.with(Cell::get) == generation
                && let Some(shown) = window.upgrade()
            {
                shown.set_toast(SharedString::new());
                shown.set_toast_link(SharedString::new());
                SHOWN.with(|state| state.borrow_mut().toast = None);
            }
        });
    });
}

/// Interval of querying `/state` for duration and amount of data.
const DETAIL_INTERVAL: Duration = Duration::from_secs(5);

/// Queries `/state` every 5 seconds as long as "Connected" is shown and no own action is running.
/// `/watch` only reports changes of state, not the running amount of data.
fn start_detail_polling(shared: &Shared) {
    let shared = shared.clone();
    spawn("details", move || {
        loop {
            thread::sleep(DETAIL_INTERVAL);
            // The event loop is no longer running: the window is closed.
            if slint::invoke_from_event_loop(|| {}).is_err() {
                return;
            }
            let poll = {
                let monitor = lock(&shared.monitor);
                matches!(monitor.display(), Some(DisplayState::Connected { .. })) && monitor.view().can_switch
            };
            if !poll {
                continue;
            }
            match shared.client.state() {
                // HTTP 409 (connecting or disconnecting) is skipped; the observation reports errors itself.
                Err(_) => {}
                Ok(state) => {
                    let blocked = blocked();
                    let mut monitor = lock(&shared.monitor);
                    monitor.observe(Observation::State { state, blocked });
                    // Show anew even without a change of state, because duration and amount of data change.
                    let _ = push(&shared, &monitor);
                }
            }
        }
    });
}

/// Determines the public address when the `Monitor` considers it due: `/externalIps` in the
/// background, the result arrives through `/watch`.
fn request_external_ip(shared: &Shared) {
    {
        let mut monitor = lock(&shared.monitor);
        if !monitor.begin_external_ip(Instant::now()) {
            return;
        }
        let _ = push(shared, &monitor);
    }
    let shared = shared.clone();
    spawn("public-address", move || {
        if shared.client.external_ips().is_err() {
            let mut monitor = lock(&shared.monitor);
            monitor.external_ip_failed(Instant::now());
            let _ = push(&shared, &monitor);
        }
    });
}

/// "Block internet": settings and `/route` in the background.
fn start_route(shared: &Shared) {
    let settings = lock(&shared.settings).current.clone();
    {
        let mut monitor = lock(&shared.monitor);
        if !monitor.begin_route(settings.kill_switch) {
            return;
        }
        let _ = push(shared, &monitor);
    }
    let shared = shared.clone();
    spawn("block-internet", move || {
        let applied = lock(&shared.settings).applied.clone();
        let mut ledger = Ledger::new(applied, settings::invocation_id());
        let result = run_route(&shared.client, &mut ledger, &settings);
        remember(&shared, ledger);
        {
            let mut monitor = lock(&shared.monitor);
            monitor.finish_route(&result);
            let _ = push(&shared, &monitor);
        }
        refresh(&shared);
    });
}
