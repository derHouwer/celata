// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Recognizes a system on which DNS requests can bypass the VPN.
//!
//! The hide.me client writes the DNS servers of the tunnel into `/etc/resolv.conf` and does not know
//! `systemd-resolved`. Where that service answers the name lookups, it keeps asking the DNS servers of the
//! underlying network connection. Celata only looks at a few local places and never measures a leak: no
//! program is started, nothing is sent, nothing is written.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::files;

/// Largest file that is read; a larger one counts as unreadable.
const MAX_FILE: u64 = 64 * 1024;

/// Directory in which `systemd-resolved` keeps its files, relative to the root. It stays behind when the
/// service is stopped, so its existence alone does not say that the service runs.
const RESOLVED_DIR: &str = "run/systemd/resolve";

/// Directory in which systemd keeps one entry for every unit that currently runs, relative to the root.
const UNITS_DIR: &str = "run/systemd/units";

/// Entry in [`UNITS_DIR`] that exists exactly while `systemd-resolved` runs.
const RESOLVED_UNIT_ENTRY: &str = "invocation:systemd-resolved.service";

/// Addresses on which `systemd-resolved` listens for programs that read `/etc/resolv.conf`.
const STUB_ADDRESSES: [&str; 2] = ["127.0.0.53", "127.0.0.54"];

/// Answer to one observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Yes,
    /// Also the answer if the file or directory does not exist.
    No,
    /// The place exists but cannot be read, or the file is too large.
    Unreadable,
}

/// What Celata sees of the name resolution of this system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observation {
    /// Does `systemd-resolved` run? Its directory `/run/systemd/resolve` must exist, and where systemd lists
    /// its running units in `/run/systemd/units`, the service must be among them.
    pub resolved_running: Answer,
    /// Does the `hosts:` line of `/etc/nsswitch.conf` name `resolve`?
    pub nss_uses_resolve: Answer,
    /// Does `/etc/resolv.conf` point into `/run/systemd/resolve` or name one of the stub addresses?
    pub resolv_conf_is_stub: Answer,
}

/// Result for the hint in the overview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assessment {
    /// DNS requests can bypass the VPN: show the hint.
    AtRisk,
    /// Nothing points to `systemd-resolved` answering the lookups. This is not a promise of safety.
    Clear,
    /// Something needed could not be read: no hint.
    Unknown,
}

/// Rates an observation. Pure function without file access.
#[must_use]
pub fn assess(observation: &Observation) -> Assessment {
    use Answer::{No, Unreadable, Yes};
    match (
        observation.resolved_running,
        observation.nss_uses_resolve,
        observation.resolv_conf_is_stub,
    ) {
        (No, _, _) | (Yes, No, No) => Assessment::Clear,
        (Yes, Yes, _) | (Yes, _, Yes) => Assessment::AtRisk,
        (Unreadable | Yes, _, _) => Assessment::Unknown,
    }
}

/// Reads the places below `root` (`/` in the program, a prepared tree in tests).
#[must_use]
pub fn observe(root: &Path) -> Observation {
    let nss = read_text(&root.join("etc/nsswitch.conf"));
    Observation {
        resolved_running: resolved_runs(root),
        nss_uses_resolve: content_answer(nss, hosts_line_names_resolve),
        resolv_conf_is_stub: resolv_conf_is_stub(root),
    }
}

/// A stopped or masked `systemd-resolved` leaves its directory behind until the next restart of the system.
/// The list of running units tells the two apart. Without that list the directory alone decides.
fn resolved_runs(root: &Path) -> Answer {
    match is_directory(&root.join(RESOLVED_DIR)) {
        Answer::Yes => {}
        other => return other,
    }
    let units = root.join(UNITS_DIR);
    match is_directory(&units) {
        Answer::Yes => exists(&units.join(RESOLVED_UNIT_ENTRY)),
        Answer::No => Answer::Yes,
        Answer::Unreadable => Answer::Unreadable,
    }
}

/// Is there anything at `path`? A link counts even if its target does not exist.
fn exists(path: &Path) -> Answer {
    match fs::symlink_metadata(path) {
        Ok(_) => Answer::Yes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Answer::No,
        Err(_) => Answer::Unreadable,
    }
}

fn is_directory(path: &Path) -> Answer {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Answer::Yes,
        Ok(_) => Answer::No,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Answer::No,
        Err(_) => Answer::Unreadable,
    }
}

/// Content of a file as text, `Ok(None)` if it does not exist. Bytes that are not UTF-8 are replaced.
fn read_text(path: &Path) -> io::Result<Option<String>> {
    Ok(files::read_limited(path, MAX_FILE)?.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
}

fn content_answer(content: io::Result<Option<String>>, applies: fn(&str) -> bool) -> Answer {
    match content {
        Ok(Some(content)) if applies(&content) => Answer::Yes,
        Ok(_) => Answer::No,
        Err(_) => Answer::Unreadable,
    }
}

fn resolv_conf_is_stub(root: &Path) -> Answer {
    let path = root.join("etc/resolv.conf");
    match fs::read_link(&path) {
        Ok(target) if points_into_resolved(root, &target) => return Answer::Yes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Answer::No,
        // Not a link, or a link to somewhere else: the content decides, and reading it reports any error.
        _ => {}
    }
    content_answer(read_text(&path), names_stub_address)
}

/// Does the target of the link `<root>/etc/resolv.conf` lie in the directory of `systemd-resolved`?
fn points_into_resolved(root: &Path, target: &Path) -> bool {
    let resolved = normalize(&root.join(RESOLVED_DIR));
    if target.is_absolute() {
        let target = normalize(target);
        target.starts_with(Path::new("/").join(RESOLVED_DIR)) || target.starts_with(&resolved)
    } else {
        normalize(&root.join("etc").join(target)).starts_with(&resolved)
    }
}

/// Does the first line that starts with `hosts:` name the word `resolve`? Comments do not count.
fn hosts_line_names_resolve(content: &str) -> bool {
    content
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim_start())
        .find_map(|line| line.strip_prefix("hosts:"))
        .is_some_and(|sources| sources.split_whitespace().any(|word| word == "resolve"))
}

/// Does a `nameserver` line name one of the stub addresses? Comments do not count.
fn names_stub_address(content: &str) -> bool {
    content.lines().any(|line| {
        let mut words = line.split(['#', ';']).next().unwrap_or_default().split_whitespace();
        words.next() == Some("nameserver") && words.next().is_some_and(|address| STUB_ADDRESSES.contains(&address))
    })
}

/// `path` without `.` and `..`, resolved by name only.
fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU32, Ordering};

    use Answer::{No, Unreadable, Yes};
    use Assessment::{AtRisk, Clear, Unknown};

    const UBUNTU_HOSTS: &str = "passwd: files systemd\nhosts:          files mdns4_minimal [NOTFOUND=return] dns\n";
    const FEDORA_HOSTS: &str = "# Generated by authselect\nhosts:      files myhostname mdns4_minimal [NOTFOUND=return] resolve [!UNAVAIL=return] dns\n";
    const PLAIN_RESOLV: &str = "# Generated by NetworkManager\nnameserver 192.0.2.1\n";

    /// An empty directory that stands for `/`.
    fn tree(name: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let number = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("celata-dnsleak-{}-{number}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("etc")).unwrap();
        root
    }

    fn with_resolved(root: &Path) {
        fs::create_dir_all(root.join(RESOLVED_DIR)).unwrap();
    }

    fn write(root: &Path, file: &str, content: &str) {
        fs::write(root.join(file), content).unwrap();
    }

    fn observation(resolved_running: Answer, nss_uses_resolve: Answer, resolv_conf_is_stub: Answer) -> Observation {
        Observation {
            resolved_running,
            nss_uses_resolve,
            resolv_conf_is_stub,
        }
    }

    #[test]
    fn assessment_follows_the_table() {
        let all = [Yes, No, Unreadable];
        for nss in all {
            for stub in all {
                assert_eq!(assess(&observation(No, nss, stub)), Clear, "{nss:?} {stub:?}");
                assert_eq!(assess(&observation(Unreadable, nss, stub)), Unknown, "{nss:?} {stub:?}");
            }
        }
        for other in all {
            assert_eq!(assess(&observation(Yes, Yes, other)), AtRisk, "{other:?}");
            assert_eq!(assess(&observation(Yes, other, Yes)), AtRisk, "{other:?}");
        }
        assert_eq!(assess(&observation(Yes, No, No)), Clear);
        assert_eq!(assess(&observation(Yes, Unreadable, No)), Unknown);
        assert_eq!(assess(&observation(Yes, Unreadable, Unreadable)), Unknown);
        assert_eq!(assess(&observation(Yes, No, Unreadable)), Unknown);
    }

    #[test]
    fn ubuntu_is_at_risk_through_the_link() {
        let root = tree("ubuntu");
        with_resolved(&root);
        write(&root, "etc/nsswitch.conf", UBUNTU_HOSTS);
        symlink("../run/systemd/resolve/stub-resolv.conf", root.join("etc/resolv.conf")).unwrap();
        assert_eq!(observe(&root), observation(Yes, No, Yes));
        assert_eq!(assess(&observe(&root)), AtRisk);
    }

    #[test]
    fn fedora_is_at_risk_through_nsswitch() {
        let root = tree("fedora");
        with_resolved(&root);
        write(&root, "etc/nsswitch.conf", FEDORA_HOSTS);
        write(&root, "etc/resolv.conf", PLAIN_RESOLV);
        assert_eq!(observe(&root), observation(Yes, Yes, No));
        assert_eq!(assess(&observe(&root)), AtRisk);
    }

    /// `systemd-resolved` was stopped after it had run: its directory is still there, but systemd no longer
    /// lists the unit as running. Seen on a real system, where the hint stayed until this was checked.
    #[test]
    fn a_stopped_resolved_is_clear_although_its_directory_stays() {
        let root = tree("stopped");
        with_resolved(&root);
        write(&root, "run/systemd/resolve/stub-resolv.conf", "nameserver 127.0.0.53\n");
        fs::create_dir_all(root.join(UNITS_DIR)).unwrap();
        symlink(
            "0123456789abcdef",
            root.join(UNITS_DIR).join("invocation:other.service"),
        )
        .unwrap();
        write(&root, "etc/nsswitch.conf", FEDORA_HOSTS);
        write(&root, "etc/resolv.conf", PLAIN_RESOLV);
        assert_eq!(observe(&root), observation(No, Yes, No));
        assert_eq!(assess(&observe(&root)), Clear);

        // The service runs again: systemd lists it, the hint is back.
        symlink("fedcba9876543210", root.join(UNITS_DIR).join(RESOLVED_UNIT_ENTRY)).unwrap();
        assert_eq!(observe(&root), observation(Yes, Yes, No));
        assert_eq!(assess(&observe(&root)), AtRisk);
    }

    /// The list of running units cannot be read: no statement, so no hint.
    #[test]
    fn an_unreadable_list_of_units_is_unknown() {
        let root = tree("units-unreadable");
        with_resolved(&root);
        fs::create_dir_all(root.join("run/systemd")).unwrap();
        // A link to itself: asking what it points to fails with an error other than "not found".
        symlink("units", root.join(UNITS_DIR)).unwrap();
        write(&root, "etc/nsswitch.conf", FEDORA_HOSTS);
        assert_eq!(observe(&root).resolved_running, Unreadable);
        assert_eq!(assess(&observe(&root)), Unknown);
    }

    /// `systemd-resolved` is masked: the directory is missing, `nsswitch.conf` still names `resolve`.
    #[test]
    fn a_masked_resolved_is_clear() {
        let root = tree("masked");
        write(&root, "etc/nsswitch.conf", FEDORA_HOSTS);
        write(&root, "etc/resolv.conf", PLAIN_RESOLV);
        assert_eq!(observe(&root), observation(No, Yes, No));
        assert_eq!(assess(&observe(&root)), Clear);
    }

    #[test]
    fn debian_without_resolved_is_clear() {
        let root = tree("debian");
        write(&root, "etc/nsswitch.conf", "hosts:          files dns\n");
        write(&root, "etc/resolv.conf", PLAIN_RESOLV);
        assert_eq!(observe(&root), observation(No, No, No));
        assert_eq!(assess(&observe(&root)), Clear);
    }

    #[test]
    fn missing_files_count_as_no() {
        let root = tree("empty");
        assert_eq!(observe(&root), observation(No, No, No));
        with_resolved(&root);
        assert_eq!(observe(&root), observation(Yes, No, No));
        assert_eq!(assess(&observe(&root)), Clear);
        // A link whose target does not exist and lies elsewhere.
        symlink("/nonexistent/celata/resolv.conf", root.join("etc/resolv.conf")).unwrap();
        assert_eq!(observe(&root).resolv_conf_is_stub, No);
    }

    /// A directory at the place of the file: opening works, reading fails. Withdrawing the read permission
    /// would not do, because the tests run as root during a package build.
    #[test]
    fn an_unreadable_nsswitch_is_unknown() {
        let root = tree("unreadable-nss");
        with_resolved(&root);
        fs::create_dir(root.join("etc/nsswitch.conf")).unwrap();
        write(&root, "etc/resolv.conf", PLAIN_RESOLV);
        assert_eq!(observe(&root), observation(Yes, Unreadable, No));
        assert_eq!(assess(&observe(&root)), Unknown);
    }

    #[test]
    fn an_unreadable_resolv_conf_is_unknown() {
        let root = tree("unreadable-resolv");
        with_resolved(&root);
        write(&root, "etc/nsswitch.conf", UBUNTU_HOSTS);
        fs::create_dir(root.join("etc/resolv.conf")).unwrap();
        assert_eq!(observe(&root), observation(Yes, No, Unreadable));
        assert_eq!(assess(&observe(&root)), Unknown);
    }

    #[test]
    fn a_file_at_the_place_of_the_directory_is_not_a_running_resolved() {
        let root = tree("file-as-dir");
        fs::create_dir_all(root.join("run/systemd")).unwrap();
        write(&root, RESOLVED_DIR, "");
        assert_eq!(observe(&root).resolved_running, No);
    }

    #[test]
    fn files_over_64_kib_are_unreadable() {
        let root = tree("large");
        with_resolved(&root);
        let padding = "#".repeat(64 * 1024);
        write(&root, "etc/nsswitch.conf", &format!("{FEDORA_HOSTS}{padding}"));
        write(&root, "etc/resolv.conf", &format!("nameserver 127.0.0.53\n{padding}"));
        assert_eq!(observe(&root), observation(Yes, Unreadable, Unreadable));
        // Exactly 64 KiB are still read.
        let fitting = format!("nameserver 127.0.0.53\n{padding}");
        write(&root, "etc/resolv.conf", &fitting[..64 * 1024]);
        assert_eq!(observe(&root).resolv_conf_is_stub, Yes);
    }

    #[test]
    fn resolve_counts_only_as_a_word_of_the_first_hosts_line() {
        assert!(hosts_line_names_resolve(FEDORA_HOSTS));
        assert!(hosts_line_names_resolve("hosts:resolve\n"));
        assert!(hosts_line_names_resolve("  hosts:\tfiles\tresolve\n"));
        assert!(!hosts_line_names_resolve(UBUNTU_HOSTS));
        assert!(!hosts_line_names_resolve(""));
        // Only in a comment.
        assert!(!hosts_line_names_resolve(
            "# hosts: files resolve dns\nhosts: files dns\n"
        ));
        assert!(!hosts_line_names_resolve("hosts: files dns # resolve\n"));
        // Only in another line.
        assert!(!hosts_line_names_resolve(
            "passwd: files resolve\nhosts: files dns\nhostsx: resolve\n"
        ));
        // Only as part of another word.
        assert!(!hosts_line_names_resolve(
            "hosts: files myresolve resolved [resolve=return]\n"
        ));
        // Only the first `hosts:` line counts.
        assert!(!hosts_line_names_resolve("hosts: files dns\nhosts: resolve\n"));
    }

    #[test]
    fn stub_addresses_count_only_in_nameserver_lines() {
        assert!(names_stub_address(
            "nameserver 127.0.0.53\noptions edns0 trust-ad\nsearch .\n"
        ));
        assert!(names_stub_address("nameserver 192.0.2.1\n\tnameserver\t127.0.0.54\n"));
        assert!(!names_stub_address(PLAIN_RESOLV));
        assert!(!names_stub_address(""));
        // Only in a comment.
        assert!(!names_stub_address(
            "# nameserver 127.0.0.53\n; nameserver 127.0.0.54\nnameserver 192.0.2.1\n"
        ));
        assert!(!names_stub_address("nameserver 192.0.2.1 # 127.0.0.53\n"));
        // Other addresses and other lines.
        assert!(!names_stub_address(
            "nameserver 127.0.0.530\nnameserver 127.0.0.5\nsearch 127.0.0.53\n"
        ));
    }

    #[test]
    fn resolv_conf_as_a_link_into_the_resolved_directory_is_a_stub() {
        for target in [
            "/run/systemd/resolve/stub-resolv.conf",
            "/run/systemd/resolve/resolv.conf",
            "../run/./systemd/resolve/resolv.conf",
        ] {
            let root = tree("link");
            symlink(target, root.join("etc/resolv.conf")).unwrap();
            assert_eq!(observe(&root).resolv_conf_is_stub, Yes, "{target}");
        }
        // A link elsewhere: the content of the target decides.
        let root = tree("link-elsewhere");
        write(&root, "etc/real.conf", PLAIN_RESOLV);
        symlink("real.conf", root.join("etc/resolv.conf")).unwrap();
        assert_eq!(observe(&root).resolv_conf_is_stub, No);
        write(&root, "etc/real.conf", "nameserver 127.0.0.53\n");
        assert_eq!(observe(&root).resolv_conf_is_stub, Yes);
        // A name that only looks similar.
        let root = tree("link-similar");
        symlink("/run/systemd/resolve-other/resolv.conf", root.join("etc/resolv.conf")).unwrap();
        assert_eq!(observe(&root).resolv_conf_is_stub, No);
    }

    #[test]
    fn normalize_resolves_by_name_only() {
        assert_eq!(normalize(Path::new("/etc/../run/./systemd")), Path::new("/run/systemd"));
        assert_eq!(normalize(Path::new("/../../etc")), Path::new("/etc"));
        assert_eq!(normalize(Path::new("/a/b/../../c")), Path::new("/c"));
    }

    #[test]
    fn observing_writes_nothing() {
        let root = tree("read-only");
        with_resolved(&root);
        write(&root, "etc/nsswitch.conf", FEDORA_HOSTS);
        let listing = |dir: &str| {
            let mut names: Vec<_> = fs::read_dir(root.join(dir))
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            names.sort();
            names
        };
        let before = (listing("etc"), listing(RESOLVED_DIR));
        let _ = observe(&root);
        assert_eq!((listing("etc"), listing(RESOLVED_DIR)), before);
    }
}
