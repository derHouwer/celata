// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Running the few system tools Celata reads from (`ip`, `systemctl`).
//!
//! Always a fixed path, never a shell, an empty environment, no input, and a bounded output.

use std::io::{self, Read};
use std::process::{Command, Stdio};

/// Runs the first existing program of `candidates` with `args` and returns its standard output.
///
/// Errors: none of the programs exists (`NotFound`), the program fails, its output is not UTF-8 or longer
/// than `limit` bytes (`InvalidData`; the program is stopped then).
pub fn run(candidates: &[&str], args: &[&str], limit: u64) -> io::Result<String> {
    let program = candidates
        .iter()
        .find(|path| std::path::Path::new(path).is_file())
        .ok_or(io::ErrorKind::NotFound)?;
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut output = Vec::new();
    let read = match child.stdout.take() {
        Some(stdout) => stdout
            .take(limit.saturating_add(1))
            .read_to_end(&mut output)
            .map(|_| ()),
        None => Err(io::Error::other("no output pipe")),
    };
    let too_long = u64::try_from(output.len()).map_or(true, |length| length > limit);
    if read.is_err() || too_long {
        // The program may still be writing: stop it instead of waiting for it.
        let _ = child.kill();
    }
    let status = child.wait()?;
    read?;
    if too_long {
        return Err(io::ErrorKind::InvalidData.into());
    }
    if !status.success() {
        return Err(io::Error::other("the program reported an error"));
    }
    String::from_utf8(output).map_err(|_| io::ErrorKind::InvalidData.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRINTF: [&str; 2] = ["/usr/bin/printf", "/bin/printf"];
    const HEAD: [&str; 2] = ["/usr/bin/head", "/bin/head"];
    const ENV: [&str; 2] = ["/usr/bin/env", "/bin/env"];
    const FALSE: [&str; 2] = ["/usr/bin/false", "/bin/false"];

    #[test]
    fn returns_the_output() {
        assert_eq!(run(&PRINTF, &["a b\\n"], 100).unwrap(), "a b\n");
        assert_eq!(run(&PRINTF, &[""], 100).unwrap(), "");
    }

    #[test]
    fn takes_the_first_existing_program() {
        assert_eq!(
            run(&["/nonexistent/printf", PRINTF[0], PRINTF[1]], &["x"], 10).unwrap(),
            "x"
        );
    }

    #[test]
    fn missing_program_is_not_found() {
        assert_eq!(
            run(&["/nonexistent/a", "/nonexistent/b"], &[], 10).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(run(&[], &[], 10).unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn output_is_bounded() {
        assert_eq!(run(&HEAD, &["-c", "10", "/dev/zero"], 10).unwrap().len(), 10);
        assert_eq!(
            run(&HEAD, &["-c", "11", "/dev/zero"], 10).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        // Far more than fits into a pipe: the program must be stopped, not waited for.
        assert_eq!(
            run(&HEAD, &["-c", "50000000", "/dev/zero"], 10).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn failing_program_is_an_error() {
        assert!(run(&FALSE, &[], 10).is_err());
    }

    #[test]
    fn environment_is_empty() {
        assert_eq!(run(&ENV, &[], 10_000).unwrap(), "");
    }

    #[test]
    fn output_must_be_text() {
        assert_eq!(
            run(&PRINTF, &["\\377"], 10).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
