// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Files in the user's own directories: where they live and how they are written and read.

use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Permissions of files anyone may read (caches of public lists, desktop entry, icon).
pub const PUBLIC_MODE: u32 = 0o644;

/// Base directory from an XDG variable (only an absolute path counts), otherwise `fallback` below `$HOME`.
#[must_use]
pub fn user_dir(variable: &str, fallback: &str) -> Option<PathBuf> {
    resolve_user_dir(
        std::env::var_os(variable).map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
        fallback,
    )
}

fn resolve_user_dir(from_variable: Option<PathBuf>, home: Option<PathBuf>, fallback: &str) -> Option<PathBuf> {
    from_variable
        .filter(|path| path.is_absolute())
        .or_else(|| home.map(|home| home.join(fallback)))
}

/// Seconds since 1970-01-01; a clock before that counts as 0.
#[must_use]
pub fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |age| age.as_secs())
}

/// Writes `content` to `path` with exactly the permissions `mode`, through a temporary file in the same
/// directory, so that a half-written file never appears under the final name.
///
/// The temporary file is created exclusively: something lying at its place (even a symbolic link) is removed
/// first and never written through.
pub fn write_atomic(path: &Path, content: &[u8], mode: u32) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut name = path.file_name().ok_or(io::ErrorKind::InvalidInput)?.to_owned();
    name.push(".tmp");
    let temporary = path.with_file_name(name);
    let _ = fs::remove_file(&temporary);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&temporary)?;
    // Exactly these permissions, independent of the umask.
    let written = file
        .set_permissions(fs::Permissions::from_mode(mode))
        .and_then(|()| file.write_all(content))
        .and_then(|()| file.sync_all());
    drop(file);
    let result = written.and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Reads a file of at most `limit` bytes. `Ok(None)` if it does not exist; a larger file is an error.
pub fn read_limited(path: &Path, limit: u64) -> io::Result<Option<Vec<u8>>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut content = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut content)?;
    if u64::try_from(content.len()).is_ok_and(|length| length <= limit) {
        Ok(Some(content))
    } else {
        Err(io::ErrorKind::InvalidData.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_dir(name: &str) -> PathBuf {
        let number = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("celata-files-{}-{number}-{name}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn mode_of(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn xdg_variable_wins_when_absolute() {
        let home = Some(PathBuf::from("/home/user"));
        assert_eq!(
            resolve_user_dir(Some("/data".into()), home.clone(), ".config"),
            Some("/data".into())
        );
        assert_eq!(
            resolve_user_dir(Some("relative".into()), home.clone(), ".config"),
            Some("/home/user/.config".into())
        );
        assert_eq!(
            resolve_user_dir(Some(PathBuf::new()), home.clone(), ".cache"),
            Some("/home/user/.cache".into())
        );
        assert_eq!(
            resolve_user_dir(None, home, ".local/share"),
            Some("/home/user/.local/share".into())
        );
        assert_eq!(resolve_user_dir(None, None, ".config"), None);
        assert_eq!(resolve_user_dir(Some("relative".into()), None, ".config"), None);
    }

    #[test]
    fn unix_seconds_counts_from_1970() {
        assert_eq!(unix_seconds(UNIX_EPOCH), 0);
        assert_eq!(unix_seconds(UNIX_EPOCH + std::time::Duration::from_secs(90)), 90);
        assert_eq!(unix_seconds(UNIX_EPOCH - std::time::Duration::from_secs(5)), 0);
    }

    #[test]
    fn writes_content_with_exact_mode_and_creates_directories() {
        let dir = temp_dir("write");
        let private = dir.join("a/b/private.json");
        write_atomic(&private, b"one", 0o600).unwrap();
        assert_eq!(fs::read(&private).unwrap(), b"one");
        assert_eq!(mode_of(&private), 0o600);
        let public = dir.join("public.svg");
        write_atomic(&public, b"two", 0o644).unwrap();
        assert_eq!(mode_of(&public), 0o644);
    }

    #[test]
    fn replaces_an_existing_file_and_leaves_no_temporary_file() {
        let dir = temp_dir("replace");
        let path = dir.join("file.json");
        write_atomic(&path, b"old", 0o644).unwrap();
        write_atomic(&path, b"new", 0o600).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(mode_of(&path), 0o600);
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["file.json"]);
    }

    #[test]
    fn never_writes_through_a_link_at_the_temporary_place() {
        let dir = temp_dir("link");
        let victim = dir.join("victim");
        fs::write(&victim, b"untouched").unwrap();
        let path = dir.join("file.json");
        std::os::unix::fs::symlink(&victim, dir.join("file.json.tmp")).unwrap();
        write_atomic(&path, b"content", 0o600).unwrap();
        assert_eq!(fs::read(&victim).unwrap(), b"untouched");
        assert_eq!(fs::read(&path).unwrap(), b"content");
        assert!(!fs::symlink_metadata(&path).unwrap().file_type().is_symlink());
    }

    #[test]
    fn a_failed_write_leaves_the_old_file() {
        let dir = temp_dir("fail");
        let blocker = dir.join("blocker");
        fs::write(&blocker, b"").unwrap();
        // A file where a directory is needed.
        assert!(write_atomic(&blocker.join("file.json"), b"x", 0o600).is_err());
        // A directory where the file should go.
        let taken = dir.join("taken");
        fs::create_dir(&taken).unwrap();
        assert!(write_atomic(&taken, b"x", 0o600).is_err());
        assert!(taken.is_dir());
        assert!(!dir.join("taken.tmp").exists());
    }

    #[test]
    fn reads_up_to_the_limit() {
        let dir = temp_dir("read");
        let path = dir.join("file");
        assert_eq!(read_limited(&path, 10).unwrap(), None);
        fs::write(&path, b"0123456789").unwrap();
        assert_eq!(read_limited(&path, 10).unwrap().as_deref(), Some(&b"0123456789"[..]));
        assert_eq!(read_limited(&path, 9).unwrap_err().kind(), io::ErrorKind::InvalidData);
        // A directory in place of the file is an error, not "missing".
        assert!(read_limited(&dir, 10).is_err());
    }
}
