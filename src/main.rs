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
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use celata::autostart::{self, AutostartState};
use celata::background::{self, CloseOutcome, StartMode};
use celata::best::{self, Choice};
use celata::categories::{self, Category};
use celata::daemon::{Client, Flow, Observation};
use celata::desktop;
use celata::donation;
use celata::favorites;
use celata::i18n::{self, Key, Language, Phrase};
use celata::instance::{self, Claim};
use celata::look::{self, ConnectWish, MainButton, MenuAction, MenuDecision, StateColor};
use celata::notify;
use celata::presence::{self, TrayPresence};
use celata::servers::{self, Country, Source};
use celata::settings::Class;
use celata::settings::{
    self, AppSettings, Appearance, Applied, Edit, Environment, Field, FieldError, Filter, IpVersion, Ledger,
    PendingChange, Settings,
};
use celata::state::{
    self, CloseDecision, CloseDialog, DisconnectStart, DisplayState, Monitor, ROUTE_HINT, ReapplyGate, Start,
    StartButton, StatusView, UNBLOCK_WARNING, reapply_notice, run_connect, run_disconnect, run_route, run_unblock,
};
use celata::streaming::{self, Check};
use celata::switch::{self, SwitchResult, Target};
use celata::{about, cli, dnsleak, flags, killswitch, trayicon, unit, update, zone};
use celata_ui::{
    CategoryRow, CelataTray, ConfirmRow, DetailsRow, DonationTexts, FavoriteItem, HintRow, LocationItem, MainWindow,
    SettingsErrors, SettingsForm, TrayContinent, TrayCountry, TrayPlace, TraySlot,
};
use slint::{CloseRequestResponse, ComponentHandle, Image, Model, ModelRc, SharedString, VecModel};

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
    Connect(Pick),
}

/// Host an operation connects or switches to, and whether "Best location" chose it.
#[derive(Clone)]
struct Pick {
    host: String,
    /// `None`: the user chose the location. Otherwise "Best location" chose `host`, and these are the further
    /// candidates (at most `best::MAX_TRIES` − 1) for the check of names during a switch.
    best: Option<Vec<String>>,
}

impl Pick {
    /// A location the user chose.
    fn own(host: String) -> Self {
        Self { host, best: None }
    }

    fn via_best(&self) -> bool {
        self.best.is_some()
    }
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
    /// Texts for desktop notifications; `None` if the thread could not start.
    notifier: Option<Sender<String>>,
    /// Display last handed to the window, to find the change worth a notification. Set under the lock of the
    /// monitor, so that the changes are seen in the right order.
    last_shown: Arc<Mutex<notify::Shown>>,
    /// State of the location list that lasts only for this session.
    session: Arc<Mutex<Session>>,
}

/// State of the location list for this session only; never stored.
#[derive(Default)]
struct Session {
    /// Unfolded countries (codes) in the tab "Standard".
    expanded: BTreeSet<String>,
    /// Country names built by Celata with which connecting or switching failed.
    failed_derived: BTreeSet<String>,
    /// What "Best location" chose and which of its hosts failed (feature 013); never stored.
    best: best::Memory,
}

/// The models of the three location lists. The same models stay for the whole run and only the rows that
/// changed are replaced, so that the list views keep their scroll position. Only use it in the window
/// thread.
#[derive(Default)]
struct Lists {
    servers: Rc<VecModel<LocationItem>>,
    streaming: Rc<VecModel<LocationItem>>,
    favorites: Rc<VecModel<FavoriteItem>>,
    shown_servers: Vec<look::LocationItem>,
    shown_streaming: Vec<look::LocationItem>,
    shown_favorites: Vec<look::FavoriteItem>,
    /// Connected host the lists were last built for; they are built anew when it changes.
    connected: SharedString,
}

thread_local! {
    static LISTS: RefCell<Lists> = RefCell::default();
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
    /// Target of the running connection attempt as last shown (`StatusView::connect_target`), for the card
    /// "Location". Only used in the window thread.
    static CONNECT_TARGET: RefCell<Option<String>> = const { RefCell::new(None) };
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
    let installed = desktop::program_path().and_then(|exe| {
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

thread_local! {
    /// Icon in the system tray. Created only once the system tray is there (Slint tries to register an icon
    /// only once); `None` before that or without a system tray. Only used in the window thread.
    static TRAY: RefCell<Option<CelataTray>> = const { RefCell::new(None) };
    /// Last answer of the thread `presence`. Only used in the window thread.
    static PRESENCE: Cell<TrayPresence> = const { Cell::new(TrayPresence::Unknown) };
}

/// Creates the icon in the system tray. If the logo cannot be drawn or Slint fails, Celata runs on without the
/// icon and without a message; main button and sidebar show the state alone.
fn create_tray(shared: &Shared) -> Option<CelataTray> {
    let tray = CelataTray::new().ok()?;
    // The first picture; `show_state_icon` sets the colour and text of the state from then on.
    tray.set_state_icon(logo(StateColor::Normal)?);
    let activate = shared.clone();
    tray.on_activate(move || bring_window_back(&activate));
    connect_tray_menu(&tray, shared);
    Some(tray)
}

/// Connects the entries of the menu. Each one only names what was chosen; `menu_choice` decides in the state
/// at the time of the choice.
fn connect_tray_menu(tray: &CelataTray, shared: &Shared) {
    let menu = shared.clone();
    tray.on_show_window(move || menu_choice(&menu, &MenuAction::ShowWindow));
    let menu = shared.clone();
    tray.on_main_action(move || menu_choice(&menu, &MenuAction::Main));
    let menu = shared.clone();
    tray.on_connect_best(move || menu_choice(&menu, &MenuAction::Best));
    let menu = shared.clone();
    tray.on_side_action(move || menu_choice(&menu, &MenuAction::Side));
    let menu = shared.clone();
    tray.on_choose(move |host| menu_choice(&menu, &MenuAction::Choose(host.to_string())));
    let menu = shared.clone();
    tray.on_toggle_favorite(move || menu_choice(&menu, &MenuAction::ToggleFavorite));
    let menu = shared.clone();
    tray.on_open_update(move || menu_choice(&menu, &MenuAction::OpenUpdate));
    let menu = shared.clone();
    tray.on_about(move || menu_choice(&menu, &MenuAction::About));
    let menu = shared.clone();
    tray.on_quit(move || menu_choice(&menu, &MenuAction::Quit));
}

thread_local! {
    /// Menu last handed to the tray icon, with the language of its fixed texts; set again only on a change.
    static LAST_MENU: RefCell<Option<(Language, look::TrayMenu)>> = const { RefCell::new(None) };
    /// The answer owed to the last choice in the menu: its message also goes out as a notification, because
    /// the window may not be in front.
    static MENU_FEEDBACK: RefCell<notify::MenuFeedback> = RefCell::default();
    /// One protection against double clicks for the links of "About Celata" and the hint about a new version.
    static LINK_DEBOUNCE: RefCell<donation::Debounce> = RefCell::default();
}

/// Builds the menu from the current state and hands it to the tray icon if it changed.
fn show_tray_menu(window: &MainWindow, shared: &Shared) {
    if TRAY.with(|tray| tray.borrow().is_none()) {
        return;
    }
    let (display, view) = {
        let monitor = lock(&shared.monitor);
        (monitor.display().cloned(), monitor.view())
    };
    let (kill_switch, favorites) = {
        let store = lock(&shared.settings);
        (store.current.kill_switch, store.app.favorites.clone())
    };
    let visible = streaming::visible(&lock(&shared.streaming));
    let selected = window.get_selected_host();
    let language = i18n::current();
    let update_version = UPDATE.with(|schedule| schedule.borrow().shown().map(|version| version.to_string()));
    let menu = {
        let countries = lock(&shared.countries);
        let list = match window.get_list_state().as_str() {
            "loading" => look::ListState::Loading,
            "ready" => look::ListState::Ready,
            _ if countries.is_empty() => look::ListState::Missing,
            _ => look::ListState::Ready,
        };
        look::tray_menu(&look::TrayContext {
            display: display.as_ref(),
            view: &view,
            kill_switch,
            countries: &countries,
            streaming: &visible,
            favorites: &favorites,
            list,
            free_only: window.get_free_only(),
            selected: &selected,
            language,
            update: update_version.as_deref(),
        })
    };
    let changed = LAST_MENU.with(|last| {
        let mut last = last.borrow_mut();
        let changed = last
            .as_ref()
            .is_none_or(|(shown_language, shown)| *shown_language != language || *shown != menu);
        if changed {
            *last = Some((language, menu.clone()));
        }
        changed
    });
    if changed {
        with_tray(|tray| apply_tray_menu(tray, &menu, language));
    }
}

fn tray_place(place: &look::MenuPlace) -> TrayPlace {
    TrayPlace {
        label: place.label.as_str().into(),
        host: place.host.as_str().into(),
        enabled: place.enabled,
        current: place.current,
    }
}

fn tray_country(country: &look::MenuCountry) -> TrayCountry {
    TrayCountry {
        label: country.label.as_str().into(),
        host: country.host.as_str().into(),
        enabled: country.enabled,
        current: country.current,
        places: model(country.places.iter().map(tray_place).collect()),
    }
}

fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::from(Rc::new(VecModel::from(rows)))
}

/// Sets every text and model of the menu. The menu has no scroll position; the models are replaced whole.
fn apply_tray_menu(tray: &CelataTray, menu: &look::TrayMenu, language: Language) {
    let text = |key| SharedString::from(i18n::text(language, key));
    tray.set_show_label(text(Key::TrayShow));
    tray.set_status_label(menu.status.as_str().into());
    tray.set_main_label(menu.main.label.as_str().into());
    tray.set_main_enabled(menu.main.enabled);
    tray.set_best_label(text(Key::TrayBest));
    tray.set_best_enabled(menu.best_enabled);
    tray.set_side_shown(menu.side.is_some());
    tray.set_side_label(
        menu.side
            .as_ref()
            .map(|side| side.label.as_str())
            .unwrap_or_default()
            .into(),
    );
    tray.set_favorites_label(text(Key::TrayFavorites));
    tray.set_favorites(model(menu.favorites.iter().map(tray_place).collect()));
    tray.set_no_favorites_label(text(Key::TrayNoFavorites));
    tray.set_favorite_toggle_label(menu.favorite_toggle.label.as_str().into());
    tray.set_favorite_toggle_enabled(menu.favorite_toggle.enabled);
    tray.set_locations_label(text(Key::TrayLocations));
    tray.set_best_location_label(text(Key::BestName));
    let (note, continents) = match &menu.locations {
        look::MenuLocations::Loading => (text(Key::TrayLocationsLoading), Vec::new()),
        look::MenuLocations::Missing => (text(Key::TrayLocationsMissing), Vec::new()),
        look::MenuLocations::Ready(continents) => (SharedString::new(), continents.clone()),
    };
    tray.set_locations_note(note);
    let continents = continents
        .iter()
        .map(|continent| TrayContinent {
            label: continent.label.as_str().into(),
            slots: model(
                look::menu_slots(&continent.countries, look::MENU_SLOTS)
                    .into_iter()
                    .map(|slot| TraySlot {
                        singles: model(slot.singles.iter().map(tray_country).collect()),
                        groups: model(slot.groups.iter().map(tray_country).collect()),
                    })
                    .collect(),
            ),
        })
        .collect();
    tray.set_continents(model(continents));
    tray.set_streaming_label(text(Key::TrayStreaming));
    tray.set_streaming(model(menu.streaming.iter().map(tray_place).collect()));
    tray.set_update_label(menu.update.as_deref().unwrap_or_default().into());
    tray.set_about_label(text(Key::TrayAbout));
    tray.set_quit_label(text(Key::TrayQuit));
}

/// Carries out a choice in the menu, decided in the state at the time of the choice.
fn menu_choice(shared: &Shared, action: &MenuAction) {
    let Some(window) = shared.window.upgrade() else { return };
    let kill_switch = lock(&shared.settings).current.kill_switch;
    let confirm_open = SHOWN.with(|shown| shown.borrow().confirm.is_some());
    let selected = window.get_selected_host();
    let (decision, notice_count) = {
        let monitor = lock(&shared.monitor);
        let view = monitor.view();
        let decision = look::menu_decision(
            action,
            &look::MenuState {
                display: monitor.display(),
                view: &view,
                dialog_open: confirm_open || window.get_dialog_open() || view.unblock_asked,
                selected: &selected,
                kill_switch,
            },
        );
        (decision, monitor.notice_count())
    };
    // Choices that start something at the service, or may answer with a message, owe that answer.
    if matches!(
        decision,
        MenuDecision::Connect(_)
            | MenuDecision::StartThenConnect { .. }
            | MenuDecision::Disconnect
            | MenuDecision::Route
    ) {
        MENU_FEEDBACK.with(|feedback| feedback.borrow_mut().chosen(Instant::now(), notice_count));
        REVEALED.set(false);
    }
    match decision {
        MenuDecision::BringBack => bring_window_back(shared),
        MenuDecision::Connect(host) => {
            choose_location(&window, shared, &host);
            start_connect(shared, host);
        }
        MenuDecision::StartThenConnect { target } => {
            choose_location(&window, shared, &target);
            start_service_then_connect(shared, target);
        }
        MenuDecision::Disconnect => {
            start_disconnect(shared, false);
        }
        MenuDecision::Route => start_route(shared),
        MenuDecision::AskUnblock => {
            bring_window_back(shared);
            let mut monitor = lock(&shared.monitor);
            monitor.ask_unblock();
            let _ = push(shared, &monitor);
        }
        MenuDecision::ToggleFavorite(host) => toggle_favorite(shared, &host),
        MenuDecision::OpenUpdate => open_link(shared, update::RELEASES_URL, Key::UpdateBlockedNotice),
        MenuDecision::Nothing => {}
        MenuDecision::ShowAbout => {
            bring_window_back(shared);
            window.set_about_version(about::version_text(i18n::current()).into());
            window.set_about_open(true);
        }
        MenuDecision::Quit => quit_from_menu(shared),
    }
}

/// A location chosen in the menu becomes the selection of the window, as after a click in the list: the card
/// "Location" and the lists show it completely.
fn choose_location(window: &MainWindow, shared: &Shared, host: &str) {
    let name = if best::is_best(host) {
        tk(Key::BestName)
    } else {
        look::location_of(host, &lock(&shared.countries), i18n::current())
            .map_or_else(|| SharedString::from(host), |(_, label)| SharedString::from(label))
    };
    window.set_selected_host(host.into());
    window.set_selected_name(name);
    show_location(window, &lock(&shared.countries));
    show_lists(window, shared);
}

/// "Quit" in the menu: the same decision as closing the window with "Keep running after closing" off. If it
/// asks, the window comes back first.
fn quit_from_menu(shared: &Shared) {
    let asks = matches!(lock(&shared.monitor).close_decision(), CloseDecision::Ask(_));
    if asks {
        bring_window_back(shared);
    }
    if request_close(shared) == CloseRequestResponse::HideWindow {
        close_app(shared);
    }
}

/// Opens a fixed address in the default browser, in a thread of its own, like the donation button. With
/// blocked internet a notice; if the browser does not open, the link for copying.
fn open_link(shared: &Shared, url: &'static str, blocked_notice: Key) {
    if !LINK_DEBOUNCE.with(|debounce| debounce.borrow_mut().allow(Instant::now())) {
        return;
    }
    if donation::is_blocked(lock(&shared.monitor).display()) {
        show_toast(shared, Phrase::key(blocked_notice), false);
    }
    let failed = shared.clone();
    spawn("link", move || {
        if donation::open(url).is_err() {
            show_toast_link(&failed, Phrase::key(donation::FAILED_NOTICE), url.to_owned());
        }
    });
}

/// The icon in the system tray, if there is one.
fn with_tray(work: impl FnOnce(&CelataTray)) {
    TRAY.with(|tray| {
        if let Some(tray) = tray.borrow().as_ref() {
            work(tray);
        }
    });
}

/// A new answer about the system tray. Creates the icon as soon as the tray is there, and sets state and text
/// at once.
fn on_presence(shared: &Shared, presence: TrayPresence) {
    PRESENCE.set(presence);
    if presence == TrayPresence::Shown {
        ABSENT_SINCE.set(None);
    } else if ABSENT_SINCE.get().is_none() {
        ABSENT_SINCE.set(Some(Instant::now()));
    }
    if let Some(window) = shared.window.upgrade() {
        window.set_tray_shown(presence == TrayPresence::Shown);
    }
    if presence.tray_available()
        && TRAY.with(|tray| tray.borrow().is_none())
        && let Some(tray) = create_tray(shared)
    {
        TRAY.with(|slot| *slot.borrow_mut() = Some(tray));
        // Set colour, text and menu anew, also if they did not change since the last time.
        STATE_ICON.with(|shown| *shown.borrow_mut() = None);
        LAST_MENU.with(|last| *last.borrow_mut() = None);
        let monitor = lock(&shared.monitor);
        let view = monitor.view();
        let state_color = look::state_color(monitor.display());
        drop(monitor);
        if let Some(window) = shared.window.upgrade() {
            show_state_icon(shared, &window, &view, state_color);
            show_tray_menu(&window, shared);
        }
    }
}

/// Starts the thread that watches the system tray.
fn start_presence(shared: &Shared) {
    let shared = shared.clone();
    let started = presence::spawn(move |presence| {
        let shared = shared.clone();
        slint::invoke_from_event_loop(move || on_presence(&shared, presence)).is_ok()
    });
    if started.is_err() {
        report(Phrase::fill(Key::TerminalThreadFailed, [Phrase::raw("presence")]));
    }
}

/// The logo in the colour of the state, as a pixel image for the system tray and the window. Drawn once
/// per colour; only use it in the window thread (`slint::Image` is not `Send`).
fn logo(state: StateColor) -> Option<Image> {
    LOGOS.with(|logos| {
        logos
            .borrow_mut()
            .entry(state.as_str())
            .or_insert_with(|| trayicon::render(state).map(Image::from_rgba8))
            .clone()
    })
}

thread_local! {
    static LOGOS: RefCell<HashMap<&'static str, Option<Image>>> = RefCell::default();
    /// Colour and hover text last handed to the tray icon and the window, so that they are only set again
    /// when they change and not every second.
    static STATE_ICON: RefCell<Option<(StateColor, String)>> = const { RefCell::new(None) };
}

/// Sets the logo in the colour of the state in the system tray and as window icon, and the hover text of
/// the tray icon. Under Wayland the window icon has no effect yet.
fn show_state_icon(shared: &Shared, window: &MainWindow, view: &StatusView, state_color: StateColor) {
    let language = i18n::current();
    let short = trayicon::short_state(view, &lock(&shared.countries), language);
    let tooltip = trayicon::tooltip(language, &short);
    let (color_changed, text_changed) = STATE_ICON.with(|shown| {
        let mut shown = shown.borrow_mut();
        let changed = match shown.as_ref() {
            Some((color, text)) => (*color != state_color, *text != tooltip),
            None => (true, true),
        };
        *shown = Some((state_color, tooltip.clone()));
        changed
    });
    if color_changed && let Some(image) = logo(state_color) {
        window.set_state_icon(image.clone());
        with_tray(|tray| tray.set_state_icon(image));
    }
    if text_changed {
        with_tray(|tray| tray.set_state_tooltip(tooltip.into()));
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

/// Language of the settings file for messages in the terminal; English without a file.
fn stored_language() -> Language {
    settings::load(settings::config_path().as_deref()).app.language
}

fn main() -> ExitCode {
    // Options answer before anything else happens: no desktop entry, no window, no icon, no name on the bus.
    // `args_os`: an argument that is not valid Unicode must not abort the start.
    let command = cli::parse(std::env::args_os().map(|argument| argument.to_string_lossy().into_owned()));
    let autostart = match command {
        cli::Command::Gui { autostart } => autostart,
        cli::Command::Version => {
            println!("{}", cli::version_line());
            return ExitCode::SUCCESS;
        }
        cli::Command::Help => {
            println!("{}", cli::help_text(stored_language()));
            return ExitCode::SUCCESS;
        }
        // The rule for polkit for "Fix permanently": only printed, no window, no bus, no service.
        cli::Command::PolkitRule => {
            print!("{}", unit::RULE);
            return ExitCode::SUCCESS;
        }
        // "Unblock internet" without the window: no desktop entry, no icon, no name on the bus.
        cli::Command::Unblock => {
            let outcome = celata::unblock::unblock(
                &Client::default(),
                killswitch::is_blocked,
                celata::unblock::Wait::default(),
            );
            let text = celata::unblock::message(&outcome, stored_language());
            if celata::unblock::is_success(&outcome) {
                println!("{text}");
            } else {
                eprintln!("{text}");
            }
            return ExitCode::from(celata::unblock::exit_code(&outcome));
        }
        cli::Command::Unknown(argument) => {
            eprintln!("{}", cli::unknown_text(stored_language(), &argument));
            return ExitCode::from(cli::EXIT_UNKNOWN);
        }
    };
    // Load the settings before the window, so that even the first picture appears in the stored
    // language.
    let settings_path = settings::config_path();
    let applied_path = settings::applied_path();
    let loaded = settings::load(settings_path.as_deref());
    let language = loaded.app.language;
    i18n::set_current(language);
    // Only one running window: a second start brings the first one back and ends. The connection keeps the
    // name for the whole run.
    let _instance = match instance::claim(|| {
        let _ = slint::invoke_from_event_loop(|| {
            ACTIVATE.with(|shared| {
                if let Some(shared) = shared.borrow().as_ref() {
                    bring_window_back(shared);
                }
            });
        });
    }) {
        Claim::Other => return ExitCode::SUCCESS,
        Claim::Owner(connection) => Some(connection),
        Claim::NoBus => None,
    };
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
        notifier: notify::spawn().ok(),
        last_shown: Arc::new(Mutex::new(notify::Shown::default())),
        session: Arc::new(Mutex::new(Session::default())),
    };
    ACTIVATE.with(|activate| *activate.borrow_mut() = Some(shared.clone()));
    // On every way of starting, also at login without the window: start the service if it does not run. Celata
    // never connects by itself afterwards.
    if lock(&shared.monitor).begin_startup_check() {
        start_service_check(&shared);
    }
    LISTS.with(|lists| {
        let lists = lists.borrow();
        window.set_servers(ModelRc::from(lists.servers.clone()));
        window.set_streaming_servers(ModelRc::from(lists.streaming.clone()));
        window.set_favorites(ModelRc::from(lists.favorites.clone()));
    });
    connect_settings(&window, &shared);
    connect_donation(&window, &shared);
    connect_appearance(&window, &shared);
    connect_language(&window, &shared);
    connect_application(&window, &shared);
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
    window.on_search_changed(move |_text| {
        if let Some(window) = search.window.upgrade() {
            show_lists(&window, &search);
        }
    });
    // The location of the overview follows the selection in the list or in "Favorite locations"; the list
    // shows a selection inside a folded country at the country.
    let selection = shared.clone();
    window.on_selection_changed(move |_host| {
        if let Some(window) = selection.window.upgrade() {
            show_location(&window, &lock(&selection.countries));
            show_lists(&window, &selection);
        }
    });
    let reload = shared.clone();
    window.on_reload_servers(move || load_servers(&reload));
    let free = shared.clone();
    window.on_free_only_changed(move |_| {
        if let Some(window) = free.window.upgrade() {
            show_lists(&window, &free);
        }
    });
    connect_locations(&window, &shared);
    let connect = shared.clone();
    window.on_connect(move |host| connect_or_start(&connect, &host));
    // The command for "Fix permanently" names the running program; the path is shown only in the status card.
    window.set_fix_rule_command(unit::rule_command(desktop::program_path().ok().as_deref()).into());
    let start_service = shared.clone();
    window.on_start_service_clicked(move || start_service_interactive(&start_service));
    let guide = shared.clone();
    window.on_setup_guide_clicked(move || open_link(&guide, about::SETUP_GUIDE_URL, Key::SetupGuideBlockedNotice));
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
    window.window().on_close_requested(move || {
        let keep_running = lock(&close.settings).app.keep_running;
        let decision = lock(&close.monitor).close_decision();
        if background::on_close(keep_running, PRESENCE.get(), decision) == CloseOutcome::Hide {
            // Only the window goes; Celata, the connection and the icon stay. Nothing goes to the service.
            WINDOW_HIDDEN.set(true);
            return CloseRequestResponse::HideWindow;
        }
        let response = request_close(&close);
        if response == CloseRequestResponse::HideWindow {
            close_app(&close);
        }
        response
    });
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
            close_app(&close_disconnect);
        }
    });
    // "Stay connected and close" does not call the service.
    let close_keep = shared.clone();
    window.on_close_keep(move || close_app(&close_keep));

    // "Best location" is preselected; `apply_language` sets its name.
    window.set_selected_host(best::BEST.into());
    // Set all texts in the stored language before the window appears. The streaming list also appears
    // complete at once, without waiting for the network or the server list.
    apply_language(&window, &shared, language);
    start_observer(&shared);
    load_servers(&shared);
    check_streaming(&shared);
    start_presence(&shared);

    // The event loop runs until `close_app`: the window may be hidden while Celata keeps running. Started at
    // login with "Keep running after closing", the window appears only if the icon does not within 30 seconds.
    let keep_running = lock(&shared.settings).app.keep_running;
    let shown = match background::start_mode(autostart, keep_running) {
        StartMode::Window => window.show(),
        StartMode::TrayOnly { wait } => {
            WINDOW_HIDDEN.set(true);
            START_DEADLINE.set(Some(Instant::now() + wait));
            Ok(())
        }
    };
    let restore_timer = slint::Timer::default();
    let restore = shared.clone();
    restore_timer.start(slint::TimerMode::Repeated, Duration::from_secs(1), move || {
        check_window_reachable(&restore);
        check_update(&restore);
    });
    let result = shown.and_then(|()| slint::run_event_loop_until_quit());
    match result {
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
    push_with(shared, monitor, false)
}

/// Like `push`, and brings a hidden window back once if a start of the service ended with a reason. Only where a
/// start can end: the observation and the thread "service-start".
fn push_revealing(shared: &Shared, monitor: &mut Monitor) -> Result<(), slint::EventLoopError> {
    let reveal = monitor.take_reveal();
    push_with(shared, monitor, reveal)
}

fn push_with(shared: &Shared, monitor: &Monitor, reveal: bool) -> Result<(), slint::EventLoopError> {
    let view = monitor.view();
    let next = notify::Shown {
        display: monitor.display().cloned(),
        connect_target: view.connect_target.clone(),
    };
    let event = {
        let mut last = lock(&shared.last_shown);
        let event = notify::event(&last, &next);
        *last = next;
        event
    };
    let tone = look::tone(monitor.display()).as_str();
    let state_color = look::state_color(monitor.display());
    let button = look::main_button(monitor.display(), &view);
    let connected = matches!(monitor.display(), Some(DisplayState::Connected { .. }));
    let notice_count = monitor.notice_count();
    let shared = shared.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(window) = shared.window.upgrade() {
            show(&window, &view, tone, button, state_color, &lock(&shared.countries));
            show_state_icon(&shared, &window, &view, state_color);
            show_location(&window, &lock(&shared.countries));
            show_tray_menu(&window, &shared);
            // "Connected" in the lists follows the connected server; built anew only when it changes.
            let connected_host = window.get_connected_host();
            if LISTS.with(|lists| lists.borrow().connected != connected_host) {
                show_lists(&window, &shared);
            }
            show_settings_state(&window, &lock(&shared.settings), connected);
            if reveal {
                bring_message_back(&shared);
                REVEALED.set(true);
            }
            let notice = view.notice.as_ref().map(|notice| &notice.text);
            notify_change(&shared, &window, event, notice, notice_count);
        }
    })
}

thread_local! {
    static DEDUP: RefCell<notify::Dedup> = RefCell::default();
}

/// Sends a notification for a change while the window is not in front, and the message of the status card that
/// answers a choice in the menu (also when it has the text of an earlier one).
fn notify_change(
    shared: &Shared,
    window: &MainWindow,
    event: Option<notify::Event>,
    notice: Option<&Phrase>,
    notice_count: u64,
) {
    let notifications = lock(&shared.settings).app.notifications;
    let front = window_in_front(window);
    let delivery = notify::Delivery {
        notifications,
        front,
        revealed: REVEALED.get(),
    };
    let answer = MENU_FEEDBACK.with(|feedback| {
        feedback
            .borrow_mut()
            .settle(notice, notice_count, event.is_some(), delivery, Instant::now())
    });
    let show = notify::should_show(notifications, front);
    let send = |event: &notify::Event| {
        if show && let Some(notifier) = &shared.notifier {
            let body = notify::body(event, i18n::current(), &lock(&shared.countries));
            let _ = notifier.send(body);
        }
    };
    if let Some(event) = event
        && DEDUP.with(|dedup| dedup.borrow_mut().admit(&event))
    {
        send(&event);
    }
    match answer {
        // The answer to a choice goes out even if the same text went out before; it counts as the last event.
        Some(notify::Feedback::Notify(answer)) => {
            DEDUP.with(|dedup| dedup.borrow_mut().admit(&answer));
            send(&answer);
        }
        // Without notifications the window comes back with the message.
        Some(notify::Feedback::Reveal) => bring_message_back(shared),
        None => {}
    }
}

thread_local! {
    /// The window came back since the last choice in the menu because a start of the service ended with a reason;
    /// the answer to the choice is then in the window already.
    static REVEALED: Cell<bool> = const { Cell::new(false) };
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
            if changed && push_revealing(&shared, &mut monitor).is_err() {
                return Flow::Stop;
            }
            connect_after_start(&shared, &mut monitor);
            flow
        });
    });
}

/// Every wish to connect from the window (main button, double click in a list or in "Favorite locations"): with
/// an answering service as before, with an unreachable one Celata starts the service first and connects after
/// its first answer.
fn connect_or_start(shared: &Shared, target: &str) {
    let wish = {
        let monitor = lock(&shared.monitor);
        look::connect_wish(monitor.display(), &monitor.view(), target)
    };
    match wish {
        ConnectWish::Connect(target) => start_connect(shared, target),
        ConnectWish::StartThenConnect(target) => start_service_then_connect(shared, target),
        ConnectWish::Nothing => {}
    }
}

/// Starts the service for a wish to connect; `target` ("Best location" as the mark) is resolved only after the
/// first answer of the service.
fn start_service_then_connect(shared: &Shared, target: String) {
    let started = {
        let mut monitor = lock(&shared.monitor);
        let started = monitor.request_start(Some(target));
        if started {
            let _ = push(shared, &monitor);
        }
        started
    };
    if started {
        start_service_check(shared);
    }
}

/// After the first answer of a service Celata started for a wish to connect: connect to its target, in the window
/// thread like a click.
fn connect_after_start(shared: &Shared, monitor: &mut Monitor) {
    if let Some(target) = monitor.take_connect_after_start() {
        let shared = shared.clone();
        let _ = slint::invoke_from_event_loop(move || start_connect(&shared, target));
    }
}

/// Thread "service-start": reads the unit and the group, starts the unit without a password prompt if that is the
/// decision, and follows the start. The monitor allows only one start at a time.
fn start_service_check(shared: &Shared) {
    let shared = shared.clone();
    spawn("service-start", move || {
        let info = unit::read_unit(state::SERVICE_UNIT);
        let (status, groups) = unit::read_group_inputs();
        let group = unit::group_check(&status, &groups, unit::GROUP);
        let before = unit::before_start(info.as_ref(), group);
        let start = {
            let mut monitor = lock(&shared.monitor);
            let start = monitor.startup_decided(before, group);
            let _ = push_revealing(&shared, &mut monitor);
            start
        };
        if start {
            let result = unit::classify(&unit::start_unit(state::SERVICE_UNIT));
            let mut monitor = lock(&shared.monitor);
            monitor.start_answered(result);
            let _ = push_revealing(&shared, &mut monitor);
        }
        follow_start(&shared);
    });
}

/// The button "Start service": the only start that lets the system ask for the password, through the agent of the
/// desktop.
fn start_service_interactive(shared: &Shared) {
    {
        let mut monitor = lock(&shared.monitor);
        if !monitor.begin_interactive_start() {
            return;
        }
        let _ = push(shared, &monitor);
    }
    let shared = shared.clone();
    spawn("service-start", move || {
        let result = unit::classify_interactive(&unit::start_unit_interactive(state::SERVICE_UNIT));
        {
            let mut monitor = lock(&shared.monitor);
            monitor.interactive_answered(result);
            let _ = push_revealing(&shared, &mut monitor);
        }
        follow_start(&shared);
    });
}

/// Reads the state of the unit every second while the start this thread follows runs, until the service answers,
/// the unit failed or the waiting time is over. Never starts anything itself. A reading for a start that ended in
/// the meantime has no effect on a newer one, which its own thread follows.
fn follow_start(shared: &Shared) {
    // A statement of its own, so that the lock is released before the next one.
    let followed = lock(&shared.monitor).start_since();
    let Some(since) = followed else { return };
    loop {
        thread::sleep(Duration::from_secs(1));
        let progress = unit::poll(unit::read_unit(state::SERVICE_UNIT).as_ref(), since.elapsed());
        let mut monitor = lock(&shared.monitor);
        let running = monitor.start_progress_for(since, progress);
        if push_revealing(shared, &mut monitor).is_err() || !running {
            return;
        }
    }
}

/// Connects in the background, or switches the server while a connection exists. A second click during
/// an operation does nothing. If the routes have to be rebuilt from "Internet blocked", Celata asks
/// first, because the internet is free for a moment while doing so.
///
/// "Best location" (`best::BEST`, also an empty selection) is resolved here and only here into a host of the
/// list: time zone or system language of this computer and the loaded list, no request.
/// The mark itself never reaches the service.
fn start_connect(shared: &Shared, host: String) {
    let pick = if best::is_best(&host) {
        let origin = zone::system_origin();
        match pick_best(shared, origin.as_ref()) {
            Some(pick) => pick,
            None => return,
        }
    } else {
        Pick::own(host)
    };
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
                Awaiting::Connect(pick),
                Phrase::key(Key::ConfirmRebuildTitle),
                Phrase::key(Key::ConfirmRebuildText),
                Phrase::key(Key::ConfirmRebuildOk),
            );
            return;
        }
    }
    begin_connect(shared, pick, false);
}

/// Carries out the decision of `best::choose` for "Best location": a notice without any call, or the host
/// with the further candidates. Only in the window thread.
fn pick_best(shared: &Shared, origin: Option<&best::Origin>) -> Option<Pick> {
    let window = shared.window.upgrade()?;
    let ranked = {
        let countries = lock(&shared.countries);
        let session = lock(&shared.session);
        best::rank(
            &countries,
            origin,
            window.get_list_state() == "loading",
            window.get_free_only(),
            session.best.excluded(),
        )
    };
    let connected = lock(&shared.monitor).view().connected_host;
    let connected = (!connected.is_empty()).then_some(connected.as_str());
    match best::choose(ranked, connected) {
        Choice::Inform(key) => {
            inform(shared, key);
            None
        }
        Choice::Already(host) => {
            if inform(shared, Key::NoticeBestAlready) {
                lock(&shared.session).best.started(true, &host);
                show_lists(&window, shared);
            }
            None
        }
        Choice::Go { host, rest } => Some(Pick { host, best: Some(rest) }),
    }
}

/// Shows a notice in the status card without starting anything; nothing goes to the service. Returns
/// whether it is shown (not while an operation is running).
fn inform(shared: &Shared, key: Key) -> bool {
    let mut monitor = lock(&shared.monitor);
    let shown = monitor.inform(Phrase::key(key));
    if shown {
        let _ = push(shared, &monitor);
    }
    shown
}

/// Starts connecting. `confirmed`: the question about the rebuild was confirmed.
fn begin_connect(shared: &Shared, pick: Pick, confirmed: bool) {
    let start = {
        let mut monitor = lock(&shared.monitor);
        let Some(start) = monitor.begin_connect(&pick.host) else {
            return;
        };
        let _ = push(shared, &monitor);
        start
    };
    // "Chosen: …" belongs to "Best location" only; an own choice removes it.
    lock(&shared.session).best.started(pick.via_best(), &pick.host);
    let was_free = match start {
        Start::Connect { was_free } => was_free,
        Start::Switch { from } => return start_switch(shared, from, pick),
    };
    let shared = shared.clone();
    spawn("connect", move || {
        let host = pick.host.clone();
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
        if result.name_failed() {
            // Without a check of names beforehand: the next attempt takes the next candidate.
            if pick.via_best() {
                lock(&shared.session).best.failed(&host);
            }
            forget_derived(&shared, &host);
        }
        refresh(&shared);
    });
}

/// Target of a switch through "Best location": the first candidate with an address, at most
/// `best::MAX_TRIES` names. Candidates without an address leave "Best location" for this session.
/// Returns the target, or `None` if the switch already ended here.
fn best_switch_target(shared: &Shared, from: &str, pick: &Pick, rest: &[String]) -> Option<String> {
    let mut hosts = vec![pick.host.clone()];
    hosts.extend(rest.iter().cloned());
    let (found, without_address) = switch::first_reachable(&hosts, streaming::resolves);
    {
        let mut session = lock(&shared.session);
        for host in &without_address {
            session.best.failed(host);
        }
    }
    let Some(found) = found else {
        // As for every switch to a name without an address; the connection stays.
        let mut monitor = lock(&shared.monitor);
        monitor.finish_switch(&SwitchResult::Unreachable);
        let _ = push(shared, &monitor);
        return None;
    };
    if found == pick.host {
        return Some(found);
    }
    match best::choose(Ok(vec![found]), Some(from)) {
        Choice::Go { host, .. } => {
            let mut monitor = lock(&shared.monitor);
            if monitor.retarget(&host) {
                let _ = push(shared, &monitor);
                drop(monitor);
                lock(&shared.session).best.started(true, &host);
            }
            Some(host)
        }
        // The connected server is the best reachable one after all: no call to the service.
        Choice::Already(_) | Choice::Inform(_) => {
            let mut monitor = lock(&shared.monitor);
            let ended = monitor.abandon_switch(Phrase::key(Key::NoticeBestAlready));
            let _ = push(shared, &monitor);
            drop(monitor);
            if ended {
                lock(&shared.session).best.started(true, from);
                return None;
            }
            // "Disconnect" was requested meanwhile: the switch thread carries it out.
            Some(from.to_owned())
        }
    }
}

/// Switches from `from` to `to` in the background. The same thread also carries out a requested
/// "Disconnect", so that no `/connect` follows after `/destroy`.
fn start_switch(shared: &Shared, from: String, pick: Pick) {
    let shared = shared.clone();
    spawn("switch", move || {
        // Through "Best location" the target is the first candidate with an address; that name is not queried
        // a second time.
        let (to, checked) = match &pick.best {
            None => (pick.host.clone(), None),
            Some(rest) => match best_switch_target(&shared, &from, &pick, rest) {
                Some(found) => (found.clone(), Some(found)),
                None => {
                    refresh_lists(&shared);
                    refresh(&shared);
                    return;
                }
            },
        };
        let resolves = |host: &str| checked.as_deref() == Some(host) || streaming::resolves(host);
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
            resolves,
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
        if result.name_failed() {
            if pick.via_best() {
                lock(&shared.session).best.failed(&to);
            }
            forget_derived(&shared, &to);
        }
        refresh_lists(&shared);
        refresh(&shared);
    });
}

/// A connection or switch with `host` failed. If it is a country name built by Celata, hide.me does not
/// know it: it is left out until the end of the session (also after loading the list again), and the
/// country only folds and unfolds. The usual message about the failure stays as it is. Other hosts are not
/// touched.
fn forget_derived(shared: &Shared, host: &str) {
    {
        let mut countries = lock(&shared.countries);
        if !servers::is_derived(&countries, host) {
            return;
        }
        let mut session = lock(&shared.session);
        session.failed_derived.insert(host.to_owned());
        servers::drop_derived(&mut countries, &session.failed_derived);
    }
    let shared = shared.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(window) = shared.window.upgrade() {
            show_lists(&window, &shared);
            show_location(&window, &lock(&shared.countries));
        }
    });
}

/// Builds the location lists anew in the window thread, for example after "Best location" chose another host.
fn refresh_lists(shared: &Shared) {
    let shared = shared.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(window) = shared.window.upgrade() {
            show_lists(&window, &shared);
        }
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
    let _ = push_revealing(shared, &mut monitor);
    connect_after_start(shared, &mut monitor);
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
        let shared = shared.clone();
        let _ = slint::invoke_from_event_loop(move || close_app(&shared));
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

/// Ends Celata: hides the window and the icon in the system tray and stops the event loop. Every way of
/// closing goes through here, because a visible tray icon would otherwise keep the event loop running and
/// Celata would go on invisibly.
fn close_app(shared: &Shared) {
    if let Some(window) = shared.window.upgrade() {
        let _ = window.hide();
    }
    with_tray(|tray| {
        let _ = tray.hide();
    });
    let _ = slint::quit_event_loop();
}

/// Click on the icon in the system tray: shows the window again and restores it if it is minimised.
/// Nothing else; the icon never connects, disconnects or blocks.
fn bring_window_back(shared: &Shared) {
    WINDOW_HIDDEN.set(false);
    START_DEADLINE.set(None);
    if let Some(window) = shared.window.upgrade() {
        let _ = window.show();
        window.window().set_minimized(false);
        // After a start without the window, Slint draws the first picture of the newly created window only with the
        // next event (a mouse movement, for example); until then the desktop shows nothing. Ask for it at once.
        window.window().request_redraw();
    }
}

/// Brings the window back for a message of the status card (a failed start of the service, or the answer to a
/// choice in the menu without notifications) and shows the overview, where the message stands.
fn bring_message_back(shared: &Shared) {
    bring_window_back(shared);
    if let Some(window) = shared.window.upgrade() {
        window.set_page("overview".into());
    }
}

thread_local! {
    /// Celata hid the window (closed with "Keep running after closing", or started at login). Only used in the
    /// window thread.
    static WINDOW_HIDDEN: Cell<bool> = const { Cell::new(false) };
    /// Start at login without the window: until then the icon has to appear, otherwise the window comes.
    static START_DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
    /// Since when the icon has not been shown; `None` while it is.
    static ABSENT_SINCE: Cell<Option<Instant>> = Cell::new(Some(Instant::now()));
    /// For `Activate()` of a second start, which arrives before `main` could hand anything over otherwise.
    static ACTIVATE: RefCell<Option<Shared>> = const { RefCell::new(None) };
}

/// Celata must never run on unreachable: a hidden window comes back when the icon is missing for 10 seconds,
/// or after a start at login when it has not appeared within 30 seconds.
fn check_window_reachable(shared: &Shared) {
    let now = Instant::now();
    if let Some(deadline) = START_DEADLINE.get() {
        if PRESENCE.get() == TrayPresence::Shown {
            START_DEADLINE.set(None);
        } else if now >= deadline {
            bring_window_back(shared);
        }
        return;
    }
    if background::should_restore(WINDOW_HIDDEN.get(), ABSENT_SINCE.get(), now) {
        bring_window_back(shared);
    }
}

/// The window can be seen and has the focus. Without access to the winit window it counts as not in front, so
/// that a notification rather appears once too often.
fn window_in_front(window: &MainWindow) -> bool {
    use slint::winit_030::WinitWindowAccessor;
    !WINDOW_HIDDEN.get()
        && window.window().is_visible()
        && !window.window().is_minimized()
        && window
            .window()
            .with_winit_window(slint::winit_030::winit::window::Window::has_focus)
            == Some(true)
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
        // Standard favourites show "Loading …" at once, also when loading again after a failure.
        show_lists(&window, shared);
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
                    let mut countries = loaded.countries;
                    // Built country names that failed in this session stay away.
                    servers::drop_derived(&mut countries, &lock(&shared.session).failed_derived);
                    *lock(&shared.countries) = countries;
                }
                Err(error) => {
                    set_list_message(&window, Some(error.phrase()));
                    window.set_list_state("error".into());
                }
            }
            show_lists(&window, &shared);
            show_location(&window, &lock(&shared.countries));
            // Titles such as "Connecting to Germany, Berlin (…) …" name the location only with the list.
            let _ = push(&shared, &lock(&shared.monitor));
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
fn location_item(row: &look::LocationItem) -> LocationItem {
    LocationItem {
        kind: row.kind.as_str().into(),
        flag: if row.code.is_empty() {
            Image::default()
        } else {
            flag(&row.code)
        },
        country: row.country.as_str().into(),
        place: row.place.as_str().into(),
        host: row.host.as_str().into(),
        star_host: row.star_host.as_str().into(),
        free: row.free,
        choice: row.choice.as_str().into(),
        favorite: row.favorite,
        expandable: row.expandable,
        expanded: row.expanded,
        holds_selected: row.holds_selected,
        holds_connected: row.holds_connected,
        code: row.code.as_str().into(),
    }
}

/// Converts a row of "Favorite locations" into the type of the user interface.
fn favorite_item(row: &look::FavoriteItem) -> FavoriteItem {
    FavoriteItem {
        flag: if row.code.is_empty() {
            Image::default()
        } else {
            flag(&row.code)
        },
        label: row.label.as_str().into(),
        host: row.host.as_str().into(),
        star_host: row.star_host.as_str().into(),
        state: row.state.as_str().into(),
        streaming: row.streaming,
        connected: row.connected,
        choice: row.choice.as_str().into(),
        best: row.best,
    }
}

/// Replaces in `model` only the range in which `new` differs from the rows shown so far (`look::splice`), so
/// that the list keeps its scroll position. Returns that range, so that the caller can keep the visible rows
/// in place (`look::scroll_after`).
fn replace_rows<T: PartialEq, U: Clone + 'static>(
    model: &VecModel<U>,
    shown: &mut Vec<T>,
    new: Vec<T>,
    convert: impl Fn(&T) -> U,
) -> Option<(usize, usize, usize)> {
    let range = look::splice(shown, &new)?;
    let (start, removed, inserted) = range;
    let same = removed.min(inserted);
    for (index, row) in new.iter().enumerate().skip(start).take(same) {
        model.set_row_data(index, convert(row));
    }
    if removed > inserted {
        for _ in inserted..removed {
            model.remove(start + inserted);
        }
    } else {
        for (index, row) in new.iter().enumerate().skip(start + removed).take(inserted - removed) {
            model.insert(index, convert(row));
        }
    }
    *shown = new;
    Some(range)
}

/// Builds the tabs "Standard" and "Streaming" of the page "Locations" and "Favorite locations" of the
/// overview anew from the current state: search, "Free only", unfolded countries, selection, connected
/// server and favourites.
fn show_lists(window: &MainWindow, shared: &Shared) {
    let language = i18n::current();
    let query = window.get_search_text();
    let selected = window.get_selected_host();
    let connected_host = window.get_connected_host();
    let connected = (!connected_host.is_empty()).then_some(connected_host.as_str());
    let loading = window.get_list_state() == "loading";
    let favorites = lock(&shared.settings).app.favorites.clone();
    let (expanded, best_host) = {
        let session = lock(&shared.session);
        (session.expanded.clone(), session.best.host().map(str::to_owned))
    };
    let visible = streaming::visible(&lock(&shared.streaming));
    let countries = lock(&shared.countries);
    let ctx = look::ListContext {
        countries: &countries,
        language,
        query: &query,
        free_only: window.get_free_only(),
        expanded: &expanded,
        selected: &selected,
        connected,
        favorites: &favorites,
        loading,
        best_host: best_host.as_deref(),
    };
    let standard = look::location_items(&ctx);
    let streaming_rows = look::streaming_items(&visible, &ctx);
    let favorite_rows = look::favorite_items(&favorites, &countries, &visible, loading, connected, language);
    drop(countries);
    LISTS.with(|lists| {
        let mut lists = lists.borrow_mut();
        let lists = &mut *lists;
        let changed = replace_rows(&lists.servers, &mut lists.shown_servers, standard, location_item);
        window.set_servers_scroll(look::scroll_after(changed, window.get_servers_scroll()));
        let changed = replace_rows(
            &lists.streaming,
            &mut lists.shown_streaming,
            streaming_rows,
            location_item,
        );
        window.set_streaming_scroll(look::scroll_after(changed, window.get_streaming_scroll()));
        // "Favorite locations" is short and sits in the overview; there the position follows the list view.
        replace_rows(
            &lists.favorites,
            &mut lists.shown_favorites,
            favorite_rows,
            favorite_item,
        );
        lists.connected = connected_host.clone();
    });
    // Favourites, "Free only", the list and the selection are part of the menu as well.
    show_tray_menu(window, shared);
}

/// Connects star and arrow. Neither of them selects, connects or calls the service.
fn connect_locations(window: &MainWindow, shared: &Shared) {
    // Star: save at once like the donation hint, not through `handle_edit`: no validation, no question, no 2
    // second period, nothing to the service, no reconnect.
    let star = shared.clone();
    window.on_toggle_favorite(move |host| toggle_favorite(&star, &host));
    // Arrow: unfolded countries are only remembered for this session.
    let arrow = shared.clone();
    window.on_toggle_country(move |code| {
        let Some(window) = arrow.window.upgrade() else { return };
        {
            let mut session = lock(&arrow.session);
            if !session.expanded.remove(code.as_str()) {
                session.expanded.insert(code.to_string());
            }
        }
        show_lists(&window, &arrow);
    });
}

/// Star in the window or entry in the menu: save at once, refresh lists, "Favorite locations" and menu.
fn toggle_favorite(shared: &Shared, host: &str) {
    let Some(window) = shared.window.upgrade() else { return };
    let mut store = lock(&shared.settings);
    favorites::toggle(&mut store.app.favorites, host);
    if let Some(path) = &store.settings_path {
        match settings::save(path, &store.current, &store.kept, &store.app) {
            Ok(()) => set_file_notice(&window, None),
            Err(error) => set_file_notice(&window, Some(save_failed(&error))),
        }
    }
    drop(store);
    // The display follows the star even if saving fails.
    show_lists(&window, shared);
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
                show_lists(&window, &shared);
            }
        });
    });
}

/// Sets the location of the overview by `look::overview_location` from the current values of the window.
fn show_location(window: &MainWindow, countries: &[Country]) {
    let target = CONNECT_TARGET.with(|target| target.borrow().clone());
    let location = look::overview_location(
        &window.get_connected_host(),
        target.as_deref(),
        &window.get_selected_host(),
        &window.get_selected_name(),
        countries,
        i18n::current(),
    );
    window.set_location_label(location.label.into());
    window.set_location_flag(location.code.as_deref().map(flag).unwrap_or_default());
    window.set_location_best(location.best);
}

/// Transfers the display into the window.
fn show(
    window: &MainWindow,
    view: &StatusView,
    tone: &str,
    button: MainButton,
    state_color: StateColor,
    countries: &[Country],
) {
    let language = i18n::current();
    CONNECT_TARGET.with(|target| target.borrow_mut().clone_from(&view.connect_target));
    // During a switch or an own connection attempt: "Switching to Germany, Berlin (berlin.hideservers.net) …",
    // "Connecting to … …" or "Returning to … …"; the target stands for `{0}` in the title.
    let target = view.switch_target.as_ref().or(view.connect_target.as_ref());
    let title = match target {
        Some(target) => Phrase::fill(
            view.message.title,
            [Phrase::raw(look::target_label(target, countries, language))],
        ),
        None => Phrase::key(view.message.title),
    };
    window.set_status_title(tr(&title));
    // Sidebar: "Switching to Germany, Berlin …" without the host name in brackets, so that the whole state
    // stays readable there.
    let short = match target {
        Some(target) => Phrase::fill(
            view.message.title,
            [Phrase::raw(look::target_short(target, countries, language))],
        ),
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
    window.set_state_color(state_color.as_str().into());
    window.set_main_label(tk(button.label));
    window.set_main_action(button.action.as_str().into());
    window.set_main_side(button.side.as_str().into());
    window.set_main_side_label(button.side_label.map(tk).unwrap_or_default());
    // Connecting is also possible while the service is unreachable, if connecting starts it first.
    window.set_can_connect(view.can_connect || view.start_on_connect);
    window.set_start_button_visible(view.start_button != StartButton::Hidden);
    window.set_start_button_busy(view.start_button == StartButton::Busy);
    window.set_fix_rule_visible(view.fix_rule);
    window.set_guide_link_visible(view.guide_link);
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
        // A question started from the menu must be seen: bring a hidden or minimised window back first.
        if !window.window().is_visible() || window.window().is_minimized() {
            bring_window_back(shared);
        }
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

/// Builds the menu anew once the running callback has released its locks (the setting "Kill switch" decides
/// whether the menu offers "Block internet").
fn refresh_tray_menu_later(shared: &Shared) {
    let shared = shared.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(window) = shared.window.upgrade() {
            show_tray_menu(&window, &shared);
        }
    });
}

/// Checks a change and saves it.
fn handle_edit(shared: &Shared, edit: Edit) {
    refresh_tray_menu_later(shared);
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
        refresh_tray_menu_later(&confirm);
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
            Some(Awaiting::Connect(pick)) => begin_connect(&confirm, pick, true),
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
    let state_color = look::state_color(monitor.display());
    show(
        window,
        &view,
        look::tone(monitor.display()).as_str(),
        look::main_button(monitor.display(), &view),
        state_color,
        &lock(&shared.countries),
    );
    drop(monitor);
    // "Best location" stays selected under its name in the new language.
    if best::is_best(&window.get_selected_host()) {
        window.set_selected_host(best::BEST.into());
        window.set_selected_name(tk(Key::BestName));
    }
    show_state_icon(shared, window, &view, state_color);
    show_fixed_texts(window);
    show_lists(window, shared);
    show_location(window, &lock(&shared.countries));
    show_settings(window, &lock(&shared.settings), connected);
    show_close_dialog(window);
    show_confirm(window);
    window.set_about_version(about::version_text(i18n::current()).into());
    show_autostart(window);
    show_update(window);
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
    // "Project page" in "About Celata".
    let project = shared.clone();
    window.on_open_project(move || open_link(&project, about::PROJECT_URL, Key::DonationBlockedNotice));
}

/// Sets the stored appearance (light or dark) before the window is shown, also for the hidden start and for
/// bringing the window back later, and saves the choice of the button in the header. The window holds the
/// appearance itself (`set-appearance` is the only place that sets it); Rust never reads it back. Saving only:
/// no service, no network, no reconnect. If saving fails, the appearance stays for this session and the usual
/// notice about the file appears.
fn connect_appearance(window: &MainWindow, shared: &Shared) {
    window.invoke_set_appearance(lock(&shared.settings).app.appearance.is_dark());
    let toggled = shared.clone();
    window.on_appearance_toggled(move |dark| {
        save_app(&toggled, |app| {
            app.appearance = if dark { Appearance::Dark } else { Appearance::Light };
        });
    });
}

/// Saves a switch of Celata itself at once, like "Show donation hint": no validation, no question, nothing to
/// the service, no reconnect. The display follows the switch even if saving fails.
fn save_app(shared: &Shared, change: impl FnOnce(&mut AppSettings)) {
    let Some(window) = shared.window.upgrade() else { return };
    let mut store = lock(&shared.settings);
    change(&mut store.app);
    if let Some(path) = &store.settings_path {
        match settings::save(path, &store.current, &store.kept, &store.app) {
            Ok(()) => set_file_notice(&window, None),
            Err(error) => set_file_notice(&window, Some(save_failed(&error))),
        }
    }
}

/// Connects the section "Application": notifications, running in the background, start at login.
fn connect_application(window: &MainWindow, shared: &Shared) {
    {
        let store = lock(&shared.settings);
        window.set_notifications(store.app.notifications);
        window.set_keep_running(store.app.keep_running);
        window.set_update_check(store.app.update_check);
    }
    // An own entry for the start at login follows a moved program; errors go to the terminal only.
    if let (Ok(exe), Some(home)) = (desktop::program_path(), autostart::config_home())
        && let Err(error) = autostart::refresh(&exe, &home)
    {
        report(Phrase::raw(error.to_string()));
    }
    show_autostart(window);
    let toggle = shared.clone();
    window.on_update_check_toggled(move |on| {
        save_app(&toggle, |app| app.update_check = on);
        if let Some(window) = toggle.window.upgrade() {
            window.set_update_check(on);
        }
        // Switched on: check at once. Switched off: no further request; a hint already shown stays.
        if on {
            UPDATE.with(|schedule| schedule.borrow_mut().enable());
            check_update(&toggle);
        }
    });
    let releases = shared.clone();
    window.on_open_update(move || open_link(&releases, update::RELEASES_URL, Key::UpdateBlockedNotice));
    let toggle = shared.clone();
    window.on_notifications_toggled(move |on| {
        save_app(&toggle, |app| app.notifications = on);
        if let Some(window) = toggle.window.upgrade() {
            window.set_notifications(on);
        }
    });
    let toggle = shared.clone();
    window.on_keep_running_toggled(move |on| {
        save_app(&toggle, |app| app.keep_running = on);
        if let Some(window) = toggle.window.upgrade() {
            window.set_keep_running(on);
        }
    });
    let toggle = shared.clone();
    window.on_autostart_toggled(move |on| {
        let changed = match (desktop::program_path(), autostart::config_home()) {
            (Ok(exe), Some(home)) if on => autostart::enable(&exe, &home),
            (_, Some(home)) if !on => autostart::disable(&home),
            _ => Err(std::io::Error::other("no configuration directory")),
        };
        if changed.is_err() {
            show_toast(&toggle, Phrase::key(Key::SettingsAutostartFailed), true);
        }
        // The switch shows what the entry really is now.
        if let Some(window) = toggle.window.upgrade() {
            show_autostart(&window);
        }
    });
}

thread_local! {
    /// When the next check for a new version is due and which version was found; only for this session.
    static UPDATE: RefCell<update::Schedule> = RefCell::default();
}

/// Asks GitHub for a newer version when the switch is on and a check is due: at start, once the first state of
/// the service is known, then once a day. Never while the internet is blocked or the routes cannot be read;
/// such a check is left out until the next one is due.
fn check_update(shared: &Shared) {
    if !lock(&shared.settings).app.update_check {
        return;
    }
    let Some(display) = lock(&shared.monitor).display().cloned() else {
        return;
    };
    let now = Instant::now();
    let due = UPDATE.with(|schedule| {
        let mut schedule = schedule.borrow_mut();
        let due = schedule.due(now);
        if due {
            schedule.done(now);
        }
        due
    });
    if !due || !update::may_check(Some(&display), &killswitch::is_blocked()) {
        return;
    }
    let found = shared.clone();
    spawn("update", move || {
        // The routes are read once more right before the request.
        let Some(version) = update::fetch_latest(killswitch::is_blocked) else {
            return;
        };
        let _ = slint::invoke_from_event_loop(move || {
            if UPDATE.with(|schedule| schedule.borrow_mut().record(version))
                && let Some(window) = found.window.upgrade()
            {
                show_update(&window);
                show_tray_menu(&window, &found);
            }
        });
    });
}

/// The hint in the overview in the language in force; empty without a newer version.
fn show_update(window: &MainWindow) {
    let text = UPDATE.with(|schedule| {
        schedule.borrow().shown().map(|version| {
            i18n::fill(
                i18n::text(i18n::current(), Key::UpdateAvailable),
                &[version.to_string()],
            )
        })
    });
    window.set_update_text(text.unwrap_or_default().into());
}

/// Shows the state of the entry for the start at login: the file itself is the state of the switch.
fn show_autostart(window: &MainWindow) {
    let Some(home) = autostart::config_home() else {
        window.set_autostart(false);
        window.set_autostart_foreign(false);
        window.set_autostart_note(SharedString::new());
        return;
    };
    let state = autostart::current(&home).unwrap_or(AutostartState::Off);
    window.set_autostart(state == AutostartState::On);
    window.set_autostart_foreign(state == AutostartState::Foreign);
    let note = if state == AutostartState::Foreign {
        tr(&Phrase::fill(
            Key::SettingsAutostartForeign,
            [Phrase::raw(home_relative(&autostart::path(&home)))],
        ))
    } else {
        SharedString::new()
    };
    window.set_autostart_note(note);
}

/// A path below the home directory as `~/…`, otherwise as it is.
fn home_relative(path: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) if path.starts_with(&home) && home.is_absolute() => {
            format!("~/{}", path.strip_prefix(&home).unwrap_or(path).display())
        }
        _ => path.display().to_string(),
    }
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
