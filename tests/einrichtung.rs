// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Tests of the setup script `packaging/setup/celata-setup.sh`.
//!
//! The script is loaded with `source` (it then only defines functions and tables and runs nothing) and its
//! pure functions are called with example files from `tests/fixtures/`. Parts that touch the system are
//! replaced by small functions in the test itself. Nothing here runs `sudo`, writes below `/etc`, `/usr` or
//! `/var`, or starts a service. Some tests read the source text of the script to hold rules that must never
//! be broken.
#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn script_path() -> PathBuf {
    root().join("packaging/setup/celata-setup.sh")
}

fn script_text() -> String {
    fs::read_to_string(script_path()).expect("the setup script is readable")
}

struct Run {
    out: String,
    err: String,
    code: i32,
}

fn finish(child: std::process::Child, stdin: &str) -> Run {
    let mut child = child;
    if let Some(mut pipe) = child.stdin.take() {
        // The script may end before it reads everything; a broken pipe is fine.
        let _ = pipe.write_all(stdin.as_bytes());
    }
    let output = child.wait_with_output().expect("bash finishes");
    Run {
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    }
}

fn base_command() -> Command {
    let mut command = Command::new("bash");
    command
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", "/nonexistent")
        .env("LC_ALL", "C")
        .env("SCRIPT", script_path())
        .env("FIXTURES", root().join("tests/fixtures"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// Loads the script and runs a snippet of bash after it (errexit is switched off to read return values).
fn run_bash(snippet: &str, env: &[(&str, &str)], stdin: &str) -> Run {
    let mut command = base_command();
    command.arg("-c").arg(format!("source \"$SCRIPT\"; set +e; {snippet}"));
    for (key, value) in env {
        command.env(key, value);
    }
    finish(command.spawn().expect("bash starts"), stdin)
}

fn bash(snippet: &str) -> Run {
    run_bash(snippet, &[], "")
}

/// Output of a snippet without the trailing line break.
fn out(snippet: &str) -> String {
    let run = bash(snippet);
    assert_eq!(run.code, 0, "{snippet}\nstderr: {}", run.err);
    run.out.trim_end_matches('\n').to_owned()
}

/// Runs the script itself with arguments.
fn run_script(args: &[&str], env: &[(&str, &str)], stdin: &str) -> Run {
    let mut command = base_command();
    command.arg(script_path()).args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    finish(command.spawn().expect("bash starts"), stdin)
}

/// A folder below the temporary folder that is removed at the end of the test.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("celata-einrichtung-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temporary folder");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn s(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// --- Source text of the script ------------------------------------------------------------------------------

const BEGIN_TEXTS: &str = "# BEGIN TEXTS";
const END_TEXTS: &str = "# END TEXTS";

/// The script without the block of texts. Fails if one of the two marker lines is missing or appears twice.
fn code_without_texts() -> String {
    let text = script_text();
    assert_eq!(text.matches(BEGIN_TEXTS).count(), 1, "exactly one `{BEGIN_TEXTS}` line");
    assert_eq!(text.matches(END_TEXTS).count(), 1, "exactly one `{END_TEXTS}` line");
    let begin = text.find(BEGIN_TEXTS).unwrap();
    let end = text.find(END_TEXTS).unwrap() + END_TEXTS.len();
    assert!(begin < end, "the texts block starts before it ends");
    format!("{}{}", &text[..begin], &text[end..])
}

fn texts_block() -> String {
    let text = script_text();
    let begin = text.find(BEGIN_TEXTS).expect("block start");
    let end = text.find(END_TEXTS).expect("block end");
    text[begin..end].to_owned()
}

/// Entries `[key]="text"` of one table of the texts block.
fn table(name: &str) -> BTreeMap<String, String> {
    let block = texts_block();
    let start = block
        .find(&format!("declare -A {name}=("))
        .unwrap_or_else(|| panic!("table {name} exists"));
    let mut entries = BTreeMap::new();
    for line in block[start..].lines().skip(1) {
        if line.trim() == ")" {
            break;
        }
        let line = line.trim();
        let (key, value) = line
            .strip_prefix('[')
            .and_then(|rest| rest.split_once("]=\""))
            .unwrap_or_else(|| panic!("{name}: cannot read the entry {line}"));
        let value = value
            .strip_suffix('"')
            .unwrap_or_else(|| panic!("{name}: {key} does not end with a quote"));
        assert!(
            entries.insert(key.to_owned(), value.to_owned()).is_none(),
            "{name}: {key} twice"
        );
    }
    entries
}

/// Number of `%s` in a text; `%%` does not count.
fn placeholders(text: &str) -> usize {
    let mut count = 0;
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '%' {
            match chars.next() {
                Some('s') => count += 1,
                Some('%') => {}
                other => panic!("unexpected conversion %{other:?} in {text}"),
            }
        }
    }
    count
}

/// Text of a function: from the line `name() {` to the next line that is only `}`.
fn function_body(code: &str, name: &str) -> String {
    let header = format!("{name}() {{");
    let mut inside = false;
    let mut body = String::new();
    for line in code.lines() {
        if line == header {
            inside = true;
        }
        if inside {
            body.push_str(line);
            body.push('\n');
            if line == "}" {
                return body;
            }
        }
    }
    panic!("function {name} not found");
}

/// Lines of code that are not comments.
fn code_lines(code: &str) -> Vec<(usize, &str)> {
    code.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with('#'))
        .map(|(number, line)| (number + 1, line))
        .collect()
}

/// Keys of messages used in the code: exact keys and prefixes of keys that are put together at run time.
fn used_keys(code: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let markers = [
        "say ",
        "say_err ",
        "fmt ",
        "usage_error ",
        "ask ",
        "die \"$RC_ABORT_CLEAN\" ",
    ];
    let mut exact = BTreeSet::new();
    let mut prefixes = BTreeSet::new();
    for (_, line) in code_lines(code) {
        for marker in markers {
            let mut rest = line;
            while let Some(position) = rest.find(marker) {
                // The marker must start a word: `say ` inside `say_err ` or `ask ` inside `task ` do not count.
                let before = rest[..position].chars().last();
                let after = &rest[position + marker.len()..];
                rest = after;
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-') {
                    continue;
                }
                let word = after.trim_start_matches('"');
                let name: String = word
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_' || *c == '-')
                    .collect();
                if name.is_empty() || !name.contains('_') && !word[name.len()..].starts_with('$') && name.len() < 3 {
                    continue;
                }
                if word[name.len()..].starts_with('$') || name.ends_with('_') {
                    prefixes.insert(name);
                } else {
                    exact.insert(name);
                }
            }
        }
    }
    (exact, prefixes)
}

// --- Scaffold -----------------------------------------------------------------------------------------------

#[test]
fn loading_the_script_runs_nothing() {
    let run = bash("true");
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(run.out, "", "loading with `source` must not print");
    assert_eq!(run.err, "");
}

#[test]
fn script_is_executable_and_starts_like_the_other_sources() {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(script_path()).unwrap().permissions().mode();
    assert_eq!(mode & 0o111, 0o111, "the script is executable");
    let text = script_text();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("#!/usr/bin/env bash"));
    assert_eq!(lines.next(), Some("# SPDX-License-Identifier: GPL-3.0-only"));
    assert_eq!(lines.next(), Some("# Copyright (C) 2026 derHouwer"));
    assert!(text.contains("\nset -Eeuo pipefail\n"), "strict mode");
}

#[test]
fn version_and_help_answer_without_touching_the_system() {
    let version = run_script(&["--version"], &[], "");
    assert_eq!(
        (version.code, version.out.trim()),
        (0, "celata-setup 0.9.0-1"),
        "{}",
        version.err
    );
    let help = run_script(&["--help"], &[], "");
    assert_eq!(help.code, 0, "{}", help.err);
    for option in [
        "--yes",
        "--user",
        "--client",
        "--check",
        "--dry-run",
        "--uninstall",
        "--help",
        "--version",
    ] {
        assert!(help.out.contains(option), "the help names {option}");
    }
    let german = run_script(&["--help"], &[("LC_ALL", "de_DE.UTF-8")], "");
    assert!(german.out.contains("Richtet Celata ein"), "German help: {}", german.out);
}

#[test]
fn wrong_calls_end_with_return_value_2_and_a_short_help_on_stderr() {
    let calls: &[&[&str]] = &[
        &["--check", "--dry-run"],
        &["--check", "--uninstall"],
        &["--check", "--yes"],
        &["--uninstall", "--client", "/usr/bin/hide.me"],
        &["--bogus"],
        &["setup"],
        &["--yes", "--yes"],
        &["--check", "--check"],
        &["--user"],
        &["--user", "--yes"],
        &["--user", "ro ot"],
        &["--client"],
        &["--client", "relative/path"],
        &["--user", "anna", "--user", "bob"],
    ];
    for call in calls {
        let run = run_script(call, &[], "");
        assert_eq!(
            run.code, 2,
            "{call:?} should be a wrong call; stdout: {} stderr: {}",
            run.out, run.err
        );
        assert_eq!(run.out, "", "{call:?}: nothing on stdout");
        assert!(
            run.err.contains("Usage:"),
            "{call:?}: the short help is on stderr: {}",
            run.err
        );
    }
    let german = run_script(&["--bogus"], &[("LC_ALL", "de_DE.UTF-8")], "");
    assert_eq!(german.code, 2);
    assert!(german.err.contains("Unbekannte Option"), "{}", german.err);
}

#[test]
fn valid_combinations_are_accepted() {
    let calls: &[&[&str]] = &[
        &["--dry-run", "--user", "anna", "--client", "/usr/local/bin/hide.me"],
        &["--uninstall", "--yes", "--dry-run"],
        &["--check", "--user", "anna", "--client", "/opt/hide.me/hide.me"],
        &["--yes", "--user", "anna"],
    ];
    for call in calls {
        let snippet = format!(
            "parse_args {}; echo \"rc=$?\"",
            call.iter().map(|a| format!("'{a}'")).collect::<Vec<_>>().join(" ")
        );
        assert_eq!(out(&snippet), "rc=0", "{call:?}");
    }
    assert_eq!(
        out("parse_args --dry-run --user anna --client /x/hide.me; echo \"$OPT_DRY $OPT_USER $OPT_CLIENT\""),
        "1 anna /x/hide.me"
    );
}

#[test]
fn both_tables_have_the_same_keys_and_placeholders() {
    let english = table("TEXT_EN");
    let german = table("TEXT_DE");
    let en_keys: BTreeSet<_> = english.keys().collect();
    let de_keys: BTreeSet<_> = german.keys().collect();
    assert_eq!(en_keys, de_keys, "TEXT_EN and TEXT_DE need the same keys");
    for (key, text) in &english {
        assert!(!text.is_empty() && !german[key].is_empty(), "{key} is empty");
        assert_eq!(
            placeholders(text),
            placeholders(&german[key]),
            "{key}: different number of %s"
        );
    }
    assert!(english.len() >= 150, "only {} texts", english.len());
}

#[test]
fn every_key_used_in_the_code_exists_and_every_key_is_used() {
    let code = code_without_texts();
    let english = table("TEXT_EN");
    let (exact, prefixes) = used_keys(&code);
    for key in &exact {
        assert!(
            english.contains_key(key),
            "the code uses the message `{key}`, which has no text"
        );
    }
    for prefix in &prefixes {
        assert!(
            english.keys().any(|key| key.starts_with(prefix.as_str())),
            "the code builds message keys that start with `{prefix}`, but no text does"
        );
    }
    let dynamic = ["chk_name_", "step_", "reason_", "choice_", "chk_package_ok_"];
    for key in english.keys() {
        let used = code.contains(key.as_str()) || dynamic.iter().any(|prefix| key.starts_with(prefix));
        assert!(used, "the text `{key}` is never used");
    }
    assert!(exact.len() >= 100, "only {} keys were found in the code", exact.len());
}

#[test]
fn every_step_and_finding_has_a_text() {
    let english = table("TEXT_EN");
    let steps = [
        "package-install",
        "package-update",
        "package-reinstall",
        "package-remove",
        "dropin-write",
        "dropin-remove",
        "dropin-delete",
        "reload",
        "group-create",
        "group-add",
        "service-stop",
        "state-write",
        "state-delete",
    ];
    for step in steps {
        let key = format!("step_{}", step.replace('-', "_"));
        assert!(english.contains_key(&key), "no text for the step {step}");
    }
    let ids = out("printf '%s\\n' \"${CHECK_ORDER[@]}\"");
    assert_eq!(ids.lines().count(), 14, "14 findings");
    for id in ids.lines() {
        assert!(english.contains_key(&format!("chk_name_{id}")), "no name for {id}");
    }
    for reason in [
        "architecture",
        "no-systemd",
        "atomic",
        "opensuse",
        "old-release",
        "unknown",
    ] {
        assert!(
            english.contains_key(&format!("reason_{reason}")),
            "no text for the reason {reason}"
        );
    }
}

#[test]
fn plan_texts_are_short_before_the_command() {
    for name in ["TEXT_EN", "TEXT_DE"] {
        for (key, text) in table(name) {
            if !key.starts_with("step_") || text.contains("%s: ") || text.starts_with("%s") {
                continue;
            }
            let Some(activity) = text.strip_suffix(" (%s)") else {
                continue;
            };
            assert!(
                activity.chars().count() <= 80,
                "{name} {key}: {} characters before the command",
                activity.chars().count()
            );
        }
    }
}

#[test]
fn plan_texts_have_the_form_activity_and_command() {
    for name in ["TEXT_EN", "TEXT_DE"] {
        let entries = table(name);
        for step in [
            "package_install",
            "dropin_write",
            "group_add",
            "state_write",
            "service_stop",
        ] {
            let text = &entries[&format!("step_{step}")];
            assert!(
                text.ends_with(" (%s)"),
                "{name} step_{step}: `{text}` is not `<activity> (<command>)`"
            );
        }
    }
}

// --- Rules that must hold in the source text -----------------------------------------------------------------

/// Is `needle` found in the line as a whole word (not inside a longer name)?
fn has_word(line: &str, needle: &str) -> bool {
    line.match_indices(needle).any(|(position, _)| {
        let before = line[..position].chars().last();
        let after = line[position + needle.len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
            && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

#[test]
fn the_access_token_is_only_ever_tested_for_size() {
    let code = code_without_texts();
    let mut found = 0;
    for (number, line) in code_lines(&code) {
        if line.contains("accessToken") {
            found += 1;
            assert!(
                line.contains("[ -s \"$1/accessToken.txt\" ]"),
                "line {number}: accessToken may only appear in a `[ -s … ]` test: {line}"
            );
        }
    }
    assert_eq!(found, 1, "the Access-Token file is named exactly once");
    for word in ["cat ", "head ", "tail ", "cp ", "mv ", "base64", "xxd", "od "] {
        for (number, line) in code_lines(&code) {
            assert!(
                !(line.contains(word) && line.to_lowercase().contains("token")),
                "line {number}: a command that could read the token ({word}): {line}"
            );
        }
    }
}

#[test]
fn nothing_writes_dns_settings() {
    let code = code_without_texts();
    for (number, line) in code_lines(&code) {
        assert!(
            !line.contains("resolv.conf"),
            "line {number}: resolv.conf is not touched by the script: {line}"
        );
        if line.contains("systemd-resolved") {
            assert!(
                line.contains("is-active"),
                "line {number}: systemd-resolved may only be asked with is-active: {line}"
            );
        }
        assert!(
            !(line.contains("NetworkManager") || line.contains("nmcli")),
            "line {number}: NetworkManager is not touched by the script: {line}"
        );
    }
}

#[test]
fn only_the_release_page_is_ever_contacted() {
    let code = code_without_texts();
    for (number, line) in code_lines(&code) {
        for scheme in ["http://", "https://", "ftp://"] {
            for (position, _) in line.match_indices(scheme) {
                let address = &line[position..];
                let allowed = address.starts_with("https://github.com/derHouwer/celata/releases/download/")
                    || (address.starts_with("http://localhost/version")
                        && line.contains("--unix-socket \"$CONTROL_SOCKET\""));
                assert!(
                    allowed,
                    "line {number}: address that is not allowed in the code: {line}"
                );
            }
        }
    }
    for (name, entries) in [("TEXT_EN", table("TEXT_EN")), ("TEXT_DE", table("TEXT_DE"))] {
        for (key, text) in entries {
            for (position, _) in text.match_indices("http") {
                let address = &text[position..];
                assert!(
                    address.starts_with("https://github.com/eventure/hide.client.linux")
                        || address.starts_with("https://github.com/derHouwer/celata/"),
                    "{name} {key}: address that is not allowed in a text: {text}"
                );
            }
        }
    }
}

#[test]
fn downloaded_files_are_never_run_and_nothing_is_piped_into_a_shell() {
    let code = code_without_texts();
    for (number, line) in code_lines(&code) {
        for pattern in [
            "| sh",
            "| bash",
            "|sh",
            "|bash",
            "| sudo",
            "eval ",
            "bash -c",
            "sh -c",
            "sudo bash",
            "sudo sh",
        ] {
            assert!(
                !line.contains(pattern),
                "line {number}: `{pattern}` is not allowed: {line}"
            );
        }
        assert!(
            !(has_word(line, "source") || line.trim_start().starts_with(". ")),
            "line {number}: the script never loads another file with source: {line}"
        );
    }
    let fetch = function_body(&code, "fetch");
    assert!(
        fetch.contains("--proto =https --proto-redir =https --tlsv1.2 -fL"),
        "curl takes only https"
    );
    assert!(fetch.contains("wget --https-only"), "wget takes only https");
}

#[test]
fn the_service_is_never_started_enabled_or_restarted() {
    let code = code_without_texts();
    for (number, line) in code_lines(&code) {
        for verb in [
            "enable", "start", "restart", "reload", "disable", "mask", "unmask", "kill",
        ] {
            for after in line.split("systemctl ").skip(1) {
                let word = after.split_whitespace().next().unwrap_or("");
                assert_ne!(word, verb, "line {number}: systemctl {verb} is not allowed: {line}");
            }
        }
    }
    let stops: Vec<_> = code_lines(&code)
        .into_iter()
        .filter(|(_, line)| line.contains("as_root systemctl stop"))
        .collect();
    assert_eq!(stops.len(), 1, "systemctl stop appears once: {stops:?}");
    assert!(
        function_body(&code, "step_service_stop").contains("systemctl stop \"$UNIT\""),
        "systemctl stop only in the function of the step service-stop"
    );
    let callers: Vec<_> = code_lines(&code)
        .into_iter()
        .filter(|(_, line)| line.contains("step_service_stop") && !line.contains("step_service_stop()"))
        .collect();
    assert_eq!(
        callers.len(),
        1,
        "step_service_stop is called from one place only: {callers:?}"
    );
    assert!(callers[0].1.contains("service-stop"), "{callers:?}");
}

#[test]
fn administrator_rights_are_only_used_through_as_root() {
    let code = code_without_texts();
    for (number, line) in code_lines(&code) {
        for (position, _) in line.match_indices("sudo") {
            let before = line[..position].chars().last();
            let after = &line[position + 4..];
            let part_of_a_name = before.is_some_and(|c| c.is_alphanumeric() || c == '_') || after.starts_with('_');
            let allowed = part_of_a_name
                || after.starts_with(" -- ")
                || after.starts_with(" -n ")
                || after.starts_with('\'')
                || line.contains("command -v sudo")
                || line.contains("say_err")
                || line.contains("say remains_")
                || line.contains("printf '%q ' sudo")
                || line.contains("printf ' %q'");
            assert!(
                allowed,
                "line {number}: sudo only in as_root and the help functions: {line}"
            );
        }
    }
    let as_root = function_body(&code, "as_root");
    assert!(
        as_root.contains("sudo -- \"$@\""),
        "as_root runs one command at a time through sudo"
    );
    for (number, line) in code_lines(&code) {
        if line.contains("useradd") || line.contains("passwd") || line.contains("chmod -R") || line.contains("chown") {
            assert!(
                !line.contains("as_root"),
                "line {number}: the script never changes rights of the client: {line}"
            );
        }
    }
}

#[test]
fn the_script_never_writes_to_the_client_folder_or_the_package_files() {
    let code = code_without_texts();
    let drop = function_body(&code, "render_dropin");
    assert!(
        drop.contains("ReadWritePaths=$folder /etc -/run/systemd/resolve"),
        "the whole of /etc stays writable"
    );
    assert!(!drop.contains("/etc/resolv.conf"));
    for (number, line) in code_lines(&code) {
        assert!(
            !line.contains("override.conf"),
            "line {number}: override.conf is never written or deleted: {line}"
        );
        assert!(
            !line.contains("/etc/polkit-1/rules.d/49-celata.rules\" >"),
            "line {number}: the rule is never written: {line}"
        );
    }
    assert!(code.contains("DROPIN_FILE=$DROPIN_DIR/50-celata-setup.conf"));
}

#[test]
fn comments_name_no_requirements_or_planning_documents() {
    for (number, line) in script_text().lines().enumerate() {
        let Some(comment) = line.trim_start().strip_prefix('#') else {
            continue;
        };
        // Put together here, so that this file does not name a planning document itself.
        let patterns = [
            "FR-".to_owned(),
            "SC-".to_owned(),
            format!("{}/", "specs"),
            format!("{}.md", "research"),
            format!("{}.md", "plan"),
            format!("{}.md", "spec"),
            format!("{}.md", "tasks"),
        ];
        for pattern in &patterns {
            assert!(
                !comment.contains(pattern.as_str()),
                "line {}: `{pattern}` in a comment: {line}",
                number + 1
            );
        }
    }
}

#[test]
fn the_rule_checksum_matches_the_rule_of_the_package() {
    let rule = fs::read(root().join("packaging/polkit/49-celata.rules")).unwrap();
    let digest = Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child.stdin.take().unwrap().write_all(&rule)?;
            child.wait_with_output()
        })
        .expect("sha256sum runs");
    let sum = String::from_utf8_lossy(&digest.stdout);
    let sum = sum.split_whitespace().next().unwrap();
    assert_eq!(
        out("printf %s \"$CELATA_RULE_SHA256\""),
        sum,
        "update CELATA_RULE_SHA256 after changing the rule"
    );
}

#[test]
fn the_version_in_the_script_is_the_version_of_the_project() {
    let manifest = fs::read_to_string(root().join("Cargo.toml")).unwrap();
    let version = manifest
        .lines()
        .skip_while(|line| line.trim() != "[workspace.package]")
        .find_map(|line| {
            line.trim()
                .strip_prefix("version = \"")?
                .strip_suffix('"')
                .map(str::to_owned)
        })
        .expect("version in [workspace.package]");
    assert_eq!(out("printf %s \"$CELATA_VERSION\""), version);
    assert_eq!(out("printf %s \"$CELATA_PKGREL\""), "1");
}

// --- Systems ---------------------------------------------------------------------------------------------------

/// `kind|base|base_version|family|reason` for an os-release file and the facts of a normal system.
fn classify(file: &str, extra: &str) -> String {
    out(&format!(
        "HAS_SYSTEMD=1; IS_OSTREE=0; MACHINE=x86_64; {extra}; read_os_release \"$FIXTURES/os-release/{file}\"; classify; \
         echo \"$CL_KIND|$CL_BASE|$CL_BASE_VERSION|$CL_FAMILY|$CL_REASON\""
    ))
}

#[test]
fn the_eight_tested_systems_are_recognised() {
    let tested = [
        ("fedora-44", "tested|fedora|44|rpm|"),
        ("fedora-45", "tested|fedora|45|rpm|"),
        ("debian-13", "tested|debian|13|deb|"),
        ("ubuntu-24.04", "tested|ubuntu|24.04|deb|"),
        ("ubuntu-26.04", "tested|ubuntu|26.04|deb|"),
        ("ubuntu-26.10", "tested|ubuntu|26.10|deb|"),
        ("linuxmint-22", "tested|ubuntu|24.04|deb|"),
        ("arch", "tested|arch||arch|"),
    ];
    for (file, expected) in tested {
        assert_eq!(classify(file, "true"), expected, "{file}");
    }
}

#[test]
fn derivatives_are_assigned_to_their_base_and_marked_untested() {
    let derivatives = [
        ("nobara-44", "derivative|fedora|44|rpm|"),
        ("popos-24.04", "derivative|ubuntu|24.04|deb|"),
        ("zorin-18", "derivative|ubuntu|24.04|deb|"),
        ("manjaro", "derivative|arch||arch|"),
        ("endeavouros", "derivative|arch||arch|"),
    ];
    for (file, expected) in derivatives {
        assert_eq!(classify(file, "true"), expected, "{file}");
    }
}

#[test]
fn everything_else_is_refused_with_a_reason() {
    let refused = [
        ("debian-12", "unsupported|debian|12|deb|old-release"),
        ("ubuntu-22.04", "unsupported|ubuntu|22.04|deb|old-release"),
        ("linuxmint-21", "unsupported|ubuntu|22.04|deb|old-release"),
        ("lmde-6", "unsupported|debian|12|deb|old-release"),
        ("fedora-43", "unsupported|fedora|43|rpm|old-release"),
        ("fedora-silverblue-44", "unsupported||||atomic"),
        ("bazzite", "unsupported||||atomic"),
        ("opensuse-tumbleweed", "unsupported||||opensuse"),
        ("opensuse-leap-15", "unsupported||||opensuse"),
        ("rhel-9", "unsupported||||unknown"),
        ("almalinux-9", "unsupported||||unknown"),
        ("kali", "unsupported|debian||deb|unknown"),
        ("alpine", "unsupported||||unknown"),
        ("unbekannt-ohne-id", "unsupported||||unknown"),
    ];
    for (file, expected) in refused {
        assert_eq!(classify(file, "true"), expected, "{file}");
    }
}

#[test]
fn the_exclusions_come_first_and_in_a_fixed_order() {
    assert_eq!(classify("fedora-44", "MACHINE=aarch64"), "unsupported||||architecture");
    assert_eq!(classify("fedora-44", "HAS_SYSTEMD=0"), "unsupported||||no-systemd");
    assert_eq!(classify("fedora-44", "IS_OSTREE=1"), "unsupported||||atomic");
    assert_eq!(
        classify("fedora-44", "MACHINE=aarch64; HAS_SYSTEMD=0; IS_OSTREE=1"),
        "unsupported||||architecture"
    );
    assert_eq!(
        classify("fedora-44", "HAS_SYSTEMD=0; IS_OSTREE=1"),
        "unsupported||||no-systemd"
    );
}

#[test]
fn os_release_is_read_as_text_and_never_run() {
    let marker = Path::new("/tmp/celata-os-release-was-run");
    let _ = fs::remove_file(marker);
    let result = out(
        "read_os_release \"$FIXTURES/os-release/injection\"; printf '%s|%s|%s' \"$OS_ID\" \"$OS_VERSION\" \"$OS_PRETTY\"",
    );
    assert!(!marker.exists(), "the commands inside the file were run");
    assert_eq!(
        result,
        "$(touch /tmp/celata-os-release-was-run)|`touch /tmp/celata-os-release-was-run`|a ; touch /tmp/celata-os-release-was-run"
    );
    assert_eq!(
        out(
            "read_os_release \"$FIXTURES/os-release/nobara-44\"; printf '%s|%s|%s|%s' \"$OS_ID\" \"$OS_LIKE\" \"$OS_PLATFORM\" \"$OS_CODENAME\""
        ),
        "nobara|rhel centos fedora|platform:f44|"
    );
}

#[test]
fn package_files_are_named_like_the_build_names_them() {
    assert_eq!(out("package_file deb"), "celata_0.9.0-1_amd64.deb");
    assert_eq!(out("package_file rpm"), "celata-0.9.0-1.x86_64.rpm");
    assert_eq!(out("package_file arch"), "celata-0.9.0-1-x86_64.pkg.tar.zst");
    let dist = root().join("dist");
    if dist.join("SHA256SUMS").exists() {
        let sums = fs::read_to_string(dist.join("SHA256SUMS")).unwrap();
        for family in ["deb", "rpm", "arch"] {
            let name = out(&format!("package_file {family}"));
            assert!(
                sums.lines().any(|line| line.ends_with(&format!("  {name}"))),
                "{name} is a file of the build"
            );
        }
    }
}

#[test]
fn versions_are_compared_like_sort_v() {
    let cases = [
        ("0.105", "0.106", false),
        ("0.106", "0.106", true),
        ("121", "0.106", true),
        ("124", "0.106", true),
        ("0.9.0-1", "0.9.0-2", false),
        ("0.9.0-2", "0.9.0-1", true),
        ("0.9", "0.10", false),
    ];
    for (have, need, expected) in cases {
        let run = bash(&format!("version_at_least '{have}' '{need}'"));
        assert_eq!(run.code == 0, expected, "{have} against {need}");
    }
}

#[test]
fn polkit_must_know_javascript_rules() {
    assert_eq!(bash("pkaction() { echo 'pkaction version 0.105'; }; polkit_ok").code, 1);
    assert_eq!(
        out("pkaction() { echo 'pkaction version 0.105'; }; polkit_ok; true"),
        "0.105"
    );
    for version in ["0.106", "121", "124"] {
        let run = bash(&format!(
            "pkaction() {{ echo 'pkaction version {version}'; }}; polkit_ok"
        ));
        assert_eq!((run.code, run.out.as_str()), (0, version), "polkit {version}");
    }
    assert_eq!(
        bash("PATH=/nonexistent; polkit_ok").code,
        0,
        "not installed counts as fine: the package brings it"
    );
}

#[test]
fn each_family_has_its_package_manager_commands() {
    let cases = [
        ("deb", "probe", "apt-get install --simulate /x/f.deb"),
        (
            "deb",
            "install",
            "env DEBIAN_FRONTEND=noninteractive apt-get install -y /x/f.deb",
        ),
        (
            "deb",
            "reinstall",
            "env DEBIAN_FRONTEND=noninteractive apt-get install -y --reinstall /x/f.deb",
        ),
        (
            "deb",
            "remove",
            "env DEBIAN_FRONTEND=noninteractive apt-get remove -y celata",
        ),
        ("deb", "probe-update", "apt-get update"),
        ("rpm", "probe", "dnf install --assumeno /x/f.rpm"),
        ("rpm", "install", "dnf install -y /x/f.rpm"),
        ("rpm", "reinstall", "dnf reinstall -y /x/f.rpm"),
        ("rpm", "remove", "dnf remove -y celata"),
        ("arch", "probe", "pacman -U --print /x/f.pkg"),
        ("arch", "install", "pacman -U --noconfirm --needed /x/f.pkg"),
        ("arch", "reinstall", "pacman -U --noconfirm /x/f.pkg"),
        ("arch", "remove", "pacman -R --noconfirm celata"),
    ];
    for (family, action, expected) in cases {
        let file = if family == "deb" {
            "/x/f.deb"
        } else if family == "rpm" {
            "/x/f.rpm"
        } else {
            "/x/f.pkg"
        };
        assert_eq!(
            out(&format!(
                "CL_FAMILY={family}; pm_build {action} {file}; IFS=' '; echo \"${{PM_ARGV[*]}}\""
            )),
            expected,
            "{family} {action}"
        );
    }
}

#[test]
fn installed_versions_are_read_from_the_answer_of_the_package_manager() {
    let cases = [
        ("deb", "install ok installed 0.9.0-1", Some("0.9.0-1")),
        ("deb", "deinstall ok config-files 0.9.0-1", None),
        ("rpm", "0.9.0-1", Some("0.9.0-1")),
        ("rpm", "package celata is not installed", None),
        ("arch", "celata 0.9.0-1", Some("0.9.0-1")),
        ("arch", "error: package 'celata' was not found", None),
    ];
    for (family, answer, expected) in cases {
        let run = bash(&format!("parse_package_version {family} '{answer}'"));
        match expected {
            Some(version) => assert_eq!((run.code, run.out.as_str()), (0, version), "{family}: {answer}"),
            None => assert_eq!(run.code, 1, "{family}: {answer}"),
        }
    }
}

#[test]
fn probes_of_the_package_managers_are_judged_by_their_known_answers() {
    let cases = [
        ("deb", 0, "Inst a", true),
        ("deb", 100, "E: Unable to correct problems", false),
        ("rpm", 1, "Transaction Summary:\nOperation aborted by the user.", true),
        (
            "rpm",
            1,
            "Failed to resolve the transaction\nProblem: conflicting requests",
            false,
        ),
        ("rpm", 0, "Nothing to do.", true),
        ("rpm", 2, "Operation aborted by the user.", false),
        ("arch", 0, "./file", true),
        (
            "arch",
            1,
            "error: failed to prepare transaction (could not satisfy dependencies)",
            false,
        ),
    ];
    for (family, code, output, expected) in cases {
        let run = bash(&format!(
            "CL_FAMILY={family}; probe_succeeded {code} \"$(printf '%s' '{output}')\""
        ));
        assert_eq!(run.code == 0, expected, "{family} {code} {output}");
    }
}

#[test]
fn an_unsupported_system_ends_every_mode_with_3_and_names_the_way_out() {
    for (mode, language, phrase) in [
        ("--dry-run", "C", "manual-setup.md"),
        ("", "C", "Supported: Fedora 44 and 45"),
        ("--uninstall", "C", "manual-setup.md"),
        ("--dry-run", "de_DE.UTF-8", "Unterstützt: Fedora 44 und 45"),
    ] {
        for file in ["debian-12", "fedora-silverblue-44", "opensuse-tumbleweed", "kali"] {
            let snippet = format!(
                "resolve_user() {{ USER_NAME=tester; }}; gather_system_facts() {{ HAS_SYSTEMD=1; MACHINE=x86_64; IS_OSTREE=0; }}; \
                 os_release_file() {{ echo \"$FIXTURES/os-release/{file}\"; }}; as_root() {{ echo AS_ROOT-WAS-CALLED; }}; \
                 fetch() {{ echo FETCH-WAS-CALLED; }}; main {mode}"
            );
            let run = run_bash(&snippet, &[("LC_ALL", language)], "");
            assert_eq!(run.code, 3, "{file} {mode}: stdout {} stderr {}", run.out, run.err);
            assert!(run.err.contains(phrase), "{file} {mode}: {}", run.err);
            assert!(
                !run.out.contains("WAS-CALLED") && !run.err.contains("WAS-CALLED"),
                "{file}: nothing may run"
            );
        }
    }
}

// --- Client and service file ---------------------------------------------------------------------------------

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn clients_are_found_by_their_executable_files_without_doubles() {
    let scratch = Scratch::new("clients");
    let base = fs::canonicalize(scratch.path()).unwrap();
    write_file(&base.join("a/hide.me"), "client");
    make_executable(&base.join("a/hide.me"));
    write_file(&base.join("b/hide.me"), "not executable");
    fs::create_dir_all(base.join("c/hide.me")).unwrap();
    fs::create_dir_all(base.join("d")).unwrap();
    std::os::unix::fs::symlink(base.join("a/hide.me"), base.join("d/hide.me")).unwrap();
    write_file(&base.join("e/hide.me"), "second client");
    make_executable(&base.join("e/hide.me"));
    let b = base.to_string_lossy();
    let found = out(&format!(
        "find_clients {b}/a/hide.me {b}/b/hide.me {b}/c/hide.me {b}/d/hide.me {b}/missing/hide.me {b}/e/hide.me"
    ));
    assert_eq!(found, format!("{b}/a/hide.me\n{b}/e/hide.me"));
}

#[test]
fn the_working_folder_is_the_first_folder_with_ca_pem() {
    let scratch = Scratch::new("clientdir");
    let base = fs::canonicalize(scratch.path()).unwrap();
    let b = base.to_string_lossy();
    write_file(&base.join("own/hide.me"), "");
    write_file(&base.join("etc/CA.pem"), "ca");
    write_file(&base.join("opt/CA.pem"), "ca");
    assert_eq!(
        out(&format!("client_dir {b}/own/hide.me {b}/etc {b}/opt")),
        format!("{b}/etc")
    );
    write_file(&base.join("own/CA.pem"), "ca");
    assert_eq!(
        out(&format!("client_dir {b}/own/hide.me {b}/etc {b}/opt")),
        format!("{b}/own")
    );
    assert_eq!(bash(&format!("client_dir {b}/own/hide.me {b}/nothing")).code, 0);
    fs::remove_file(base.join("own/CA.pem")).unwrap();
    assert_eq!(
        bash(&format!("client_dir {b}/own/hide.me {b}/nothing")).code,
        1,
        "no CA.pem means an unfinished client"
    );
}

#[test]
fn the_package_expects_the_client_where_the_hide_me_guide_puts_it() {
    assert_eq!(out("expected_client deb"), "/opt/hide.me/hide.me /opt/hide.me");
    assert_eq!(out("expected_client rpm"), "/opt/hide.me/hide.me /opt/hide.me");
    assert_eq!(out("expected_client arch"), "/usr/bin/hide.me /etc/hide.me");
}

#[test]
fn a_drop_in_is_needed_only_when_the_client_lies_elsewhere_and_the_unit_is_the_package_s() {
    let cases = [
        ("rpm", "/opt/hide.me/hide.me", "/opt/hide.me", 0, 1),
        ("deb", "/opt/hide.me/hide.me", "/opt/hide.me", 0, 1),
        ("rpm", "/usr/local/bin/hide.me", "/etc/hide.me", 0, 0),
        ("deb", "/usr/bin/hide.me", "/etc/hide.me", 0, 0),
        ("arch", "/usr/bin/hide.me", "/etc/hide.me", 0, 1),
        ("arch", "/opt/hide.me/hide.me", "/opt/hide.me", 0, 0),
        ("rpm", "/usr/local/bin/hide.me", "/etc/hide.me", 1, 1),
        ("arch", "/opt/hide.me/hide.me", "/opt/hide.me", 1, 1),
        ("rpm", "/opt/hide.me/hide.me", "/etc/hide.me", 0, 0),
    ];
    for (family, client, folder, manual, no_dropin) in cases {
        let run = bash(&format!("CL_FAMILY={family}; dropin_needed {client} {folder} {manual}"));
        assert_eq!(
            run.code == 0,
            no_dropin == 0,
            "{family} {client} {folder} manual={manual}"
        );
    }
}

#[test]
fn the_drop_in_has_exactly_the_agreed_content() {
    let expected = "\
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 derHouwer
# X-Celata-Setup=1
#
# Written by celata-setup.sh: points the Celata service at the hide.me client of this computer.
# \"bash celata-setup.sh --uninstall\" removes it.
[Service]
WorkingDirectory=/etc/hide.me
ExecStart=
ExecStart=/usr/local/bin/hide.me --caddr /run/hide.me/control.sock service
ReadWritePaths=
ReadWritePaths=/etc/hide.me /etc -/run/systemd/resolve";
    assert_eq!(out("render_dropin /usr/local/bin/hide.me /etc/hide.me"), expected);
}

#[test]
fn the_effective_unit_is_judged_from_the_output_of_systemctl_show() {
    let cases = [
        ("package-deb.txt", "/opt/hide.me/hide.me", "/opt/hide.me", true),
        ("package-deb.txt", "/usr/local/bin/hide.me", "/etc/hide.me", false),
        ("package-arch.txt", "/usr/bin/hide.me", "/etc/hide.me", true),
        ("package-arch.txt", "/opt/hide.me/hide.me", "/opt/hide.me", false),
        ("dropin-usr-local.txt", "/usr/local/bin/hide.me", "/etc/hide.me", true),
        ("dropin-usr-local.txt", "/opt/hide.me/hide.me", "/opt/hide.me", false),
        (
            "dropin-and-override.txt",
            "/usr/local/bin/hide.me",
            "/etc/hide.me",
            false,
        ),
        ("manual-unit.txt", "/opt/hide.me/hide.me", "/opt/hide.me", true),
        ("active.txt", "/opt/hide.me/hide.me", "/opt/hide.me", true),
        ("not-found.txt", "/opt/hide.me/hide.me", "/opt/hide.me", false),
        ("rwpaths-resolv-only.txt", "/opt/hide.me/hide.me", "/opt/hide.me", false),
    ];
    for (file, client, folder, expected) in cases {
        let run = bash(&format!(
            "parse_unit_show \"$(cat \"$FIXTURES/systemctl-show/{file}\")\"; unit_matches {client} {folder}"
        ));
        assert_eq!(run.code == 0, expected, "{file} with {client} in {folder}");
    }
    assert_eq!(
        out(
            "parse_unit_show \"$(cat \"$FIXTURES/systemctl-show/active.txt\")\"; echo \"${UNIT_INFO[ActiveState]}|${UNIT_INFO[LoadState]}\""
        ),
        "active|loaded"
    );
    assert_eq!(
        out(
            "parse_unit_show \"$(cat \"$FIXTURES/systemctl-show/dropin-and-override.txt\")\"; echo \"${UNIT_INFO[DropInPaths]}\""
        ),
        "/etc/systemd/system/celata-hide.me.service.d/50-celata-setup.conf /etc/systemd/system/celata-hide.me.service.d/override.conf"
    );
}

#[test]
fn only_the_size_of_the_token_file_is_looked_at() {
    let scratch = Scratch::new("token");
    let s = scratch.s();
    assert_eq!(bash(&format!("token_present {s}")).code, 1, "no file");
    write_file(&scratch.path().join("accessToken.txt"), "");
    assert_eq!(bash(&format!("token_present {s}")).code, 1, "an empty file is no token");
    write_file(&scratch.path().join("accessToken.txt"), "invented-example-value\n");
    assert_eq!(bash(&format!("token_present {s}")).code, 0);
}

#[test]
fn the_chain_of_a_client_must_belong_to_root_and_be_closed_to_others() {
    let trusted = "/opt/hide.me/hide.me 0 755\n/opt/hide.me 0 755\n/opt 0 755\n/ 0 555";
    let cases = [
        (trusted.to_owned(), true, ""),
        (
            "/opt/hide.me/hide.me 1000 755\n/opt/hide.me 0 755\n/ 0 755".to_owned(),
            false,
            "/opt/hide.me/hide.me",
        ),
        (
            "/opt/hide.me/hide.me 0 755\n/opt/hide.me 1000 755\n/ 0 755".to_owned(),
            false,
            "/opt/hide.me",
        ),
        (
            "/opt/hide.me/hide.me 0 755\n/tmp 0 1777\n/ 0 755".to_owned(),
            false,
            "/tmp",
        ),
        (
            "/opt/hide.me/hide.me 0 755\n/opt/hide.me/CA.pem 0 666\n/ 0 755".to_owned(),
            false,
            "/opt/hide.me/CA.pem",
        ),
        (
            "/opt/hide.me/hide.me 0 775\n/ 0 755".to_owned(),
            false,
            "/opt/hide.me/hide.me",
        ),
        (
            "/opt/hide.me/hide.me 0 757\n/ 0 755".to_owned(),
            false,
            "/opt/hide.me/hide.me",
        ),
        ("/opt/my folder/hide.me 0 755\n/ 0 755".to_owned(), true, ""),
        (
            "/opt/my folder/hide.me 5 755\n/ 0 755".to_owned(),
            false,
            "/opt/my folder/hide.me",
        ),
    ];
    for (lines, expected, bad) in cases {
        let run = run_bash("chain_trusted", &[], &format!("{lines}\n"));
        assert_eq!(run.code == 0, expected, "{lines}");
        assert_eq!(run.out, bad, "{lines}");
    }
}

#[test]
fn the_chain_is_read_from_the_file_up_to_the_root_folder() {
    let scratch = Scratch::new("chain");
    let base = fs::canonicalize(scratch.path()).unwrap();
    write_file(&base.join("x/y/hide.me"), "");
    let lines = out(&format!("stat_chain {}/x/y/hide.me", base.display()));
    let lines: Vec<&str> = lines.lines().collect();
    assert_eq!(
        lines[0],
        format!(
            "{}/x/y/hide.me {} {}",
            base.display(),
            uid_of(&base.join("x/y/hide.me")),
            "644"
        )
    );
    assert_eq!(
        lines.last().unwrap().split(' ').next(),
        Some("/"),
        "ends at the root folder"
    );
    assert!(lines.len() >= 5, "{lines:?}");
    assert_eq!(bash("stat_chain /does/not/exist").code, 1);
}

fn uid_of(path: &Path) -> u32 {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).unwrap().uid()
}

#[test]
fn a_client_in_a_folder_of_the_user_is_not_trusted() {
    let scratch = Scratch::new("untrusted");
    let base = fs::canonicalize(scratch.path()).unwrap();
    write_file(&base.join("hm/hide.me"), "");
    write_file(&base.join("hm/CA.pem"), "");
    let run = bash(&format!("client_trusted {0}/hm/hide.me {0}/hm", base.display()));
    if uid_of(&base) == 0 {
        return; // As root the folder belongs to root, which is the case under test.
    }
    assert_eq!(run.code, 1, "files of the current user are not at a safe place");
    assert_eq!(
        run.out,
        format!("{}/hm/hide.me", base.display()),
        "the first bad path is named"
    );
}

fn client_tree(scratch: &Scratch, names: &[&str]) -> String {
    let base = fs::canonicalize(scratch.path()).unwrap();
    for name in names {
        write_file(&base.join(name).join("hide.me"), "client");
        make_executable(&base.join(name).join("hide.me"));
    }
    base.to_string_lossy().into_owned()
}

#[test]
fn the_client_is_judged_to_be_ok_missing_ambiguous_or_incomplete() {
    let scratch = Scratch::new("evaluate");
    let b = client_tree(&scratch, &["one", "two"]);
    let prelude = format!("client_trusted() {{ return 0; }}; CLIENT_DIR_PLACES=({b}/etc {b}/opt); ");
    let state = |places: &str, extra: &str| {
        out(&format!(
            "{prelude}CLIENT_PLACES=({places}); {extra}; evaluate_client; echo \"$CLIENT_STATE|$CLIENT_PATH|$CLIENT_DIR\""
        ))
    };
    assert_eq!(state(&format!("{b}/nothing/hide.me"), "true"), "missing||");
    assert_eq!(
        state(&format!("{b}/one/hide.me"), "true"),
        format!("incomplete|{b}/one/hide.me|")
    );
    write_file(&scratch.path().join("etc/CA.pem"), "ca");
    assert_eq!(
        state(&format!("{b}/one/hide.me"), "true"),
        format!("ok|{b}/one/hide.me|{b}/etc")
    );
    assert_eq!(
        state(&format!("{b}/one/hide.me {b}/two/hide.me"), "true"),
        "ambiguous||",
        "two clients and no choice"
    );
    assert_eq!(
        state(
            &format!("{b}/one/hide.me {b}/two/hide.me"),
            &format!("OPT_CLIENT={b}/two/hide.me")
        ),
        format!("ok|{b}/two/hide.me|{b}/etc"),
        "--client chooses"
    );
    assert_eq!(
        state(&format!("{b}/one/hide.me"), &format!("OPT_CLIENT={b}/nothing/hide.me")),
        "missing||",
        "--client at a place without a client"
    );
    assert_eq!(
        state(&format!("{b}/one/hide.me"), &format!("OPT_CLIENT={b}/two/hide.me")),
        format!("ok|{b}/two/hide.me|{b}/etc"),
        "--client adds a place"
    );
}

#[test]
fn a_client_at_an_unsafe_place_stops_the_run_and_names_the_path() {
    let scratch = Scratch::new("gate");
    let b = client_tree(&scratch, &["one"]);
    write_file(&scratch.path().join("one/CA.pem"), "ca");
    let snippet = format!("CLIENT_PLACES=({b}/one/hide.me); CLIENT_DIR_PLACES=(); client_gate 1");
    let run = bash(&snippet);
    if uid_of(&PathBuf::from(&b)) == 0 {
        return;
    }
    assert_eq!(run.code, 3, "{}", run.err);
    assert!(run.err.contains("unsafe place"), "{}", run.err);
    assert!(
        run.err.contains(&format!("{b}/one/hide.me")),
        "the path is named: {}",
        run.err
    );
    assert!(run.err.contains("chown -R root:root"), "{}", run.err);
}

#[test]
fn several_clients_without_a_choice_stop_a_run_with_yes() {
    let scratch = Scratch::new("ambiguous");
    let b = client_tree(&scratch, &["one", "two"]);
    let snippet = format!(
        "client_trusted() {{ return 0; }}; CLIENT_PLACES=({b}/one/hide.me {b}/two/hide.me); CLIENT_DIR_PLACES=({b}/one); \
         OPT_YES=1; client_gate 1"
    );
    write_file(&scratch.path().join("one/CA.pem"), "ca");
    let run = bash(&snippet);
    assert_eq!(run.code, 3);
    assert!(run.err.contains("--client PATH"), "{}", run.err);
    // Without --yes the user chooses from a list.
    let snippet = snippet.replace("OPT_YES=1;", "OPT_YES=0;");
    let run = run_bash(&format!("{snippet}; echo \"chosen=$CLIENT_PATH\""), &[], "2\n");
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.out.contains(&format!("chosen={b}/two/hide.me")), "{}", run.out);
    let run = run_bash(&snippet, &[], "9\n");
    assert_eq!(run.code, 3, "an invalid choice stops the run");
    let run = run_bash(&snippet, &[], "");
    assert_eq!(run.code, 3, "no terminal and no answer stops the run");
    // The dry run never asks.
    let run = bash(&snippet.replace("client_gate 1", "client_gate 0"));
    assert_eq!(run.code, 3);
}

#[test]
fn the_state_of_a_drop_in_is_absent_current_stale_or_foreign() {
    let scratch = Scratch::new("dropin");
    let s = scratch.s();
    let file = format!("{s}/50-celata-setup.conf");
    let prelude = format!("DROPIN_FILE={file}; CLIENT_PATH=/usr/local/bin/hide.me; CLIENT_DIR=/etc/hide.me; ");
    assert_eq!(out(&format!("{prelude}dropin_state")), "absent");
    assert_eq!(
        out(&format!(
            "{prelude}render_dropin \"$CLIENT_PATH\" \"$CLIENT_DIR\" > {file}; dropin_state"
        )),
        "current"
    );
    assert_eq!(
        out(&format!(
            "{prelude}render_dropin /opt/other/hide.me /opt/other > {file}; dropin_state"
        )),
        "stale"
    );
    write_file(Path::new(&file), "[Service]\nExecStart=\n");
    assert_eq!(
        out(&format!("{prelude}dropin_state")),
        "foreign",
        "a file without the mark of the script is never ours"
    );
}

// --- Blocked internet, sums, download ---------------------------------------------------------------------------

#[test]
fn the_block_is_recognised_like_celata_does() {
    let cases = [
        ("free.json", "free.json", false),
        ("blocked-v4.json", "free.json", true),
        ("free.json", "blocked-v6.json", true),
        ("blocked-v4.json", "blocked-v6.json", true),
        ("tunnel-only.json", "throw-only.json", false),
        ("tunnel-only.json", "free.json", false),
        ("throw-only.json", "free.json", false),
    ];
    for (v4, v6, blocked) in cases {
        let snippet = format!(
            "ip() {{ if [[ \"$*\" == *-6* ]]; then cat \"$FIXTURES/routes/{v6}\"; else cat \"$FIXTURES/routes/{v4}\"; fi; }}; is_blocked"
        );
        assert_eq!(bash(&snippet).code == 0, blocked, "{v4} / {v6}");
    }
    assert_eq!(
        bash("ip() { return 1; }; is_blocked").code,
        1,
        "a table that cannot be read counts as free"
    );
    assert_eq!(
        bash("ip() { echo; }; is_blocked").code,
        1,
        "an empty table counts as free"
    );
    for file in ["blocked-v4.json", "blocked-v6.json"] {
        assert_eq!(
            bash(&format!("routes_block_internet \"$(cat \"$FIXTURES/routes/{file}\")\"")).code,
            0,
            "{file}"
        );
    }
}

#[test]
fn the_block_table_is_the_one_of_the_service() {
    let code = code_without_texts();
    assert!(code.contains("BLOCK_TABLE=55555"));
    let celata = fs::read_to_string(root().join("src/killswitch.rs")).unwrap();
    assert!(celata.contains("55555"), "Celata uses the same table");
}

const PACKAGE_BODY: &str = "celata package body\n";
const SCRIPT_BODY: &str = "celata setup script body\n";

#[test]
fn a_line_of_sha256sums_is_found_by_its_whole_name() {
    let sums = "$FIXTURES/sums/SHA256SUMS-ok";
    let found = out(&format!("sums_line_for celata_0.9.0-1_amd64.deb \"{sums}\""));
    assert_eq!(found.len(), 64);
    let other = out(&format!("sums_line_for celata_0.9.0-1_amd64.deb.old \"{sums}\""));
    assert_ne!(found, other, "a longer name has its own line");
    assert_eq!(
        bash(&format!("sums_line_for celata_0.9.0-1_amd64 \"{sums}\"")).code,
        1,
        "never a match on part of a name"
    );
    assert_eq!(bash(&format!("sums_line_for deb \"{sums}\"")).code, 1);
    assert_eq!(bash("sums_line_for x /does/not/exist").code, 2);
}

fn verify(scratch: &Scratch, sums: &str, content: &str) -> i32 {
    let s = scratch.s();
    write_file(&scratch.path().join("celata_0.9.0-1_amd64.deb"), content);
    bash(&format!(
        "verify_file \"{s}/celata_0.9.0-1_amd64.deb\" \"{sums}\"; echo \"rc=$? sum=$EXPECTED_SUM\" >&2; exit 0"
    ))
    .err
    .split("rc=")
    .nth(1)
    .and_then(|rest| rest.split_whitespace().next())
    .and_then(|code| code.parse().ok())
    .expect("the return value is printed")
}

#[test]
fn a_file_is_checked_against_sha256sums_with_four_different_failures() {
    let scratch = Scratch::new("verify");
    assert_eq!(
        verify(&scratch, "$FIXTURES/sums/SHA256SUMS-ok", PACKAGE_BODY),
        0,
        "right sum"
    );
    assert_eq!(
        verify(&scratch, "$FIXTURES/sums/SHA256SUMS-ok", "celata package body!\n"),
        3,
        "one byte changed"
    );
    assert_eq!(
        verify(&scratch, "$FIXTURES/sums/SHA256SUMS-tampered", PACKAGE_BODY),
        3,
        "wrong sum in the list"
    );
    assert_eq!(
        verify(&scratch, "$FIXTURES/sums/SHA256SUMS-missing-line", PACKAGE_BODY),
        2,
        "no line for the file"
    );
    assert_eq!(verify(&scratch, "/does/not/exist", PACKAGE_BODY), 1, "no SHA256SUMS");
    let s = scratch.s();
    let run = bash(&format!(
        "verify_file \"{s}/gone.deb\" \"$FIXTURES/sums/SHA256SUMS-ok\"; echo \"rc=$?\" >&2"
    ));
    assert!(
        run.err.contains("rc=2"),
        "a missing file without a line is a missing line first: {}",
        run.err
    );
    let kept = out(&format!(
        "verify_file \"{s}/celata_0.9.0-1_amd64.deb\" \"$FIXTURES/sums/SHA256SUMS-ok\"; printf %s \"$EXPECTED_SUM\""
    ));
    assert_eq!(
        kept.len(),
        64,
        "the expected sum is kept for the check before the installation"
    );
}

#[test]
fn the_script_checks_itself_against_its_own_line() {
    let scratch = Scratch::new("self");
    let s = scratch.s();
    write_file(&scratch.path().join("celata-setup.sh"), SCRIPT_BODY);
    let check = |sums: &str| {
        let run = bash(&format!(
            "SCRIPT_PATH=\"{s}/celata-setup.sh\"; verify_self \"{sums}\"; echo \"rc=$?\" >&2"
        ));
        run.err
            .contains("rc=0")
            .then_some(0)
            .or_else(|| run.err.contains("rc=1").then_some(1))
            .or_else(|| run.err.contains("rc=2").then_some(2))
            .or_else(|| run.err.contains("rc=3").then_some(3))
    };
    assert_eq!(check("$FIXTURES/sums/SHA256SUMS-ok"), Some(0));
    assert_eq!(
        check("$FIXTURES/sums/SHA256SUMS-tampered"),
        Some(0),
        "the script line of the second file is right too"
    );
    assert_eq!(check("$FIXTURES/sums/SHA256SUMS-missing-line"), Some(2));
    assert_eq!(check("/does/not/exist"), Some(1));
    write_file(&scratch.path().join("celata-setup.sh"), "changed\n");
    assert_eq!(check("$FIXTURES/sums/SHA256SUMS-ok"), Some(3));
}

#[test]
fn the_download_address_is_the_release_page_of_this_version() {
    assert_eq!(
        out("download_url celata_0.9.0-1_amd64.deb"),
        "https://github.com/derHouwer/celata/releases/download/v0.9.0/celata_0.9.0-1_amd64.deb"
    );
}

#[test]
fn curl_is_preferred_to_wget_and_without_both_there_is_no_download() {
    let scratch = Scratch::new("tools");
    let bin = scratch.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let b = fs::canonicalize(&bin).unwrap();
    let with = |path: &str| bash(&format!("PATH={path} fetch_tool"));
    assert_eq!(with(&b.to_string_lossy()).code, 1, "neither tool: no download");
    write_file(&b.join("wget"), "#!/bin/sh\n");
    make_executable(&b.join("wget"));
    assert_eq!(with(&b.to_string_lossy()).out, "wget");
    write_file(&b.join("curl"), "#!/bin/sh\n");
    make_executable(&b.join("curl"));
    assert_eq!(with(&b.to_string_lossy()).out, "curl");
}

/// Prelude for tests of `locate_package`: a work folder, a fake `fetch` and the family deb.
fn locate_prelude(scratch: &Scratch) -> String {
    let s = scratch.s();
    format!(
        "CL_FAMILY=deb; SCRIPT_DIR=\"{s}/dl\"; WORKDIR=\"{s}/work\"; mkdir -p \"$SCRIPT_DIR\" \"$WORKDIR\"; \
         fetch_tool() {{ printf curl; }}; fetch() {{ echo \"FETCH $1\" >> \"{s}/log\"; printf '%s' \"$FAKE_BODY\" > \"$2\"; }}; \
         cp \"$FIXTURES/sums/SHA256SUMS-ok\" \"$SCRIPT_DIR/SHA256SUMS\"; \
         printf 'celata setup script body\\n' > \"{s}/celata-setup.sh\"; SCRIPT_PATH=\"{s}/celata-setup.sh\"; "
    )
}

#[test]
fn a_file_next_to_the_script_wins_over_a_download() {
    let scratch = Scratch::new("locate-local");
    let s = scratch.s();
    let snippet = format!(
        "{}printf '%s' \"$FAKE_BODY\" > \"$SCRIPT_DIR/celata_0.9.0-1_amd64.deb\"; locate_package 1; echo \"$PKG_FROM|$PKG_PATH|${{#EXPECTED_SUM}}\"",
        locate_prelude(&scratch)
    );
    let run = run_bash(&snippet, &[("FAKE_BODY", PACKAGE_BODY)], "");
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(run.out.trim(), format!("local|{s}/dl/celata_0.9.0-1_amd64.deb|64"));
    assert!(!scratch.path().join("log").exists(), "no download");
}

#[test]
fn a_missing_file_is_downloaded_and_checked() {
    let scratch = Scratch::new("locate-download");
    let s = scratch.s();
    let snippet = format!(
        "{}locate_package 1; echo \"$PKG_FROM|$PKG_PATH\"",
        locate_prelude(&scratch)
    );
    let run = run_bash(&snippet, &[("FAKE_BODY", PACKAGE_BODY)], "");
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(
        run.out.lines().last().unwrap(),
        format!("download|{s}/work/celata_0.9.0-1_amd64.deb")
    );
    let log = fs::read_to_string(scratch.path().join("log")).unwrap();
    assert_eq!(
        log.trim(),
        "FETCH https://github.com/derHouwer/celata/releases/download/v0.9.0/celata_0.9.0-1_amd64.deb"
    );
    let dry = run_bash(
        &format!(
            "{}locate_package 0; echo \"$PKG_FROM|$PKG_PATH\"",
            locate_prelude(&scratch)
        ),
        &[("FAKE_BODY", PACKAGE_BODY)],
        "",
    );
    assert_eq!(dry.out.trim(), "download|", "the dry run does not download");
    assert_eq!(
        fs::read_to_string(scratch.path().join("log")).unwrap().lines().count(),
        1,
        "still one download"
    );
}

#[test]
fn a_wrong_sum_stops_the_run_and_deletes_the_download() {
    let scratch = Scratch::new("locate-wrong");
    let snippet = format!("{}locate_package 1", locate_prelude(&scratch));
    let run = run_bash(&snippet, &[("FAKE_BODY", "not the package\n")], "");
    assert_eq!(run.code, 3);
    assert!(
        run.err.contains("does not match SHA256SUMS") && run.err.contains("Nothing was changed"),
        "{}",
        run.err
    );
    assert!(
        !scratch.path().join("work/celata_0.9.0-1_amd64.deb").exists(),
        "the file is deleted"
    );
    let german = run_bash(
        &format!("detect_language; {snippet}"),
        &[("FAKE_BODY", "not the package\n"), ("LC_ALL", "de_DE.UTF-8")],
        "",
    );
    assert_eq!(german.code, 3);
    assert!(german.err.contains("passt nicht zu SHA256SUMS"), "{}", german.err);
}

#[test]
fn without_sha256sums_nothing_is_downloaded_or_used() {
    let scratch = Scratch::new("locate-nosums");
    let prelude =
        locate_prelude(&scratch).replace("cp \"$FIXTURES/sums/SHA256SUMS-ok\" \"$SCRIPT_DIR/SHA256SUMS\"; ", "");
    let run = run_bash(
        &format!("{prelude}locate_package 1"),
        &[("FAKE_BODY", PACKAGE_BODY)],
        "",
    );
    assert_eq!(run.code, 3);
    assert!(run.err.contains("SHA256SUMS is not next to this script"), "{}", run.err);
    assert!(
        !scratch.path().join("log").exists(),
        "no download without a way to check it"
    );
    let local = format!("{prelude}printf x > \"$SCRIPT_DIR/celata_0.9.0-1_amd64.deb\"; locate_package 1");
    assert_eq!(
        run_bash(&local, &[], "").code,
        3,
        "a file next to the script needs SHA256SUMS as well"
    );
}

#[test]
fn a_script_that_does_not_belong_to_sha256sums_stops_the_run() {
    let scratch = Scratch::new("locate-self");
    let s = scratch.s();
    let run = run_bash(
        &format!(
            "{}printf 'changed\\n' > \"{s}/celata-setup.sh\"; locate_package 1",
            locate_prelude(&scratch)
        ),
        &[("FAKE_BODY", PACKAGE_BODY)],
        "",
    );
    assert_eq!(run.code, 3);
    assert!(run.err.contains("does not match its line"), "{}", run.err);
}

#[test]
fn no_download_tool_and_a_failing_download_end_the_run_with_a_way_out() {
    let scratch = Scratch::new("locate-fail");
    let snippet = format!(
        "{}fetch_tool() {{ return 1; }}; locate_package 1",
        locate_prelude(&scratch)
    );
    let run = run_bash(&snippet, &[], "");
    assert_eq!(run.code, 3);
    assert!(
        run.err.contains("Neither curl nor wget") && run.err.contains("next to this script"),
        "{}",
        run.err
    );
    let snippet = format!("{}fetch() {{ return 22; }}; locate_package 1", locate_prelude(&scratch));
    let run = run_bash(&snippet, &[], "");
    assert_eq!(run.code, 3);
    assert!(
        run.err.contains("blocked"),
        "the message names the block as a possible reason: {}",
        run.err
    );
}

// --- Record of changes ---------------------------------------------------------------------------------------------

#[test]
fn the_record_is_read_and_written_without_losing_unknown_keys() {
    let scratch = Scratch::new("state");
    let s = scratch.s();
    write_file(
        &scratch.path().join("state"),
        "# comment\nformat=1\npackage=setup\npackage_version=0.9.0-1\ndropin=/etc/x.conf\nuser=anna\nfuture_key=keep me\n",
    );
    let again = out(&format!(
        "load_state \"{s}/state\"; state_add_user bob; state_add_user anna; render_state"
    ));
    assert_eq!(
        again,
        "# Written by celata-setup.sh. Lists what the script set up itself.\nformat=1\npackage=setup\npackage_version=0.9.0-1\ndropin=/etc/x.conf\nuser=anna bob\nfuture_key=keep me"
    );
    let round = out(&format!(
        "load_state \"{s}/state\"; render_state > \"{s}/out\"; load_state \"{s}/out\"; render_state"
    ));
    assert_eq!(round, out(&format!("load_state \"{s}/state\"; render_state")));
    assert_eq!(
        out(&format!("load_state \"{s}/nothing\"; render_state")),
        "# Written by celata-setup.sh. Lists what the script set up itself.\nformat=1"
    );
}

// --- The plan --------------------------------------------------------------------------------------------------

/// Facts of a fresh Fedora system with the client where the package expects it.
const FRESH: &str = "CL_FAMILY=rpm; USER_NAME=tester; CLIENT_STATE=ok; CLIENT_PATH=/opt/hide.me/hide.me; CLIENT_DIR=/opt/hide.me; \
    PKG_STATE=none; PKG_FILES_MISSING=0; MANUAL_UNIT_PRESENT=0; MANUAL_RULE_PRESENT=0; MANUAL_PROGRAM_PRESENT=0; \
    DROPIN_STATE=absent; GROUP_PRESENT=0; MEMBER_PRESENT=0; UNIT_ACTIVE=0; STATE=()";

/// "id:consent" of every step of the plan for the facts of FRESH changed by `changes`.
fn plan(changes: &str) -> String {
    out(&format!(
        "{FRESH}; {changes}; build_plan; for i in \"${{!PLAN_ID[@]}}\"; do printf '%s:%s ' \"${{PLAN_ID[i]}}\" \"${{PLAN_CONSENT[i]}}\"; done"
    ))
    .trim_end()
    .to_owned()
}

#[test]
fn a_fresh_system_gets_the_package_the_group_and_the_record() {
    assert_eq!(plan("true"), "package-install:plan group-add:account state-write:plan");
}

#[test]
fn a_client_at_another_place_gets_a_drop_in_and_a_reload() {
    assert_eq!(
        plan("CLIENT_PATH=/usr/local/bin/hide.me; CLIENT_DIR=/etc/hide.me"),
        "package-install:plan dropin-write:plan reload:plan group-add:account state-write:plan"
    );
}

#[test]
fn a_complete_system_has_an_empty_plan() {
    let complete = "PKG_STATE=current; GROUP_PRESENT=1; MEMBER_PRESENT=1";
    assert_eq!(plan(complete), "");
    assert_eq!(
        plan(&format!("{complete}; PKG_STATE=newer")),
        "",
        "a newer package is left alone"
    );
    assert_eq!(
        plan(&format!(
            "{complete}; CLIENT_PATH=/usr/local/bin/hide.me; CLIENT_DIR=/etc/hide.me; DROPIN_STATE=current"
        )),
        "",
        "a drop-in that is in place and right needs nothing"
    );
}

#[test]
fn a_lost_file_of_the_package_is_repaired_by_installing_it_again() {
    let installed = "PKG_STATE=current; GROUP_PRESENT=1; MEMBER_PRESENT=1; PKG_FILES_MISSING=1";
    assert_eq!(
        plan(&format!("{installed}; STATE[package]=setup")),
        "package-reinstall:plan state-write:plan"
    );
    assert_eq!(
        plan(installed),
        "package-reinstall:overwrite",
        "a package that was there before the script is touched only after its own question"
    );
}

#[test]
fn an_older_package_is_updated_and_the_origin_decides_who_is_asked() {
    let old = "PKG_STATE=old; GROUP_PRESENT=1; MEMBER_PRESENT=1";
    assert_eq!(
        plan(&format!("{old}; STATE[package]=setup")),
        "package-update:plan state-write:plan"
    );
    assert_eq!(plan(old), "package-update:overwrite");
}

#[test]
fn a_setup_by_hand_is_never_overwritten_and_the_package_asks_on_its_own() {
    let by_hand =
        "MANUAL_UNIT_PRESENT=1; MANUAL_RULE_PRESENT=1; MANUAL_PROGRAM_PRESENT=1; GROUP_PRESENT=1; MEMBER_PRESENT=1";
    assert_eq!(plan(by_hand), "package-install:separate");
    let group_missing = "MANUAL_UNIT_PRESENT=1; MEMBER_PRESENT=0; GROUP_PRESENT=0";
    assert_eq!(
        plan(group_missing),
        "package-install:separate group-create:plan group-add:account state-write:plan",
        "the group is made by the script in case the package is declined"
    );
    assert_eq!(
        plan(&format!(
            "{by_hand}; CLIENT_PATH=/usr/local/bin/hide.me; CLIENT_DIR=/etc/hide.me"
        )),
        "package-install:separate",
        "no drop-in next to a service file made by hand"
    );
}

#[test]
fn a_running_service_is_never_stopped_or_restarted_by_a_setup() {
    for facts in [
        "UNIT_ACTIVE=1",
        "UNIT_ACTIVE=1; PKG_STATE=old; STATE[package]=setup",
        "UNIT_ACTIVE=1; CLIENT_DIR=/etc/hide.me",
    ] {
        let steps = plan(facts);
        assert!(!steps.contains("service-stop"), "{steps}");
    }
}

#[test]
fn a_drop_in_is_replaced_removed_or_left_depending_on_who_wrote_it() {
    let elsewhere = "PKG_STATE=current; GROUP_PRESENT=1; MEMBER_PRESENT=1; CLIENT_PATH=/usr/local/bin/hide.me; CLIENT_DIR=/etc/hide.me";
    assert_eq!(
        plan(&format!("{elsewhere}; DROPIN_STATE=stale")),
        "dropin-write:plan reload:plan state-write:plan"
    );
    assert_eq!(
        plan(&format!("{elsewhere}; DROPIN_STATE=foreign")),
        "dropin-write:overwrite reload:plan state-write:plan"
    );
    let expected = "PKG_STATE=current; GROUP_PRESENT=1; MEMBER_PRESENT=1";
    assert_eq!(
        plan(&format!("{expected}; DROPIN_STATE=current")),
        "dropin-remove:overwrite reload:plan state-write:plan"
    );
    assert_eq!(
        plan(&format!("{expected}; DROPIN_STATE=foreign")),
        "",
        "a foreign file is never removed"
    );
}

#[test]
fn consent_classes_decide_what_yes_covers() {
    let all = plan("true");
    assert!(all.contains("group-add:account"), "{all}");
    let asked = |opt: &str, consent: &str| {
        let run = run_bash(&format!("{opt}; ask ask_execute no {consent}; echo \"rc=$?\""), &[], "");
        run.out.contains("rc=0")
    };
    assert!(asked("OPT_YES=1", "plan"));
    assert!(asked("OPT_YES=1", "account"));
    assert!(
        !asked("OPT_YES=1", "overwrite"),
        "--yes never agrees to replacing something"
    );
    assert!(
        !asked("OPT_YES=1", "separate"),
        "--yes never agrees to a separate package"
    );
    assert!(
        !asked("OPT_YES=0", "plan"),
        "without --yes and without an answer the default no stands"
    );
}

#[test]
fn answers_to_a_question_follow_the_rules_of_the_script() {
    let answer = |stdin: &str, default: &str| {
        let run = run_bash(
            &format!("OPT_YES=0; ask ask_execute {default} plan; echo \"rc=$?\""),
            &[],
            stdin,
        );
        run.out.contains("rc=0")
    };
    for yes in ["y\n", "Y\n", "yes\n", "YES\n", "j\n", "J\n", "ja\n", "Ja\n", " y \n"] {
        assert!(answer(yes, "no"), "{yes:?} is a yes");
    }
    for no in ["n\n", "no\n", "nein\n", "x\n", "maybe\n"] {
        assert!(!answer(no, "yes"), "{no:?} is a no");
    }
    assert!(answer("\n", "yes"), "an empty answer takes the default yes");
    assert!(!answer("\n", "no"), "an empty answer takes the default no");
    let prompt = run_bash("ask ask_execute yes plan", &[], "\n");
    assert!(prompt.out.contains("Run this now? [Y/n]"), "{}", prompt.out);
    let german = run_bash(
        "detect_language; ask ask_execute yes plan",
        &[("LC_ALL", "de_DE.UTF-8")],
        "\n",
    );
    assert!(german.out.contains("Jetzt ausführen? [J/n]"), "{}", german.out);
}

#[test]
fn without_a_terminal_a_question_ends_the_run_and_points_to_yes() {
    let run = run_bash("OPT_YES=0; ask ask_execute yes plan", &[], "");
    assert_eq!(run.code, 3, "{}", run.out);
    assert!(run.err.contains("--yes"), "{}", run.err);
}

// --- The removal plan ----------------------------------------------------------------------------------------------

fn uninstall_plan(changes: &str) -> String {
    out(&format!(
        "CL_FAMILY=rpm; STATE=(); PKG_STATE=none; UNIT_ACTIVE=0; DROPIN_FILE=/nonexistent/x; STATE_FILE=/nonexistent/y; {changes}; \
         build_uninstall_plan; for i in \"${{!PLAN_ID[@]}}\"; do printf '%s:%s ' \"${{PLAN_ID[i]}}\" \"${{PLAN_CONSENT[i]}}\"; done"
    ))
    .trim_end()
    .to_owned()
}

#[test]
fn a_removal_follows_the_origin_of_the_package() {
    assert_eq!(
        uninstall_plan("PKG_STATE=current; STATE[package]=setup"),
        "package-remove:plan"
    );
    assert_eq!(
        uninstall_plan("PKG_STATE=current"),
        "package-remove:separate",
        "a package from before the script asks on its own"
    );
    assert_eq!(uninstall_plan("true"), "", "nothing to remove");
}

#[test]
fn a_removal_has_a_fixed_order_and_stops_the_service_only_before_removing_the_package() {
    let scratch = Scratch::new("uninstall");
    let s = scratch.s();
    write_file(&scratch.path().join("dropin.conf"), "# X-Celata-Setup=1\n[Service]\n");
    write_file(&scratch.path().join("state"), "format=1\n");
    let paths = format!("DROPIN_FILE={s}/dropin.conf; STATE_FILE={s}/state");
    assert_eq!(
        uninstall_plan(&format!(
            "{paths}; PKG_STATE=current; STATE[package]=setup; UNIT_ACTIVE=1"
        )),
        "service-stop:plan dropin-delete:plan reload:plan package-remove:plan state-delete:plan"
    );
    assert_eq!(
        uninstall_plan(&format!("{paths}; PKG_STATE=current; STATE[package]=setup")),
        "dropin-delete:plan reload:plan package-remove:plan state-delete:plan"
    );
    assert_eq!(
        uninstall_plan(&format!("{paths}; UNIT_ACTIVE=1")),
        "dropin-delete:plan reload:plan state-delete:plan",
        "an active service is left running when no package is removed"
    );
    write_file(&scratch.path().join("dropin.conf"), "[Service]\nExecStart=\n");
    assert_eq!(
        uninstall_plan(&format!("{paths}; PKG_STATE=current; STATE[package]=setup")),
        "package-remove:plan state-delete:plan",
        "a file without the mark is not ours"
    );
}

#[test]
fn declined_questions_take_dependent_steps_out_of_the_plan() {
    let scratch = Scratch::new("prune");
    let s = scratch.s();
    write_file(&scratch.path().join("state"), "format=1\n");
    let snippet = format!(
        "OPT_UNINSTALL=1; CL_FAMILY=rpm; STATE=(); STATE_FILE={s}/state; DROPIN_FILE=/nonexistent; PKG_STATE=current; UNIT_ACTIVE=1; \
         build_uninstall_plan; ask_separate_steps; IFS=' '; echo \"${{PLAN_ID[*]}}|${{SKIPPED_STEPS[*]}}\""
    );
    let declined = run_bash(&snippet, &[], "n\n");
    assert_eq!(declined.code, 0, "{}", declined.err);
    let last = declined.out.lines().last().unwrap();
    assert!(
        last.contains("] state-delete|"),
        "the service is not stopped when the package stays: {last}"
    );
    assert!(last.contains("not done"), "the declined step is named: {last}");
    let agreed = run_bash(&snippet, &[], "y\n");
    assert!(
        agreed
            .out
            .lines()
            .last()
            .unwrap()
            .contains("] service-stop package-remove state-delete|"),
        "{}",
        agreed.out
    );
}

#[test]
fn a_question_of_its_own_is_asked_once_per_run() {
    // The plan is asked about early (before the preflight) and again after "Execute?", and it may be rebuilt in
    // between. Each step with its own question must still be asked only once, and a "no" must stay a "no".
    let counting = "ASKED=0; ask() { ASKED=$((ASKED+1)); return \"${ANSWER:-0}\"; }; ";
    let build = "PKG_STATE=none; CLIENT_STATE=ok; GROUP_PRESENT=1; MEMBER_PRESENT=1; DROPIN_STATE=absent; \
         MANUAL_UNIT_PRESENT=1; MANUAL_RULE_PRESENT=0; MANUAL_PROGRAM_PRESENT=0; CLIENT_PATH=/x; CLIENT_DIR=/x; \
         CL_FAMILY=deb; STATE=(); OPT_UNINSTALL=0; build_plan; ";
    let run = |answer: &str| {
        run_bash(
            &format!(
                "{counting}ANSWER={answer}; {build}ask_separate_steps; {build}ask_separate_steps; ask_separate_steps; \
                 IFS=' '; echo \"$ASKED|${{PLAN_ID[*]}}|${{#SKIPPED_STEPS[@]}}\""
            ),
            &[],
            "",
        )
    };
    let yes = run("0");
    assert_eq!(yes.code, 0, "{}", yes.err);
    assert_eq!(
        yes.out.lines().last().unwrap(),
        "1|package-install state-write|0",
        "agreed once, kept through a rebuilt plan"
    );
    let no = run("1");
    assert_eq!(no.code, 0, "{}", no.err);
    assert_eq!(
        no.out.lines().last().unwrap(),
        "1||1",
        "declined once, still declined after the plan is rebuilt, named once"
    );
}

// --- Steps with a stand-in for administrator rights -------------------------------------------------------------------

/// A stand-in for `as_root` that writes every call to a log and answers the few commands whose output is used.
fn recorder(scratch: &Scratch) -> String {
    let s = scratch.s();
    format!(
        "as_root() {{ local IFS=' '; printf 'AS_ROOT %s\\n' \"$*\" >> \"{s}/log\"; \
           case \"$1\" in \
             mktemp) echo /var/tmp/celata-setup.TESTDIR1 ;; \
             sha256sum) printf '%s  %s\\n' \"${{FAKE_SUM:-$EXPECTED_SUM}}\" \"$3\" ;; \
             usermod) return \"${{USERMOD_RC:-0}}\" ;; \
           esac; }}; WORKDIR=\"{s}\"; "
    )
}

fn log(scratch: &Scratch) -> Vec<String> {
    fs::read_to_string(scratch.path().join("log"))
        .map(|text| text.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Facts for a package step; the package file exists in the scratch folder.
fn package_step(scratch: &Scratch) -> String {
    write_file(&scratch.path().join("celata_0.9.0-1_amd64.deb"), "package");
    format!(
        "CL_FAMILY=deb; PKG_PATH=\"{}/celata_0.9.0-1_amd64.deb\"; EXPECTED_SUM=abc123; STATE=(); PKG_STATE=none; ",
        scratch.s()
    )
}

#[test]
fn the_package_is_copied_checked_again_and_only_the_copy_is_installed() {
    let scratch = Scratch::new("step-package");
    let snippet = format!(
        "{}{}run_step package-install; echo \"rc=$? ${{STATE[package]:-}} ${{STATE[package_version]:-}}\"",
        recorder(&scratch),
        package_step(&scratch)
    );
    let run = bash(&snippet);
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.out.contains("rc=0 setup 0.9.0-1"), "{}", run.out);
    assert_eq!(
        log(&scratch),
        [
            "AS_ROOT mktemp -d -p /var/tmp celata-setup.XXXXXXXX",
            "AS_ROOT chmod 0755 -- /var/tmp/celata-setup.TESTDIR1",
            &format!(
                "AS_ROOT install -m 0644 -- {}/celata_0.9.0-1_amd64.deb /var/tmp/celata-setup.TESTDIR1/celata_0.9.0-1_amd64.deb",
                scratch.s()
            ),
            "AS_ROOT sha256sum -- /var/tmp/celata-setup.TESTDIR1/celata_0.9.0-1_amd64.deb",
            "AS_ROOT env DEBIAN_FRONTEND=noninteractive apt-get install -y /var/tmp/celata-setup.TESTDIR1/celata_0.9.0-1_amd64.deb",
            "AS_ROOT rm -rf -- /var/tmp/celata-setup.TESTDIR1",
        ]
    );
    let manager: Vec<_> = log(&scratch)
        .into_iter()
        .filter(|line| line.contains("apt-get"))
        .collect();
    assert_eq!(manager.len(), 1);
    assert!(
        !manager[0].contains(&scratch.s()),
        "the package manager never gets the path next to the script"
    );
}

#[test]
fn a_copy_whose_sum_changed_is_never_installed() {
    let scratch = Scratch::new("step-changed");
    let snippet = format!(
        "{}{}FAKE_SUM=deadbeef; run_step package-install; echo \"rc=$?\"",
        recorder(&scratch),
        package_step(&scratch)
    );
    let run = bash(&snippet);
    assert!(run.out.contains("rc=1"), "{}", run.out);
    let lines = log(&scratch);
    assert!(
        lines.iter().all(|line| !line.contains("apt-get")),
        "no installation: {lines:?}"
    );
    assert!(
        lines
            .last()
            .unwrap()
            .starts_with("AS_ROOT rm -rf -- /var/tmp/celata-setup."),
        "the copy is deleted: {lines:?}"
    );
}

#[test]
fn every_family_hands_the_checked_copy_to_its_package_manager() {
    for (family, file, expected) in [
        (
            "rpm",
            "celata-0.9.0-1.x86_64.rpm",
            "dnf install -y /var/tmp/celata-setup.TESTDIR1/celata-0.9.0-1.x86_64.rpm",
        ),
        (
            "arch",
            "celata-0.9.0-1-x86_64.pkg.tar.zst",
            "pacman -U --noconfirm --needed /var/tmp/celata-setup.TESTDIR1/celata-0.9.0-1-x86_64.pkg.tar.zst",
        ),
    ] {
        let scratch = Scratch::new(&format!("step-{family}"));
        let snippet = format!(
            "{}{}CL_FAMILY={family}; run_step package-install",
            recorder(&scratch),
            package_step(&scratch)
        );
        assert_eq!(bash(&snippet).code, 0);
        assert!(
            log(&scratch).iter().any(|line| line == &format!("AS_ROOT {expected}")),
            "{family}: {:?} {file}",
            log(&scratch)
        );
    }
}

#[test]
fn an_update_keeps_the_origin_and_a_reinstall_restores_files() {
    let scratch = Scratch::new("step-update");
    let snippet = format!(
        "{}{}STATE[package]=preexisting; run_step package-update; echo \"${{STATE[package]}} ${{STATE[package_version]}}\"; \
         run_step package-reinstall",
        recorder(&scratch),
        package_step(&scratch)
    );
    let run = bash(&snippet);
    assert!(run.out.contains("preexisting 0.9.0-1"), "{}", run.out);
    assert!(
        log(&scratch)
            .iter()
            .any(|line| line.contains("apt-get install -y --reinstall")),
        "{:?}",
        log(&scratch)
    );
}

#[test]
fn the_group_is_created_once_and_the_account_is_added_to_it() {
    let scratch = Scratch::new("step-group");
    let snippet = format!(
        "{}getent() {{ return 2; }}; run_step group-create; getent() {{ return 0; }}; run_step group-create; \
         user_acceptable() {{ [[ $1 == anna ]]; }}; USER_NAME=anna; STATE=(); run_step group-add; echo \"relogin=$NEED_RELOGIN user=${{STATE[user]}}\"",
        recorder(&scratch)
    );
    let run = bash(&snippet);
    assert!(run.out.contains("relogin=1 user=anna"), "{}", run.out);
    assert_eq!(
        log(&scratch),
        ["AS_ROOT groupadd --system celata", "AS_ROOT usermod -aG celata -- anna"],
        "the second create finds the group and does nothing"
    );
}

#[test]
fn root_and_system_accounts_never_join_the_group() {
    let scratch = Scratch::new("step-root");
    for name in ["root", "daemon", "missing-account"] {
        let snippet = format!(
            "{}getent() {{ return 0; }}; USER_NAME={name}; run_step group-add; echo \"rc=$?\"",
            recorder(&scratch)
        );
        let run = bash(&snippet);
        assert!(run.out.contains("rc=1"), "{name}: {}", run.out);
    }
    assert!(
        log(&scratch).is_empty(),
        "no usermod for any of them: {:?}",
        log(&scratch)
    );
    assert_eq!(bash("uid_acceptable 0").code, 1);
    assert_eq!(bash("uid_acceptable 999").code, 1);
    assert_eq!(bash("uid_acceptable 1000").code, 0);
    assert_eq!(
        bash("uid_acceptable 65534").code,
        1,
        "nobody and the numbers above the normal range are no users"
    );
    assert_eq!(bash("uid_acceptable abc").code, 1);
}

#[test]
fn the_drop_in_and_the_record_are_written_next_to_their_place_and_renamed() {
    let scratch = Scratch::new("step-files");
    let snippet = format!(
        "{}CLIENT_PATH=/usr/local/bin/hide.me; CLIENT_DIR=/etc/hide.me; STATE=(); PKG_STATE=current; STATE_EXTRA=(); \
         run_step dropin-write; run_step state-write; echo \"${{STATE[dropin]}} ${{STATE[package]}}\"",
        recorder(&scratch)
    );
    let run = bash(&snippet);
    assert!(
        run.out
            .contains("/etc/systemd/system/celata-hide.me.service.d/50-celata-setup.conf preexisting"),
        "{}",
        run.out
    );
    let s = scratch.s();
    assert_eq!(
        log(&scratch),
        [
            "AS_ROOT install -d -m 0755 -- /etc/systemd/system/celata-hide.me.service.d",
            &format!(
                "AS_ROOT install -m 0644 -- {s}/dropin.conf /etc/systemd/system/celata-hide.me.service.d/50-celata-setup.conf.new"
            ),
            "AS_ROOT mv -f -- /etc/systemd/system/celata-hide.me.service.d/50-celata-setup.conf.new /etc/systemd/system/celata-hide.me.service.d/50-celata-setup.conf",
            "AS_ROOT install -d -m 0755 -- /var/lib/celata-setup",
            &format!("AS_ROOT install -m 0644 -- {s}/state /var/lib/celata-setup/state.new"),
            "AS_ROOT mv -f -- /var/lib/celata-setup/state.new /var/lib/celata-setup/state",
        ]
    );
    let dropin = fs::read_to_string(scratch.path().join("dropin.conf")).unwrap();
    assert!(dropin.contains("ExecStart=/usr/local/bin/hide.me --caddr /run/hide.me/control.sock service"));
    let record = fs::read_to_string(scratch.path().join("state")).unwrap();
    assert!(
        record.contains("package=preexisting") && record.contains("format=1"),
        "{record}"
    );
}

#[test]
fn a_service_that_runs_asks_for_a_restart_that_the_user_makes() {
    let scratch = Scratch::new("step-reload");
    let run = bash(&format!(
        "{}UNIT_ACTIVE=1; run_step reload; echo \"restart=$NEED_RESTART\"",
        recorder(&scratch)
    ));
    assert!(run.out.contains("restart=1"));
    assert_eq!(
        log(&scratch),
        ["AS_ROOT systemctl daemon-reload"],
        "reload of systemd only, no restart of the service"
    );
    let scratch = Scratch::new("step-reload2");
    let run = bash(&format!(
        "{}UNIT_ACTIVE=0; run_step reload; echo \"restart=$NEED_RESTART\"",
        recorder(&scratch)
    ));
    assert!(run.out.contains("restart=0"));
}

#[test]
fn a_foreign_file_is_never_deleted_as_our_drop_in() {
    let scratch = Scratch::new("step-foreign");
    let s = scratch.s();
    write_file(&scratch.path().join("x.conf"), "[Service]\n");
    let run = bash(&format!(
        "{}DROPIN_FILE={s}/x.conf; run_step dropin-delete; echo \"rc=$?\"",
        recorder(&scratch)
    ));
    assert!(run.out.contains("rc=1"), "{}", run.out);
    assert!(log(&scratch).is_empty(), "{:?}", log(&scratch));
}

#[test]
fn a_failed_step_ends_the_run_but_the_record_is_still_written() {
    let scratch = Scratch::new("run-failed");
    let snippet = format!(
        "{}{}CL_FAMILY=deb; USER_NAME=anna; user_acceptable() {{ return 0; }}; getent() {{ return 0; }}; CLIENT_STATE=ok; \
         CLIENT_PATH=/opt/hide.me/hide.me; CLIENT_DIR=/opt/hide.me; GROUP_PRESENT=0; MEMBER_PRESENT=0; DROPIN_STATE=absent; \
         MANUAL_UNIT_PRESENT=0; MANUAL_RULE_PRESENT=0; MANUAL_PROGRAM_PRESENT=0; PKG_FILES_MISSING=0; \
         build_plan; USERMOD_RC=1; run_plan; echo \"rc=$?\"; IFS=' '; echo \"skipped=${{SKIPPED_STEPS[*]}}\"",
        recorder(&scratch),
        package_step(&scratch)
    );
    let run = bash(&snippet);
    assert!(run.out.contains("rc=1"), "{}", run.out);
    let lines = log(&scratch);
    assert!(
        lines.iter().any(|line| line.contains("apt-get install")),
        "the first step was done: {lines:?}"
    );
    assert!(lines.iter().any(|line| line.contains("usermod")), "{lines:?}");
    assert!(
        lines
            .last()
            .unwrap()
            .contains("mv -f -- /var/lib/celata-setup/state.new"),
        "the record is written last: {lines:?}"
    );
    let record = fs::read_to_string(scratch.path().join("state")).unwrap();
    assert!(
        record.contains("package=setup"),
        "what was done stays on record: {record}"
    );
    assert!(
        run.out.contains("Add your account to the group celata: failed"),
        "{}",
        run.out
    );
    assert!(run.err.contains("failed"), "the failure is shown: {}", run.err);
}

#[test]
fn after_a_failed_run_the_next_plan_holds_only_the_open_steps() {
    let after = "PKG_STATE=current; GROUP_PRESENT=1; MEMBER_PRESENT=0; STATE[package]=setup";
    assert_eq!(plan(after), "group-add:account state-write:plan");
    let done = "PKG_STATE=current; GROUP_PRESENT=1; MEMBER_PRESENT=1; STATE[package]=setup";
    assert_eq!(plan(done), "", "a second run changes nothing");
}

#[test]
fn a_dry_run_command_is_shown_with_the_exact_command_line() {
    let run = bash(&format!(
        "{FRESH}; CLIENT_PATH=/usr/local/bin/hide.me; CLIENT_DIR=/etc/hide.me; build_plan; show_plan"
    ));
    assert!(
        run.out.contains(
            "1. Install the Celata package (dnf install -y /var/tmp/celata-setup.XXXXXXXX/celata-0.9.0-1.x86_64.rpm)"
        ),
        "{}",
        run.out
    );
    assert!(run.out.contains("Tell the service where your hide.me client is (install -m 0644 /etc/systemd/system/celata-hide.me.service.d/50-celata-setup.conf)"), "{}", run.out);
    assert!(
        run.out
            .contains("Add your account to the group celata (usermod -aG celata tester)"),
        "{}",
        run.out
    );
    assert!(run.out.contains("sudo"), "the plan says what sudo is for: {}", run.out);
}

// --- Whole runs on a made-up system --------------------------------------------------------------------------------

/// Stand-ins for everything that looks at the system: a healthy Fedora 44 with the package, the group, the
/// rule and a client in `/opt/hide.me`. Later definitions in a test replace them.
fn healthy(scratch: &Scratch) -> String {
    let s = scratch.s();
    for name in ["pkg.service", "pkg.rules", "sysusers.conf"] {
        write_file(&scratch.path().join(name), "x");
    }
    format!(
        "exec 2>&1; S=\"{s}\"; \
         resolve_user() {{ USER_NAME=tester; }}; user_acceptable() {{ return 0; }}; \
         gather_system_facts() {{ HAS_SYSTEMD=1; MACHINE=x86_64; IS_OSTREE=0; }}; \
         os_release_file() {{ echo \"$FIXTURES/os-release/fedora-44\"; }}; \
         is_blocked() {{ return 1; }}; \
         evaluate_client() {{ CLIENT_PATH=/opt/hide.me/hide.me; CLIENT_DIR=/opt/hide.me; CLIENT_STATE=ok; CLIENT_LIST=($CLIENT_PATH); }}; \
         service_client_version() {{ printf 0.9.14; }}; token_present() {{ return 0; }}; \
         installed_package_version() {{ printf 0.9.0-1; }}; \
         PACKAGE_UNIT=\"$S/pkg.service\"; PACKAGE_RULE=\"$S/pkg.rules\"; PACKAGE_SYSUSERS=\"$S/sysusers.conf\"; \
         MANUAL_UNIT=\"$S/manual.service\"; MANUAL_PROGRAM=\"$S/manual-celata\"; STATE_FILE=\"$S/state\"; DROPIN_FILE=\"$S/50.conf\"; \
         etc_rule_state() {{ echo absent; }}; unit_show() {{ cat \"$FIXTURES/systemctl-show/package-deb.txt\"; }}; \
         getent() {{ echo 'celata:x:990:tester'; }}; id() {{ echo 'tester celata'; }}; \
         systemctl() {{ case \"$1\" in is-active) echo inactive; return 3 ;; list-units) return 0 ;; esac; }}; \
         busctl() {{ echo 'NAME=org.kde.StatusNotifierWatcher'; }}; pkaction() {{ echo 'pkaction version 121'; }}; \
         sudo() {{ echo SUDO-WAS-CALLED; }}; "
    )
}

const CHECK_ORDER: [&str; 14] = [
    "System",
    "Internet",
    "hide.me client",
    "Client version",
    "Access-Token",
    "Celata package",
    "Service file",
    "Group",
    "Membership",
    "Session",
    "Rule for polkit",
    "DNS",
    "Tray icon area",
    "Other hide.me service",
];

/// Lines of the findings: (mark, name).
fn findings(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("  ")?;
            let (mark, rest) = rest.split_once(' ')?;
            let (name, _) = rest.split_once(": ")?;
            matches!(mark, "[OK]" | "[--]" | "!" | "?").then(|| (mark.to_owned(), name.to_owned()))
        })
        .collect()
}

#[test]
fn the_check_shows_all_fourteen_findings_in_order_and_changes_nothing() {
    let scratch = Scratch::new("check");
    let run = bash(&format!("{}main --check", healthy(&scratch)));
    assert_eq!(run.code, 0, "{}", run.out);
    let found = findings(&run.out);
    let names: Vec<&str> = found.iter().map(|(_, name)| name.as_str()).collect();
    assert_eq!(names, CHECK_ORDER);
    for (mark, name) in &found {
        let allowed = if name == "Session" {
            ["[OK]", "?"]
        } else {
            ["[OK]", "[OK]"]
        };
        assert!(allowed.contains(&mark.as_str()), "{name} shows {mark}");
    }
    assert!(
        run.out.contains("Celata 0.9.0 setup") && run.out.contains("Nothing is missing."),
        "{}",
        run.out
    );
    assert!(
        !run.out.contains("WAS-CALLED") && !scratch.path().join("log").exists(),
        "no rights were asked for: {}",
        run.out
    );
}

#[test]
fn the_check_reports_what_is_missing_with_return_value_1() {
    let scratch = Scratch::new("check-missing");
    let run = bash(&format!(
        "{}installed_package_version() {{ return 1; }}; PACKAGE_RULE=\"$S/none\"; getent() {{ return 2; }}; \
         unit_show() {{ cat \"$FIXTURES/systemctl-show/not-found.txt\"; }}; main --check",
        healthy(&scratch)
    ));
    assert_eq!(run.code, 1, "{}", run.out);
    let found = findings(&run.out);
    for name in [
        "Celata package",
        "Service file",
        "Group",
        "Membership",
        "Rule for polkit",
    ] {
        assert!(
            found.contains(&("[--]".to_owned(), name.to_owned())),
            "{name} is missing: {}",
            run.out
        );
    }
    assert!(
        run.out.contains("Open:") && run.out.contains("Celata package: not installed"),
        "{}",
        run.out
    );
}

#[test]
fn the_check_in_german_uses_the_words_of_celata() {
    let scratch = Scratch::new("check-de");
    let run = run_bash(
        &format!("{}detect_language; main --check", healthy(&scratch)),
        &[("LC_ALL", "de_DE.UTF-8")],
        "",
    );
    assert_eq!(run.code, 0, "{}", run.out);
    for word in [
        "Celata 0.9.0 – Einrichtung",
        "Dienst-Datei",
        "Regel für polkit",
        "Leiste für Symbole",
        "Mitgliedschaft",
        "Nichts fehlt.",
    ] {
        assert!(run.out.contains(word), "`{word}` missing: {}", run.out);
    }
}

#[test]
fn the_checkmarks_are_signs_only_in_a_utf_8_terminal() {
    let scratch = Scratch::new("marks");
    let run = run_bash(
        &format!("{}main --check", healthy(&scratch)),
        &[("LC_ALL", "C.UTF-8")],
        "",
    );
    assert!(run.out.contains("  ✓ System"), "{}", run.out);
    let plain = bash(&format!("{}main --check", healthy(&scratch)));
    assert!(
        plain.out.contains("  [OK] System") && !plain.out.contains('✓'),
        "{}",
        plain.out
    );
}

#[test]
fn a_setup_by_hand_on_nobara_is_recognised_and_left_alone() {
    let scratch = Scratch::new("nobara");
    let s = scratch.s();
    write_file(&scratch.path().join("manual.service"), "[Service]\n");
    let run = bash(&format!(
        "{}os_release_file() {{ echo \"$FIXTURES/os-release/nobara-44\"; }}; installed_package_version() {{ return 1; }}; \
         PACKAGE_UNIT=\"$S/none\"; PACKAGE_RULE=\"$S/none\"; PACKAGE_SYSUSERS=\"$S/none\"; MANUAL_UNIT=\"{s}/manual.service\"; \
         etc_rule_state() {{ echo same; }}; unit_show() {{ cat \"$FIXTURES/systemctl-show/manual-unit.txt\"; }}; main --check",
        healthy(&scratch)
    ));
    assert_eq!(run.code, 0, "{}", run.out);
    assert!(
        run.out.contains("Nobara Linux 44 as Fedora 44, not tested"),
        "{}",
        run.out
    );
    assert!(run.out.contains("set up by hand"), "{}", run.out);
    assert!(
        run.out.contains("rule under /etc in place (made by hand)"),
        "{}",
        run.out
    );
    assert!(
        !run.out.contains("[--]"),
        "no failure on a working setup by hand: {}",
        run.out
    );
}

#[test]
fn the_dry_run_shows_the_plan_and_calls_neither_rights_nor_network() {
    let scratch = Scratch::new("dry");
    let run = bash(&format!(
        "{}installed_package_version() {{ return 1; }}; PACKAGE_RULE=\"$S/none\"; getent() {{ return 2; }}; \
         as_root() {{ echo AS_ROOT-WAS-CALLED; }}; fetch() {{ echo FETCH-WAS-CALLED; }}; SCRIPT_DIR=\"$S/empty\"; mkdir -p \"$SCRIPT_DIR\"; \
         main --dry-run",
        healthy(&scratch)
    ));
    assert_eq!(run.code, 0, "{}", run.out);
    assert!(!run.out.contains("WAS-CALLED"), "{}", run.out);
    assert!(
        run.out.contains(
            "would download: https://github.com/derHouwer/celata/releases/download/v0.9.0/celata-0.9.0-1.x86_64.rpm"
        ),
        "{}",
        run.out
    );
    assert!(
        run.out
            .contains("would run: dnf install --assumeno /var/tmp/celata-setup.XXXXXXXX/celata-0.9.0-1.x86_64.rpm"),
        "{}",
        run.out
    );
    assert!(
        run.out.contains("1. Install the Celata package (dnf install -y"),
        "{}",
        run.out
    );
    assert!(
        run.out
            .contains("Add your account to the group celata (usermod -aG celata tester)"),
        "{}",
        run.out
    );
}

#[test]
fn the_dry_run_stops_where_a_real_run_would_stop() {
    let scratch = Scratch::new("dry-stops");
    let missing_package = "installed_package_version() { return 1; }; PACKAGE_RULE=\"$S/none\"; SCRIPT_DIR=\"$S/empty\"; mkdir -p \"$SCRIPT_DIR\"; ";
    let blocked = bash(&format!(
        "{}{missing_package}is_blocked() {{ return 0; }}; main --dry-run",
        healthy(&scratch)
    ));
    assert_eq!(blocked.code, 3, "{}", blocked.out);
    assert!(
        blocked.out.contains("The internet is blocked") && blocked.out.contains("celata --unblock"),
        "{}",
        blocked.out
    );
    let none = bash(&format!(
        "{}{missing_package}evaluate_client() {{ CLIENT_STATE=missing; CLIENT_LIST=(); }}; main --dry-run",
        healthy(&scratch)
    ));
    assert_eq!(none.code, 3, "{}", none.out);
    assert!(
        none.out.contains("hide.client.linux"),
        "the way to the guide of hide.me: {}",
        none.out
    );
    write_file(&scratch.path().join("empty/celata-0.9.0-1.x86_64.rpm"), "wrong");
    let script_sum = out("sha256sum \"$SCRIPT\" | cut -d' ' -f1");
    write_file(
        &scratch.path().join("empty/SHA256SUMS"),
        &format!(
            "{}  celata-0.9.0-1.x86_64.rpm\n{script_sum}  celata-setup.sh\n",
            "0".repeat(64)
        ),
    );
    let wrong = bash(&format!("{}{missing_package}main --dry-run", healthy(&scratch)));
    assert_eq!(wrong.code, 3, "{}", wrong.out);
    assert!(wrong.out.contains("does not match SHA256SUMS"), "{}", wrong.out);
}

#[test]
fn a_complete_system_asks_for_no_rights() {
    let scratch = Scratch::new("complete");
    let run = bash(&format!(
        "{}as_root() {{ echo AS_ROOT-WAS-CALLED; }}; main",
        healthy(&scratch)
    ));
    assert_eq!(run.code, 0, "{}", run.out);
    assert!(
        run.out.contains("Everything is set up. There is nothing to do."),
        "{}",
        run.out
    );
    assert!(
        !run.out.contains("WAS-CALLED") && !run.out.contains("Run this now?"),
        "{}",
        run.out
    );
}

/// A made-up system in which the package is missing and the group appears when the package manager has run.
fn fresh_world(scratch: &Scratch) -> String {
    let s = scratch.s();
    format!(
        "{}installed_package_version() {{ return 1; }}; PACKAGE_RULE=\"$S/none\"; getent() {{ [[ -e \"$S/group-made\" ]] && echo 'celata:x:990:'; }}; \
         id() {{ echo 'tester'; }}; sudo_available() {{ return 0; }}; \
         locate_package() {{ PKG_PATH=\"$S/celata-0.9.0-1.x86_64.rpm\"; PKG_FROM=local; EXPECTED_SUM=abc; }}; \
         probe_package() {{ echo PROBE; }}; fetch() {{ echo FETCH-WAS-CALLED; }}; \
         as_root() {{ local IFS=' '; echo \"AS_ROOT $*\" >&2; case \"$1\" in \
           mktemp) echo /var/tmp/celata-setup.TESTDIR1 ;; \
           sha256sum) printf 'abc  %s\\n' \"$3\" ;; \
           dnf) touch \"$S/group-made\" ;; \
         esac; }}; ",
        healthy(scratch)
    ) + &format!("printf x > \"{s}/celata-0.9.0-1.x86_64.rpm\"; ")
}

#[test]
fn a_set_up_run_shows_the_plan_asks_once_and_works_through_the_steps() {
    let scratch = Scratch::new("setup");
    let snippet = format!("{}main", fresh_world(&scratch));
    let run = run_bash(&snippet, &[], "y\n");
    // The stand-in system does not change while the steps run, so the findings at the end still show gaps.
    assert_eq!(run.code, 1, "out: {}\nerr: {}", run.out, run.err);
    let text = format!("{}{}", run.out, run.err);
    let position = |needle: &str| {
        text.find(needle)
            .unwrap_or_else(|| panic!("`{needle}` missing in {text}"))
    };
    assert!(position("This is what will be done:") < position("Run this now? [Y/n]"));
    assert!(
        position("Run this now? [Y/n]") < position("AS_ROOT mktemp"),
        "no rights before the question is answered"
    );
    assert!(
        position("AS_ROOT dnf install -y /var/tmp/celata-setup.TESTDIR1/celata-0.9.0-1.x86_64.rpm")
            < position("AS_ROOT usermod -aG celata -- tester")
    );
    assert!(position("AS_ROOT usermod -aG celata -- tester") < position("AS_ROOT mv -f -- "));
    assert!(
        text.contains("Install the Celata package: done")
            && text.contains("Add your account to the group celata: done"),
        "{text}"
    );
    assert!(!text.contains("FETCH-WAS-CALLED"), "{text}");
    assert!(
        text.contains("Log out and log in again once"),
        "the last words tell what is left to do: {text}"
    );
}

#[test]
fn blocked_internet_or_a_bad_client_stops_a_run_before_any_rights_are_asked_for() {
    let scratch = Scratch::new("early-stop");
    let log = "echo \"AS_ROOT $*\" >&2";
    let blocked = run_bash(
        &format!(
            "{}is_blocked() {{ return 0; }}; sudo_available() {{ echo SUDO-AVAILABLE-WAS-ASKED; return 0; }}; as_root() {{ {log}; }}; main",
            fresh_world(&scratch)
        ),
        &[],
        "",
    );
    let text = format!("{}{}", blocked.out, blocked.err);
    assert_eq!(blocked.code, 3, "{text}");
    assert!(
        text.contains("celata --unblock"),
        "the way out of the block is named: {text}"
    );
    assert!(
        !text.contains("I am checking now") && !text.contains("AS_ROOT") && !text.contains("SUDO-AVAILABLE"),
        "{text}"
    );
    let no_client = run_bash(
        &format!(
            "{}evaluate_client() {{ CLIENT_STATE=missing; CLIENT_LIST=(); }}; sudo_available() {{ echo SUDO-AVAILABLE-WAS-ASKED; return 0; }}; as_root() {{ {log}; }}; main",
            fresh_world(&scratch)
        ),
        &[],
        "",
    );
    let text = format!("{}{}", no_client.out, no_client.err);
    assert_eq!(no_client.code, 3, "{text}");
    assert!(
        !text.contains("I am checking now") && !text.contains("AS_ROOT") && !text.contains("SUDO-AVAILABLE"),
        "{text}"
    );
}

#[test]
fn a_block_stops_a_run_that_has_only_a_question_about_the_package_in_its_plan() {
    let scratch = Scratch::new("early-stop-question");
    let s = scratch.s();
    write_file(&scratch.path().join("manual.service"), "[Service]\n");
    // The owner's computer: a setup by hand, so the plan holds only the package, which asks on its own.
    let world = format!(
        "{}os_release_file() {{ echo \"$FIXTURES/os-release/nobara-44\"; }}; installed_package_version() {{ return 1; }}; \
         PACKAGE_UNIT=\"$S/none\"; PACKAGE_RULE=\"$S/none\"; MANUAL_UNIT=\"{s}/manual.service\"; etc_rule_state() {{ echo same; }}; \
         unit_show() {{ cat \"$FIXTURES/systemctl-show/manual-unit.txt\"; }}; getent() {{ echo 'celata:x:990:tester'; }}; \
         is_blocked() {{ return 0; }}; as_root() {{ echo AS_ROOT-WAS-CALLED; }}; ",
        fresh_world(&scratch)
    );
    let run = run_bash(&format!("{world}main"), &[], "y\ny\n");
    let text = format!("{}{}", run.out, run.err);
    assert_eq!(run.code, 3, "{text}");
    assert!(
        text.contains("Continue? [y/N]"),
        "the question about the derivative comes first: {text}"
    );
    assert!(
        text.contains("celata --unblock"),
        "the way out of the block is named: {text}"
    );
    assert!(
        !text.contains("Do it anyway") && !text.contains("WAS-CALLED"),
        "no question about the package, no rights: {text}"
    );
}

#[test]
fn a_derivative_is_asked_before_any_rights_or_network_and_no_ends_the_run() {
    let scratch = Scratch::new("derivative");
    let world = format!(
        "{}os_release_file() {{ echo \"$FIXTURES/os-release/nobara-44\"; }}; ",
        fresh_world(&scratch)
    );
    let declined = run_bash(&format!("{world}main"), &[], "n\n");
    assert_eq!(declined.code, 4, "out: {}", declined.out);
    let text = format!("{}{}", declined.out, declined.err);
    assert!(
        text.contains("Nobara Linux 44 is set up as Fedora 44. This system is not tested."),
        "{text}"
    );
    assert!(text.contains("Continue? [y/N]"), "{text}");
    assert!(
        !text.contains("AS_ROOT") && !text.contains("PROBE") && !text.contains("FETCH-WAS-CALLED"),
        "nothing ran: {text}"
    );
    let accepted = run_bash(&format!("{world}main"), &[], "y\ny\n");
    let text = format!("{}{}", accepted.out, accepted.err);
    assert!(
        text.find("This system is not tested").unwrap() < text.find("I am checking now").unwrap(),
        "{text}"
    );
    assert!(
        text.find("Continue? [y/N]").unwrap() < text.find("AS_ROOT mktemp").unwrap(),
        "{text}"
    );
    let silent = run_bash(&format!("{world}main --yes"), &[], "");
    assert!(
        format!("{}{}", silent.out, silent.err).contains("yes (--yes)"),
        "--yes answers the question"
    );
}

#[test]
fn linux_mint_22_is_tested_and_asks_nothing() {
    let scratch = Scratch::new("mint");
    let world = format!(
        "{}os_release_file() {{ echo \"$FIXTURES/os-release/linuxmint-22\"; }}; CL_FAMILY_FORCE=deb; ",
        fresh_world(&scratch)
    );
    let run = run_bash(&format!("{world}main --dry-run"), &[], "");
    assert!(
        !run.out.contains("Continue?") && !run.out.contains("not tested"),
        "{}",
        run.out
    );
    assert!(
        run.out.contains("celata_0.9.0-1_amd64.deb"),
        "the package of Ubuntu 24.04 is used: {}",
        run.out
    );
}

#[test]
fn a_package_from_before_the_script_asks_on_its_own_and_a_no_leaves_it_alone() {
    let scratch = Scratch::new("preexisting");
    let world = format!(
        "{}installed_package_version() {{ printf 0.8.0-1; }}; PACKAGE_RULE=\"$S/pkg.rules\"; getent() {{ echo 'celata:x:990:tester'; }}; ",
        fresh_world(&scratch)
    );
    // The question about the older package comes before sudo, network or the question about the whole run.
    let declined = run_bash(&format!("{world}main"), &[], "n\n");
    let text = format!("{}{}", declined.out, declined.err);
    assert!(
        text.contains("the package was installed before this script. Update it? [y/N]"),
        "{text}"
    );
    assert!(!text.contains("AS_ROOT") && !text.contains("PROBE"), "{text}");
    assert_eq!(declined.code, 1, "a step that was left out counts as open: {text}");
    assert!(text.contains("not done, you said no"), "{text}");
    let yes_flag = run_bash(&format!("{world}main --yes"), &[], "");
    let text = format!("{}{}", yes_flag.out, yes_flag.err);
    assert!(
        !text.contains("AS_ROOT"),
        "--yes never agrees to touching a package from before: {text}"
    );
}

#[test]
fn a_setup_by_hand_is_checked_and_the_package_is_declined_without_a_single_change() {
    let scratch = Scratch::new("by-hand-run");
    let s = scratch.s();
    write_file(&scratch.path().join("manual.service"), "[Service]\n");
    let world = format!(
        "{}os_release_file() {{ echo \"$FIXTURES/os-release/nobara-44\"; }}; installed_package_version() {{ return 1; }}; \
         PACKAGE_UNIT=\"$S/none\"; PACKAGE_RULE=\"$S/none\"; MANUAL_UNIT=\"{s}/manual.service\"; etc_rule_state() {{ echo same; }}; \
         unit_show() {{ cat \"$FIXTURES/systemctl-show/manual-unit.txt\"; }}; getent() {{ echo 'celata:x:990:tester'; }}; \
         as_root() {{ echo AS_ROOT-WAS-CALLED; }}; fetch() {{ echo FETCH-WAS-CALLED; }}; ",
        fresh_world(&scratch)
    );
    let run = run_bash(&format!("{world}main"), &[], "y\nn\n");
    let text = format!("{}{}", run.out, run.err);
    assert!(!text.contains("WAS-CALLED"), "no rights, no network: {text}");
    assert!(
        text.contains("Celata is set up by hand on this system"),
        "the package question explains it: {text}"
    );
    assert_eq!(run.code, 1, "the declined package is listed as open: {text}");
    assert!(
        text.contains("Install the Celata package: not done, you said no"),
        "{text}"
    );
    assert!(!text.contains("[--]"), "no finding fails: {text}");
}

#[test]
fn the_removal_stops_the_service_removes_the_parts_and_names_what_stays() {
    let scratch = Scratch::new("remove");
    let s = scratch.s();
    write_file(&scratch.path().join("50.conf"), "# X-Celata-Setup=1\n[Service]\n");
    write_file(&scratch.path().join("state"), "format=1\npackage=setup\n");
    let world = format!(
        "{}systemctl() {{ case \"$1\" in is-active) echo active ;; list-units) return 0 ;; esac; }}; \
         as_root() {{ local IFS=' '; echo \"AS_ROOT $*\" >&2; }}; getent() {{ echo 'celata:x:990:tester'; }}; ",
        healthy(&scratch)
    );
    let run = run_bash(&format!("{world}main --uninstall"), &[], "y\n");
    let text = format!("{}{}", run.out, run.err);
    assert_eq!(run.code, 0, "{text}");
    assert!(
        text.contains("The service is stopped. A connection is cut and a block is lifted."),
        "{text}"
    );
    let order = [
        "AS_ROOT systemctl stop celata-hide.me.service",
        &format!("AS_ROOT rm -f -- {s}/50.conf"),
        "AS_ROOT systemctl daemon-reload",
        "AS_ROOT dnf remove -y celata",
        &format!("AS_ROOT rm -f -- {s}/state"),
    ];
    let mut last = 0;
    for command in order {
        let at = text
            .find(command)
            .unwrap_or_else(|| panic!("`{command}` missing: {text}"));
        assert!(at >= last, "`{command}` is out of order: {text}");
        last = at;
    }
    for stays in [
        "What stays on the system:",
        "the group celata (remove it with: sudo groupdel celata)",
        "the hide.me client and its files",
        "~/.config/celata, ~/.cache/celata and ~/.config/autostart/celata.desktop",
    ] {
        assert!(text.contains(stays), "`{stays}` missing: {text}");
    }
}

#[test]
fn a_removal_never_removes_a_package_from_before_the_script_with_yes() {
    let scratch = Scratch::new("remove-pre");
    write_file(&scratch.path().join("state"), "format=1\n");
    let world = format!(
        "{}as_root() {{ local IFS=' '; echo \"AS_ROOT $*\" >&2; }}; ",
        healthy(&scratch)
    );
    let run = run_bash(&format!("{world}main --uninstall --yes"), &[], "");
    let text = format!("{}{}", run.out, run.err);
    assert_eq!(run.code, 1, "a step that was left out counts as open: {text}");
    assert!(!text.contains("AS_ROOT dnf remove"), "{text}");
    assert!(
        text.contains("the package celata (remove it with: sudo dnf remove -y celata)"),
        "{text}"
    );
    assert!(
        text.contains("Remove the Celata package: not done, you said no"),
        "{text}"
    );
}

#[test]
fn a_setup_by_hand_is_named_in_a_removal_and_never_removed() {
    let scratch = Scratch::new("remove-manual");
    let s = scratch.s();
    write_file(&scratch.path().join("manual.service"), "[Service]\n");
    let world = format!(
        "{}installed_package_version() {{ return 1; }}; MANUAL_UNIT=\"{s}/manual.service\"; etc_rule_state() {{ echo same; }}; \
         as_root() {{ echo AS_ROOT-WAS-CALLED; }}; ",
        healthy(&scratch)
    );
    let run = bash(&format!("{world}main --uninstall --yes"));
    assert_eq!(run.code, 0, "{}", run.out);
    assert!(!run.out.contains("WAS-CALLED"), "{}", run.out);
    assert!(
        run.out.contains("There is nothing to remove that this script set up."),
        "{}",
        run.out
    );
    assert!(
        run.out
            .contains(&format!("the service file {s}/manual.service that was made by hand")),
        "{}",
        run.out
    );
    assert!(
        run.out
            .contains("the rule /etc/polkit-1/rules.d/49-celata.rules that was made by hand"),
        "{}",
        run.out
    );
    assert!(run.out.contains("Removing the setup"), "{}", run.out);
}

#[test]
fn the_removal_dry_run_shows_commands_and_changes_nothing() {
    let scratch = Scratch::new("remove-dry");
    write_file(&scratch.path().join("state"), "format=1\npackage=setup\n");
    let world = format!("{}as_root() {{ echo AS_ROOT-WAS-CALLED; }}; ", healthy(&scratch));
    let run = bash(&format!("{world}main --uninstall --dry-run"));
    assert_eq!(run.code, 0, "{}", run.out);
    assert!(!run.out.contains("WAS-CALLED"), "{}", run.out);
    assert!(
        run.out.contains("Remove the Celata package (dnf remove -y celata)"),
        "{}",
        run.out
    );
    assert!(run.out.contains("What stays on the system:"), "{}", run.out);
}

#[test]
fn an_interrupt_is_reported_and_the_work_folder_is_removed() {
    let scratch = Scratch::new("interrupt");
    let s = scratch.s();
    let snippet = format!(
        "{}trap on_exit EXIT; trap on_signal INT TERM; make_workdir; echo \"$WORKDIR\" > \"{s}/workdir\"; kill -INT $$; sleep 1; echo not-reached",
        healthy(&scratch)
    );
    let run = bash(&snippet);
    assert_eq!(run.code, 4, "{}", run.out);
    assert!(
        run.out.contains("Aborted. A new run completes the setup."),
        "{}",
        run.out
    );
    assert!(!run.out.contains("not-reached"));
    let folder = fs::read_to_string(scratch.path().join("workdir")).unwrap();
    assert!(!Path::new(folder.trim()).exists(), "the work folder is gone");
}

#[test]
fn files_are_written_under_a_temporary_name_and_renamed() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("atomic");
    let s = scratch.s();
    let run = run_bash(&format!("write_atomic \"{s}/file\" 0640"), &[], "content\n");
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(fs::read_to_string(scratch.path().join("file")).unwrap(), "content\n");
    assert_eq!(
        fs::metadata(scratch.path().join("file")).unwrap().permissions().mode() & 0o777,
        0o640
    );
    let leftovers: Vec<_> = fs::read_dir(scratch.path())
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(leftovers.len(), 1, "no temporary file stays: {leftovers:?}");
}

/// A unit of the hide.me client in its own template; written in two parts because the text would look like a
/// mail address.
fn other_unit() -> String {
    format!("{}{}", "hide.me", "@probe.service")
}

#[test]
fn findings_of_dns_tray_and_other_services_are_read_from_the_answers_of_the_system() {
    assert_eq!(out("dns_state active"), "resolved");
    assert_eq!(out("dns_state inactive"), "none");
    assert_eq!(out("dns_state ''"), "none");
    assert_eq!(out("tray_state 0 ''"), "present");
    assert_eq!(
        out(
            "tray_state 1 \"Failed to get credentials: Could not get PID of name 'org.kde.StatusNotifierWatcher': no such name\""
        ),
        "absent"
    );
    assert_eq!(
        out("tray_state 1 'Failed to connect to bus: No such file or directory'"),
        "unknown",
        "no session bus"
    );
    assert_eq!(out("other_service_state ''"), "");
    let unit = other_unit();
    assert_eq!(
        out(&format!("other_service_state '{unit} loaded active running hide.me'")),
        unit
    );
    assert_eq!(
        out("other_service_state '  unrelated.service loaded active running x'"),
        ""
    );
}

#[test]
fn the_dns_hint_names_the_leak_and_changes_nothing() {
    let scratch = Scratch::new("dns");
    let world = format!(
        "{}systemctl() {{ case \"$1\" in is-active) echo active ;; list-units) return 0 ;; esac; }}; ",
        healthy(&scratch)
    );
    let run = bash(&format!("{world}main --check"));
    let found = findings(&run.out);
    assert!(found.contains(&("!".to_owned(), "DNS".to_owned())), "{}", run.out);
    assert!(
        run.out
            .contains("systemd-resolved is running, DNS requests can bypass the VPN"),
        "{}",
        run.out
    );
    assert!(run.out.contains("docs/dns-leak.md"), "{}", run.out);
    assert_eq!(run.code, 0, "a hint is no failure");
    let unit = other_unit();
    let other = bash(&format!(
        "{}systemctl() {{ case \"$1\" in is-active) echo inactive ;; list-units) echo '{unit} loaded active running x' ;; esac; }}; main --check",
        healthy(&scratch)
    ));
    assert!(
        other.out.contains(&format!("{unit} is running")) && other.out.contains(&format!("sudo systemctl stop {unit}")),
        "{}",
        other.out
    );
}

#[test]
fn a_missing_tray_is_a_hint_and_not_a_failure() {
    let scratch = Scratch::new("tray");
    let run = bash(&format!(
        "{}busctl() {{ echo \"no such name\"; return 1; }}; main --check",
        healthy(&scratch)
    ));
    assert!(
        findings(&run.out).contains(&("!".to_owned(), "Tray icon area".to_owned())),
        "{}",
        run.out
    );
    assert_eq!(run.code, 0);
}
