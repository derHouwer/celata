// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Start at login through an own entry in the XDG autostart directory, without root.
//!
//! The switch "Start at login" has no field in the settings: the entry itself is its state. Only an entry with
//! the marker line of Celata belongs to it; a foreign entry of the same name is never changed or removed.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::desktop::{self, APP_ID, MARKER};
use crate::files;
use crate::i18n::{self, Key, Language};

/// Largest entry Celata reads; a larger file is not one of its own.
const MAX_FILE: u64 = 64 * 1024;

/// `$XDG_CONFIG_HOME` (only an absolute path counts), otherwise `~/.config`.
pub fn config_home() -> Option<PathBuf> {
    files::user_dir("XDG_CONFIG_HOME", ".config")
}

/// `<config_home>/autostart/celata.desktop`.
pub fn path(config_home: &Path) -> PathBuf {
    config_home.join("autostart").join(format!("{APP_ID}.desktop"))
}

/// The entry: like the desktop entry of Celata, but started with `--autostart`, after the panel, and never in a
/// terminal. The marker line comes last.
pub fn entry(exe: &Path) -> String {
    let mut lines = vec![
        "[Desktop Entry]".to_owned(),
        "Type=Application".to_owned(),
        "Name=Celata".to_owned(),
        format!(
            "Comment={}",
            desktop::escape_string(i18n::text(Language::En, Key::DesktopEntryComment))
        ),
    ];
    for language in Language::ALL.into_iter().filter(|language| *language != Language::En) {
        let comment = desktop::escape_string(i18n::text(language, Key::DesktopEntryComment));
        lines.push(format!("Comment[{}]={comment}", language.code()));
    }
    lines.extend([
        format!("Exec={} --autostart", desktop::exec_value(exe)),
        format!("Icon={APP_ID}"),
        "Terminal=false".to_owned(),
        "X-KDE-autostart-phase=2".to_owned(),
        MARKER.to_owned(),
    ]);
    let mut content = lines.join("\n");
    content.push('\n');
    content
}

/// State of the switch "Start at login".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutostartState {
    /// No entry.
    Off,
    /// An entry of Celata.
    On,
    /// Another entry of the same name: the switch is off and cannot be used.
    Foreign,
}

/// State from the content of the entry; `None` if there is none.
pub fn state(existing: Option<&str>) -> AutostartState {
    match existing {
        None => AutostartState::Off,
        Some(content) if desktop::has_marker(content) => AutostartState::On,
        Some(_) => AutostartState::Foreign,
    }
}

/// Reads the entry: `Ok(None)` if there is none; a file too large or not text counts as foreign.
fn read(path: &Path) -> io::Result<Option<String>> {
    match files::read_limited(path, MAX_FILE) {
        Ok(Some(bytes)) => Ok(Some(String::from_utf8(bytes).unwrap_or_default())),
        Ok(None) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => Ok(Some(String::new())),
        Err(error) => Err(error),
    }
}

/// State of the entry below `config_home`.
pub fn current(config_home: &Path) -> io::Result<AutostartState> {
    Ok(state(read(&path(config_home))?.as_deref()))
}

fn usable(exe: &Path) -> io::Result<()> {
    if !exe.is_absolute() || exe.to_str().is_none_or(|path| path.chars().any(char::is_control)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "program path cannot be used in an autostart entry",
        ));
    }
    Ok(())
}

/// Switch on: write the own entry (0644, through a temporary file). A foreign entry stays and is an error.
pub fn enable(exe: &Path, config_home: &Path) -> io::Result<()> {
    usable(exe)?;
    let target = path(config_home);
    let existing = read(&target)?;
    if state(existing.as_deref()) == AutostartState::Foreign {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "a foreign autostart entry exists",
        ));
    }
    let new = entry(exe);
    if existing.as_deref() != Some(new.as_str()) {
        files::write_atomic(&target, new.as_bytes(), files::PUBLIC_MODE)?;
    }
    Ok(())
}

/// Switch off: remove only an entry of Celata.
pub fn disable(config_home: &Path) -> io::Result<()> {
    let target = path(config_home);
    if state(read(&target)?.as_deref()) == AutostartState::On {
        match fs::remove_file(&target) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

/// At every start: an own entry follows a changed program path (for example after moving the project folder).
/// No entry or a foreign one stays as it is.
pub fn refresh(exe: &Path, config_home: &Path) -> io::Result<()> {
    let target = path(config_home);
    let existing = read(&target)?;
    if state(existing.as_deref()) != AutostartState::On {
        return Ok(());
    }
    usable(exe)?;
    let new = entry(exe);
    if existing.as_deref() != Some(new.as_str()) {
        files::write_atomic(&target, new.as_bytes(), files::PUBLIC_MODE)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "celata-autostart-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const EXE: &str = "/opt/celata/bin/celata";

    #[test]
    fn pfad_im_autostart_ordner() {
        assert_eq!(
            path(Path::new("/home/user/.config")),
            Path::new("/home/user/.config/autostart/celata.desktop")
        );
    }

    #[test]
    fn eintrag_startet_mit_autostart_nach_der_leiste() {
        let content = entry(Path::new(EXE));
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines[0], "[Desktop Entry]");
        assert!(
            lines.contains(&format!("Exec=\"{EXE}\" --autostart").as_str()),
            "{content}"
        );
        assert!(lines.contains(&"Icon=celata"));
        assert!(lines.contains(&"Terminal=false"));
        assert!(lines.contains(&"X-KDE-autostart-phase=2"));
        assert_eq!(lines.last(), Some(&MARKER));
        let desktop_entry = desktop::entry(Path::new(EXE));
        for comment in desktop_entry.lines().filter(|line| line.starts_with("Comment")) {
            assert!(lines.contains(&comment), "{comment}");
        }
        assert!(lines.iter().any(|line| line.starts_with("Comment[de]=")));
    }

    #[test]
    fn programmpfad_wird_maskiert() {
        let content = entry(Path::new("/opt/my apps/ce\"la$ta\\x%y"));
        let exec = content.lines().find(|line| line.starts_with("Exec=")).unwrap();
        assert_eq!(exec, r#"Exec="/opt/my apps/ce\\"la\\$ta\\\\x%%y" --autostart"#);
        assert_eq!(
            exec.trim_end_matches(" --autostart"),
            format!("Exec={}", desktop::exec_value(Path::new("/opt/my apps/ce\"la$ta\\x%y")))
        );
    }

    #[test]
    fn zustand_aus_dem_inhalt() {
        assert_eq!(state(None), AutostartState::Off);
        assert_eq!(state(Some(&entry(Path::new(EXE)))), AutostartState::On);
        assert_eq!(state(Some("[Desktop Entry]\nExec=celata\n")), AutostartState::Foreign);
        assert_eq!(state(Some("")), AutostartState::Foreign);
    }

    #[test]
    fn einschalten_und_ausschalten() {
        let home = temp_dir();
        assert_eq!(current(&home).unwrap(), AutostartState::Off);
        enable(Path::new(EXE), &home).unwrap();
        assert_eq!(current(&home).unwrap(), AutostartState::On);
        let file = path(&home);
        assert_eq!(fs::read_to_string(&file).unwrap(), entry(Path::new(EXE)));
        let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
        assert!(!home.join("autostart/celata.desktop.tmp").exists());
        disable(&home).unwrap();
        assert_eq!(current(&home).unwrap(), AutostartState::Off);
        assert!(!file.exists());
        // Switching off twice is fine.
        disable(&home).unwrap();
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn fremder_eintrag_bleibt_unveraendert() {
        let home = temp_dir();
        let file = path(&home);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let foreign = "[Desktop Entry]\nExec=/usr/bin/celata --minimized\n";
        fs::write(&file, foreign).unwrap();
        assert_eq!(current(&home).unwrap(), AutostartState::Foreign);
        assert!(enable(Path::new(EXE), &home).is_err());
        disable(&home).unwrap();
        refresh(Path::new("/elsewhere/celata"), &home).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), foreign);
        // Too large or not text: foreign as well.
        fs::write(&file, vec![b'x'; 70 * 1024]).unwrap();
        assert_eq!(current(&home).unwrap(), AutostartState::Foreign);
        fs::write(&file, [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(current(&home).unwrap(), AutostartState::Foreign);
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn eigener_eintrag_folgt_dem_programmpfad() {
        let home = temp_dir();
        // Off stays off.
        refresh(Path::new(EXE), &home).unwrap();
        assert_eq!(current(&home).unwrap(), AutostartState::Off);
        enable(Path::new(EXE), &home).unwrap();
        let moved = Path::new("/usr/local/bin/celata");
        refresh(moved, &home).unwrap();
        assert_eq!(fs::read_to_string(path(&home)).unwrap(), entry(moved));
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn unbrauchbarer_programmpfad_wird_nicht_eingetragen() {
        let home = temp_dir();
        assert!(enable(Path::new("relative/celata"), &home).is_err());
        assert!(enable(Path::new("/tmp/cel\nata"), &home).is_err());
        assert_eq!(current(&home).unwrap(), AutostartState::Off);
        fs::remove_dir_all(&home).unwrap();
    }
}
