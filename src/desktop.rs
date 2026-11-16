// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Registration with the desktop: application ID, desktop entry and icon in the user's directory.
//!
//! KDE Plasma assigns a Wayland window to a desktop entry through its application ID and shows the icon of
//! that entry in the task bar and the title bar. Celata creates both itself for the signed-in user, without
//! `sudo`. It only writes below `$XDG_DATA_HOME` or `~/.local/share`; a foreign entry of the same name
//! stays untouched.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::files;
use crate::i18n::{self, Key, Language};

/// Application ID for the desktop (`app_id` under Wayland), name of the desktop entry and of the icon.
pub const APP_ID: &str = "celata";

/// The logo, the same file as the window icon and the image in the header.
pub const ICON_SVG: &[u8] = include_bytes!("../ui/icons/celata.svg");

/// Marker line: only an entry with this line was created by Celata itself and may be changed by it.
pub const MARKER: &str = "X-Celata-Generated=true";

/// Desktop entry relative to the data directory.
pub fn entry_path(data_home: &Path) -> PathBuf {
    data_home.join("applications").join(format!("{APP_ID}.desktop"))
}

/// Icon relative to the data directory.
pub fn icon_path(data_home: &Path) -> PathBuf {
    data_home
        .join("icons/hicolor/scalable/apps")
        .join(format!("{APP_ID}.svg"))
}

/// A value of type `string` according to the Desktop Entry Specification: `\` becomes `\\`, line breaks
/// and tabs become `\n`, `\t`, `\r`.
fn escape_string(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\t' => result.push_str("\\t"),
            '\r' => result.push_str("\\r"),
            _ => result.push(c),
        }
    }
    result
}

/// `Exec` with the program path as its only, quoted argument. Inside the quotes `"`, `` ` ``, `$` and `\`
/// are escaped with `\`, `%` becomes `%%` (no field code). After that the rule for `string` values
/// applies, which doubles every `\` once more (specification, section "The Exec key").
fn exec_value(exe: &Path) -> String {
    let path = exe.to_string_lossy();
    let mut quoted = String::with_capacity(path.len() + 2);
    quoted.push('"');
    for c in path.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    escape_string(&quoted)
}

/// Content of the desktop entry for the program at `exe`.
pub fn entry(exe: &Path) -> String {
    let mut lines = vec![
        "[Desktop Entry]".to_owned(),
        "Type=Application".to_owned(),
        "Name=Celata".to_owned(),
        format!(
            "Comment={}",
            escape_string(i18n::text(Language::En, Key::DesktopEntryComment))
        ),
    ];
    for language in Language::ALL.into_iter().filter(|language| *language != Language::En) {
        let comment = escape_string(i18n::text(language, Key::DesktopEntryComment));
        lines.push(format!("Comment[{}]={comment}", language.code()));
    }
    lines.extend([
        format!("Exec={}", exec_value(exe)),
        format!("Icon={APP_ID}"),
        "Terminal=false".to_owned(),
        "Categories=Network;".to_owned(),
        format!("StartupWMClass={APP_ID}"),
        MARKER.to_owned(),
    ]);
    let mut content = lines.join("\n");
    content.push('\n');
    content
}

/// Should the desktop entry be written? Yes if it is missing, or if Celata created it itself (marker
/// line) and it differs from the new content. A foreign entry always stays.
pub fn should_write(existing: Option<&str>, new: &str) -> bool {
    match existing {
        None => true,
        Some(existing) => has_marker(existing) && existing != new,
    }
}

/// `$XDG_DATA_HOME` (only an absolute path counts), otherwise `~/.local/share`.
pub fn data_home() -> Option<PathBuf> {
    files::user_dir("XDG_DATA_HOME", ".local/share")
}

/// Writes `content` through a temporary file in the same directory with permissions 0644.
fn write_public(path: &Path, content: &[u8]) -> io::Result<()> {
    files::write_atomic(path, content, files::PUBLIC_MODE)
}

/// Largest desktop entry or icon Celata reads; a larger file is not one of its own.
const MAX_FILE: u64 = 64 * 1024;

/// The system data directories: `$XDG_DATA_DIRS`, or `/usr/local/share` and `/usr/share` if it is not set
/// or empty. Empty and relative entries do not count (XDG Base Directory Specification).
#[must_use]
pub fn system_data_dirs() -> Vec<PathBuf> {
    system_data_dirs_from(std::env::var_os("XDG_DATA_DIRS").as_deref())
}

fn system_data_dirs_from(value: Option<&std::ffi::OsStr>) -> Vec<PathBuf> {
    match value {
        Some(value) if !value.is_empty() => std::env::split_paths(value).filter(|dir| dir.is_absolute()).collect(),
        _ => vec![PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")],
    }
}

/// Is there a desktop entry for Celata in one of the system directories, as a package installs it? The
/// user's own data directory never counts, even if `XDG_DATA_DIRS` lists it, and neither does an entry
/// Celata wrote itself (the directory may be listed under another spelling).
#[must_use]
pub fn has_system_entry(system_dirs: &[PathBuf], data_home: &Path) -> bool {
    system_dirs
        .iter()
        .any(|dir| dir != data_home && is_packaged_entry(&entry_path(dir)))
}

/// A readable entry without the marker line. An entry too large to read is not one of Celata's either.
fn is_packaged_entry(path: &Path) -> bool {
    match read_small(path) {
        Ok(Found::Content(bytes)) => !has_marker(&String::from_utf8_lossy(&bytes)),
        Ok(Found::TooLarge) => true,
        Ok(Found::Missing) | Err(_) => false,
    }
}

fn has_marker(entry: &str) -> bool {
    entry.lines().any(|line| line.trim() == MARKER)
}

/// What [`install`] does with the entry in the user's directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// A package provides the entry: write nothing and remove an entry Celata created earlier.
    RemoveOwn,
    /// The entry in the user's directory is not from Celata: touch neither it nor the icon.
    LeaveForeign,
    /// No entry yet, or an own one: create or update icon and entry.
    WriteOwn,
}

/// Decides by two observations: is there a system entry, and what is the entry in the user's directory
/// (`None` = missing, `Some(None)` = not readable as text or too large, `Some(Some(text))` = its content).
#[must_use]
pub fn plan(system_entry: bool, existing: Option<Option<&str>>) -> Plan {
    let foreign = match existing {
        None => false,
        Some(None) => true,
        Some(Some(text)) => !has_marker(text),
    };
    if foreign {
        Plan::LeaveForeign
    } else if system_entry {
        Plan::RemoveOwn
    } else {
        Plan::WriteOwn
    }
}

/// What lies at a place in the user's directory.
enum Found {
    Missing,
    /// Larger than [`MAX_FILE`]: not read.
    TooLarge,
    Content(Vec<u8>),
}

fn read_small(path: &Path) -> io::Result<Found> {
    match files::read_limited(path, MAX_FILE) {
        Ok(Some(content)) => Ok(Found::Content(content)),
        Ok(None) => Ok(Found::Missing),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => Ok(Found::TooLarge),
        Err(error) => Err(error),
    }
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Keeps exactly one desktop entry for Celata (rule table in [`plan`]).
///
/// Without a system entry in `system_dirs`, icon and entry are created or updated below `data_home`; the icon
/// comes first, so that the entry never points to a missing icon. With a system entry, as a package installs
/// it, nothing is written, and an entry Celata created earlier is removed together with its icon (the icon
/// only if its content is still Celata's own). A foreign entry of the same name is never touched, nor is the
/// icon beside it. A program path that is not valid UTF-8 or contains control characters is not entered.
pub fn install(exe: &Path, data_home: &Path, system_dirs: &[PathBuf]) -> io::Result<()> {
    if !exe.is_absolute() || exe.to_str().is_none_or(|path| path.chars().any(char::is_control)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "program path cannot be used in a desktop entry",
        ));
    }
    let path = entry_path(data_home);
    let icon = icon_path(data_home);
    let existing = match read_small(&path)? {
        Found::Missing => None,
        Found::TooLarge => Some(None),
        // Bytes that are not text are not an entry of Celata either.
        Found::Content(bytes) => Some(String::from_utf8(bytes).ok()),
    };
    let own_icon =
        |icon: &Path| Ok::<_, io::Error>(matches!(read_small(icon)?, Found::Content(bytes) if bytes == ICON_SVG));
    match plan(
        has_system_entry(system_dirs, data_home),
        existing.as_ref().map(Option::as_deref),
    ) {
        Plan::LeaveForeign => {}
        Plan::RemoveOwn => {
            remove_if_present(&path)?;
            if own_icon(&icon)? {
                remove_if_present(&icon)?;
            }
        }
        Plan::WriteOwn => {
            if !own_icon(&icon)? {
                write_public(&icon, ICON_SVG)?;
            }
            let new = entry(exe);
            if should_write(existing.flatten().as_deref(), &new) {
                write_public(&path, new.as_bytes())?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::sync::atomic::{AtomicU32, Ordering};

    fn data_dir() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let number = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("celata-desktop-{}-{number}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn field<'a>(content: &'a str, key: &str) -> Option<&'a str> {
        content
            .lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
    }

    #[test]
    fn eintrag_hat_alle_felder() {
        let content = entry(Path::new("/home/user/celata/target/debug/celata"));
        assert!(content.starts_with("[Desktop Entry]\n"));
        assert!(content.ends_with('\n'));
        assert_eq!(field(&content, "Type"), Some("Application"));
        assert_eq!(field(&content, "Name"), Some("Celata"));
        assert_eq!(
            field(&content, "Comment"),
            Some("Graphical interface for the hide.me VPN client")
        );
        assert_eq!(
            field(&content, "Comment[de]"),
            Some("Grafische Oberfläche für den hide.me-VPN-Client")
        );
        assert_eq!(
            field(&content, "Exec"),
            Some("\"/home/user/celata/target/debug/celata\"")
        );
        assert_eq!(field(&content, "Icon"), Some("celata"));
        assert_eq!(field(&content, "Terminal"), Some("false"));
        assert_eq!(field(&content, "Categories"), Some("Network;"));
        assert_eq!(field(&content, "StartupWMClass"), Some("celata"));
        assert_eq!(field(&content, "X-Celata-Generated"), Some("true"));
        assert_eq!(content.lines().count(), 11, "{content}");
    }

    #[test]
    fn jede_sprache_hat_eine_beschreibung() {
        let content = entry(Path::new("/x/celata"));
        for language in Language::ALL.into_iter().filter(|language| *language != Language::En) {
            assert!(
                field(&content, &format!("Comment[{}]", language.code())).is_some(),
                "{content}"
            );
        }
    }

    #[test]
    fn exec_wird_maskiert() {
        let exec = |path: &str| field(&entry(Path::new(path)), "Exec").unwrap().to_owned();
        assert_eq!(exec("/home/a b/celata"), r#""/home/a b/celata""#);
        // `"` → `\"`, then `\` → `\\`.
        assert_eq!(exec(r#"/home/a"b/celata"#), r#""/home/a\\"b/celata""#);
        assert_eq!(exec("/home/$HOME/celata"), r#""/home/\\$HOME/celata""#);
        assert_eq!(exec("/home/a`b/celata"), r#""/home/a\\`b/celata""#);
        // One `\` becomes four.
        assert_eq!(exec(r"/home/a\b/celata"), r#""/home/a\\\\b/celata""#);
        assert_eq!(exec("/home/100%/celata"), r#""/home/100%%/celata""#);
    }

    #[test]
    fn schreiben_nur_bei_fehlendem_oder_eigenem_veraltetem_eintrag() {
        let new = entry(Path::new("/neu/celata"));
        let old = entry(Path::new("/alt/celata"));
        assert!(should_write(None, &new));
        assert!(should_write(Some(&old), &new));
        assert!(!should_write(Some(&new), &new));
        let foreign = "[Desktop Entry]\nType=Application\nName=Celata\nExec=/usr/bin/celata\n";
        assert!(!should_write(Some(foreign), &new));
        assert!(!should_write(Some(""), &new));
    }

    #[test]
    fn install_legt_an_und_aendert_danach_nichts() {
        let data = data_dir();
        let exe = Path::new("/home/user/celata/target/debug/celata");
        install(exe, &data, &[]).unwrap();
        let entry_file = entry_path(&data);
        let icon_file = icon_path(&data);
        assert_eq!(fs::read_to_string(&entry_file).unwrap(), entry(exe));
        assert_eq!(fs::read(&icon_file).unwrap(), ICON_SVG);
        for path in [&entry_file, &icon_file] {
            assert_eq!(fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o644);
        }
        assert!(entry_file.ends_with("applications/celata.desktop"));
        assert!(icon_file.ends_with("icons/hicolor/scalable/apps/celata.svg"));
        let inodes = |data: &Path| {
            (
                fs::metadata(entry_path(data)).unwrap().ino(),
                fs::metadata(icon_path(data)).unwrap().ino(),
            )
        };
        let before = inodes(&data);
        install(exe, &data, &[]).unwrap();
        assert_eq!(inodes(&data), before, "ein zweiter Aufruf schreibt nichts");
        // New program path (for example after moving the directory): the own entry follows.
        let moved = Path::new("/home/user/neu/target/debug/celata");
        install(moved, &data, &[]).unwrap();
        assert_eq!(fs::read_to_string(&entry_file).unwrap(), entry(moved));
        let names: Vec<_> = fs::read_dir(entry_file.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["celata.desktop"], "keine Hilfsdatei bleibt liegen");
    }

    #[test]
    fn fremder_eintrag_bleibt() {
        let data = data_dir();
        let foreign = "[Desktop Entry]\nType=Application\nName=Anderes Programm\nExec=/usr/bin/celata\n";
        fs::create_dir_all(entry_path(&data).parent().unwrap()).unwrap();
        fs::write(entry_path(&data), foreign).unwrap();
        install(Path::new("/x/celata"), &data, &[]).unwrap();
        assert_eq!(fs::read_to_string(entry_path(&data)).unwrap(), foreign);
        // A file that is not text stays as well.
        fs::write(entry_path(&data), [0xff, 0xfe]).unwrap();
        install(Path::new("/x/celata"), &data, &[]).unwrap();
        assert_eq!(fs::read(entry_path(&data)).unwrap(), [0xff, 0xfe]);
    }

    /// A system data directory with a packaged entry, as `/usr/share` after installing a package.
    fn system_dir_with_entry() -> PathBuf {
        let dir = data_dir();
        fs::create_dir_all(dir.join("applications")).unwrap();
        fs::write(
            dir.join("applications/celata.desktop"),
            "[Desktop Entry]\nType=Application\nName=Celata\nExec=celata\n",
        )
        .unwrap();
        dir
    }

    const FOREIGN: &str = "[Desktop Entry]\nType=Application\nName=Other program\nExec=/usr/bin/celata\n";

    #[test]
    fn system_directories_follow_the_xdg_rules() {
        use std::ffi::OsStr;
        let defaults = [PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")];
        assert_eq!(system_data_dirs_from(None), defaults);
        assert_eq!(system_data_dirs_from(Some(OsStr::new(""))), defaults);
        assert_eq!(
            system_data_dirs_from(Some(OsStr::new("/a:/b/c"))),
            [PathBuf::from("/a"), PathBuf::from("/b/c")]
        );
        // Empty and relative entries do not count.
        assert_eq!(
            system_data_dirs_from(Some(OsStr::new("relative::/c:./d:"))),
            [PathBuf::from("/c")]
        );
        assert_eq!(system_data_dirs_from(Some(OsStr::new("x:y"))), Vec::<PathBuf>::new());
    }

    #[test]
    fn system_entry_is_found_only_as_a_file_outside_the_own_directory() {
        let system = system_dir_with_entry();
        let own = data_dir();
        assert!(has_system_entry(&[data_dir(), system.clone()], &own));
        assert!(!has_system_entry(&[], &own));
        assert!(!has_system_entry(&[data_dir()], &own));
        // The user's own directory never counts, even if it is listed.
        assert!(!has_system_entry(std::slice::from_ref(&system), &system));
        // A directory with that name is not an entry.
        let odd = data_dir();
        fs::create_dir_all(odd.join("applications/celata.desktop")).unwrap();
        assert!(!has_system_entry(&[odd], &own));
    }

    /// The user's own directory can be listed in `XDG_DATA_DIRS` under another spelling (here: through a
    /// link). The entry Celata wrote itself must not be taken for the entry of a package, or Celata would
    /// remove and recreate it on alternate starts.
    #[test]
    fn own_entry_under_another_spelling_is_not_a_system_entry() {
        let data = data_dir();
        let exe = Path::new("/home/user/celata/target/debug/celata");
        install(exe, &data, &[]).unwrap();
        let alias = data_dir();
        std::os::unix::fs::symlink(&data, &alias).unwrap();
        assert!(!has_system_entry(std::slice::from_ref(&alias), &data));
        install(exe, &data, std::slice::from_ref(&alias)).unwrap();
        assert!(
            entry_path(&data).exists() && icon_path(&data).exists(),
            "the own entry must stay"
        );
        // An oversized file at the place of a system entry is not Celata's own: it counts as packaged.
        let odd = data_dir();
        fs::create_dir_all(odd.join("applications")).unwrap();
        fs::write(entry_path(&odd), format!("{MARKER}\n{}", "x".repeat(70 * 1024))).unwrap();
        assert!(has_system_entry(&[odd], &data));
    }

    #[test]
    fn plan_follows_the_rule_table() {
        let own = entry(Path::new("/x/celata"));
        assert_eq!(plan(false, None), Plan::WriteOwn);
        assert_eq!(plan(false, Some(Some(&own))), Plan::WriteOwn);
        assert_eq!(plan(false, Some(Some(FOREIGN))), Plan::LeaveForeign);
        assert_eq!(plan(false, Some(None)), Plan::LeaveForeign);
        assert_eq!(plan(true, None), Plan::RemoveOwn);
        assert_eq!(plan(true, Some(Some(&own))), Plan::RemoveOwn);
        assert_eq!(plan(true, Some(Some(FOREIGN))), Plan::LeaveForeign);
        assert_eq!(plan(true, Some(None)), Plan::LeaveForeign);
    }

    #[test]
    fn with_a_system_entry_nothing_is_written() {
        let data = data_dir();
        install(Path::new("/usr/bin/celata"), &data, &[system_dir_with_entry()]).unwrap();
        assert!(!entry_path(&data).exists());
        assert!(!icon_path(&data).exists());
    }

    #[test]
    fn with_a_system_entry_the_own_entry_and_icon_are_removed() {
        let data = data_dir();
        let exe = Path::new("/home/user/celata/target/debug/celata");
        install(exe, &data, &[]).unwrap();
        assert!(entry_path(&data).exists() && icon_path(&data).exists());
        install(exe, &data, &[system_dir_with_entry()]).unwrap();
        assert!(!entry_path(&data).exists(), "own entry must be removed");
        assert!(!icon_path(&data).exists(), "own icon must be removed");
        // A second run finds nothing to do and does not fail.
        install(exe, &data, &[system_dir_with_entry()]).unwrap();
    }

    #[test]
    fn with_a_system_entry_a_foreign_entry_and_its_icon_stay() {
        let data = data_dir();
        fs::create_dir_all(entry_path(&data).parent().unwrap()).unwrap();
        fs::create_dir_all(icon_path(&data).parent().unwrap()).unwrap();
        fs::write(entry_path(&data), FOREIGN).unwrap();
        fs::write(icon_path(&data), "<svg/>").unwrap();
        install(Path::new("/x/celata"), &data, &[system_dir_with_entry()]).unwrap();
        assert_eq!(fs::read_to_string(entry_path(&data)).unwrap(), FOREIGN);
        assert_eq!(fs::read_to_string(icon_path(&data)).unwrap(), "<svg/>");
    }

    #[test]
    fn with_a_system_entry_a_changed_icon_stays() {
        let data = data_dir();
        install(Path::new("/x/celata"), &data, &[]).unwrap();
        fs::write(icon_path(&data), "<svg/>").unwrap();
        install(Path::new("/x/celata"), &data, &[system_dir_with_entry()]).unwrap();
        assert!(!entry_path(&data).exists());
        assert_eq!(
            fs::read_to_string(icon_path(&data)).unwrap(),
            "<svg/>",
            "only an icon with Celata's own content is removed"
        );
    }

    /// Beside a foreign entry of the same name, the icon is not touched either.
    #[test]
    fn beside_a_foreign_entry_the_icon_is_not_touched() {
        let data = data_dir();
        fs::create_dir_all(entry_path(&data).parent().unwrap()).unwrap();
        fs::write(entry_path(&data), FOREIGN).unwrap();
        install(Path::new("/x/celata"), &data, &[]).unwrap();
        assert!(!icon_path(&data).exists(), "no icon is created beside a foreign entry");
        fs::create_dir_all(icon_path(&data).parent().unwrap()).unwrap();
        fs::write(icon_path(&data), "<svg/>").unwrap();
        install(Path::new("/x/celata"), &data, &[]).unwrap();
        assert_eq!(fs::read_to_string(icon_path(&data)).unwrap(), "<svg/>");
    }

    #[test]
    fn an_oversized_entry_counts_as_foreign() {
        let data = data_dir();
        fs::create_dir_all(entry_path(&data).parent().unwrap()).unwrap();
        let huge = format!("{MARKER}\n{}", "x".repeat(70 * 1024));
        fs::write(entry_path(&data), &huge).unwrap();
        install(Path::new("/x/celata"), &data, &[]).unwrap();
        assert_eq!(fs::read_to_string(entry_path(&data)).unwrap().len(), huge.len());
        install(Path::new("/x/celata"), &data, &[system_dir_with_entry()]).unwrap();
        assert!(entry_path(&data).exists());
    }

    #[test]
    fn veraendertes_symbol_wird_ersetzt() {
        let data = data_dir();
        fs::create_dir_all(icon_path(&data).parent().unwrap()).unwrap();
        fs::write(icon_path(&data), "<svg/>").unwrap();
        install(Path::new("/x/celata"), &data, &[]).unwrap();
        assert_eq!(fs::read(icon_path(&data)).unwrap(), ICON_SVG);
    }

    #[test]
    fn unbrauchbarer_programmpfad_wird_nicht_eingetragen() {
        let data = data_dir();
        for exe in ["celata", "/x/zeile\nExec=/bin/sh"] {
            let error = install(Path::new(exe), &data, &[]).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }
        assert!(!data.exists());
    }

    #[test]
    fn logo_ist_eigene_zeichnung_ohne_fremde_bestandteile() {
        let svg = std::str::from_utf8(ICON_SVG).unwrap();
        assert!(svg.contains("Celata logo, original drawing"));
        for forbidden in ["<script", "<text", "href", "url(", "<image", "<style"] {
            assert!(!svg.contains(forbidden), "{forbidden}");
        }
    }
}
