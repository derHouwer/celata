// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 derHouwer

//! Minimal HTTP/1.1 over the Unix socket of the service.
//!
//! The module is deliberately not public: only `client.rs` decides which paths are called. Answers are
//! never logged here, because they can contain connection keys.

use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use crate::i18n::{Key, Language, Phrase};

/// Largest answer or line that is accepted.
pub(crate) const MAX_RESPONSE: u64 = 1024 * 1024;
const MAX_HEAD: u64 = 64 * 1024;
/// Time limit for sending a call.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// Highest number of trailer lines after the last chunk of a `chunked` answer.
const MAX_TRAILER_LINES: usize = 32;

/// Errors of the connection to the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The socket is missing or accepts no connection.
    NotRunning,
    /// The user lacks the permission for the socket.
    PermissionDenied,
    /// The service is busy (HTTP 409).
    Busy,
    /// No answer arrived in time.
    Timeout,
    /// The connection broke during the call.
    ConnectionLost,
    /// The service answered with another HTTP error.
    Status(u16),
    /// The answer is unreadable.
    InvalidResponse,
    /// The answer exceeds the accepted size.
    ResponseTooLarge,
    /// The service reported an error.
    Service(String),
    /// The server name has an invalid form.
    InvalidHost,
}

impl Error {
    /// Message for the display, translated only when it is shown. The error text of the service stays
    /// unchanged.
    pub fn phrase(&self) -> Phrase {
        let key = match self {
            Self::NotRunning => Key::ErrorNotRunning,
            Self::PermissionDenied => Key::ErrorPermissionDenied,
            Self::Busy => Key::ErrorBusy,
            Self::Timeout => Key::ErrorTimeout,
            Self::ConnectionLost => Key::ErrorConnectionLost,
            Self::Status(code) => return Phrase::fill(Key::ErrorStatus, [Phrase::raw(code.to_string())]),
            Self::InvalidResponse => Key::ErrorInvalidResponse,
            Self::ResponseTooLarge => Key::ErrorResponseTooLarge,
            Self::Service(message) => return Phrase::fill(Key::ErrorService, [Phrase::raw(message.clone())]),
            Self::InvalidHost => Key::ErrorInvalidHost,
        };
        Phrase::key(key)
    }
}

/// Only for debugging and `std::error::Error`; the display uses [`Error::phrase`].
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.phrase().render(Language::En))
    }
}

impl std::error::Error for Error {}

fn map_io(error: &io::Error) -> Error {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => Error::NotRunning,
        io::ErrorKind::PermissionDenied => Error::PermissionDenied,
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => Error::Timeout,
        io::ErrorKind::InvalidData => Error::InvalidResponse,
        _ => Error::ConnectionLost,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Method {
    Get,
    Post,
}

#[derive(Debug, PartialEq, Eq)]
enum Framing {
    Length(u64),
    Chunked,
    UntilEof,
}

#[derive(Debug, PartialEq, Eq)]
struct Head {
    status: u16,
    framing: Framing,
}

fn read_head<R: BufRead>(reader: &mut R) -> Result<Head, Error> {
    let mut limited = reader.take(MAX_HEAD);
    let mut line = String::new();
    limited.read_line(&mut line).map_err(|e| map_io(&e))?;
    if line.is_empty() {
        return Err(Error::ConnectionLost);
    }
    let status = line
        .strip_prefix("HTTP/1.")
        .and_then(|rest| rest.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or(Error::InvalidResponse)?;

    let mut framing = Framing::UntilEof;
    loop {
        line.clear();
        limited.read_line(&mut line).map_err(|e| map_io(&e))?;
        if !line.ends_with('\n') {
            return Err(Error::InvalidResponse);
        }
        let header = line.trim_end();
        if header.is_empty() {
            return Ok(Head { status, framing });
        }
        let Some((name, value)) = header.split_once(':') else {
            return Err(Error::InvalidResponse);
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("transfer-encoding") {
            if !value.eq_ignore_ascii_case("chunked") {
                return Err(Error::InvalidResponse);
            }
            framing = Framing::Chunked;
        } else if name.eq_ignore_ascii_case("content-length") && framing != Framing::Chunked {
            framing = Framing::Length(value.parse().map_err(|_| Error::InvalidResponse)?);
        }
    }
}

/// Reads the body of an answer and removes the `chunked` encoding on the way.
struct Body<R> {
    inner: R,
    framing: Framing,
    chunk_left: u64,
    finished: bool,
}

impl<R: BufRead> Body<R> {
    fn new(inner: R, framing: Framing) -> Self {
        Self {
            inner,
            framing,
            chunk_left: 0,
            finished: false,
        }
    }

    fn read_chunk_size(&mut self) -> io::Result<u64> {
        let mut line = String::new();
        (&mut self.inner).take(64).read_line(&mut line)?;
        if line.is_empty() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let digits = line.trim_end().split(';').next().unwrap_or_default();
        u64::from_str_radix(digits, 16).map_err(|_| io::ErrorKind::InvalidData.into())
    }

    fn skip_line(&mut self) -> io::Result<String> {
        let mut line = String::new();
        (&mut self.inner).take(MAX_HEAD).read_line(&mut line)?;
        if !line.ends_with('\n') {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        Ok(line)
    }

    /// Skips the trailer lines up to the empty line; more than [`MAX_TRAILER_LINES`] are an error.
    fn skip_trailers(&mut self) -> io::Result<()> {
        for _ in 0..=MAX_TRAILER_LINES {
            if self.skip_line()?.trim_end().is_empty() {
                return Ok(());
            }
        }
        Err(io::ErrorKind::InvalidData.into())
    }
}

impl<R: BufRead> Read for Body<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.finished || buffer.is_empty() {
            return Ok(0);
        }
        match self.framing {
            Framing::UntilEof => self.inner.read(buffer),
            Framing::Length(left) => {
                if left == 0 {
                    self.finished = true;
                    return Ok(0);
                }
                let wanted = buffer.len().min(usize::try_from(left).unwrap_or(usize::MAX));
                let count = self.inner.read(&mut buffer[..wanted])?;
                if count == 0 {
                    return Err(io::ErrorKind::UnexpectedEof.into());
                }
                self.framing = Framing::Length(left - count as u64);
                Ok(count)
            }
            Framing::Chunked => {
                if self.chunk_left == 0 {
                    self.chunk_left = self.read_chunk_size()?;
                    if self.chunk_left == 0 {
                        self.skip_trailers()?;
                        self.finished = true;
                        return Ok(0);
                    }
                }
                let wanted = buffer.len().min(usize::try_from(self.chunk_left).unwrap_or(usize::MAX));
                let count = self.inner.read(&mut buffer[..wanted])?;
                if count == 0 {
                    return Err(io::ErrorKind::UnexpectedEof.into());
                }
                self.chunk_left -= count as u64;
                if self.chunk_left == 0 {
                    self.skip_line()?;
                }
                Ok(count)
            }
        }
    }
}

fn check_status(status: u16) -> Result<(), Error> {
    match status {
        200..=299 => Ok(()),
        409 => Err(Error::Busy),
        other => Err(Error::Status(other)),
    }
}

/// Longest error text of the service that is shown.
const MAX_SERVICE_MESSAGE: usize = 300;

/// Error text of the service for the display: without control characters and limited. Applies to every
/// way in which the service reports an error.
pub(crate) fn service_error(message: &str) -> Error {
    Error::Service(crate::text::printable(message, MAX_SERVICE_MESSAGE))
}

/// On HTTP 400, reads the error text of the service (`{"error":{"message":…}}`), for example when it
/// refuses a setting. Without a readable text the result stays [`Error::Status`].
fn rejection<R: BufRead>(reader: R, framing: Framing) -> Error {
    #[derive(serde::Deserialize)]
    struct Rejection {
        error: Failure,
    }
    #[derive(serde::Deserialize)]
    struct Failure {
        message: String,
    }
    let mut content = Vec::new();
    if Body::new(reader, framing)
        .take(MAX_HEAD)
        .read_to_end(&mut content)
        .is_err()
    {
        return Error::Status(400);
    }
    match serde_json::from_slice::<Rejection>(&content) {
        Ok(rejection) => service_error(&rejection.error.message),
        Err(_) => Error::Status(400),
    }
}

fn read_response<R: BufRead>(mut reader: R) -> Result<Vec<u8>, Error> {
    let head = read_head(&mut reader)?;
    if head.status == 400 {
        return Err(rejection(reader, head.framing));
    }
    check_status(head.status)?;
    let mut content = Vec::new();
    Body::new(reader, head.framing)
        .take(MAX_RESPONSE + 1)
        .read_to_end(&mut content)
        .map_err(|e| map_io(&e))?;
    if content.len() as u64 > MAX_RESPONSE {
        return Err(Error::ResponseTooLarge);
    }
    Ok(content)
}

fn send(
    socket: &Path,
    method: Method,
    target: &str,
    content: Option<&[u8]>,
    timeout: Option<Duration>,
) -> Result<UnixStream, Error> {
    let mut stream = UnixStream::connect(socket).map_err(|e| map_io(&e))?;
    stream.set_read_timeout(timeout).map_err(|e| map_io(&e))?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT)).map_err(|e| map_io(&e))?;
    let verb = match method {
        Method::Get => "GET",
        Method::Post => "POST",
    };
    let mut request = format!("{verb} {target} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n").into_bytes();
    if let Some(content) = content {
        request.extend_from_slice(
            format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                content.len()
            )
            .as_bytes(),
        );
    }
    request.extend_from_slice(b"\r\n");
    if let Some(content) = content {
        request.extend_from_slice(content);
    }
    stream.write_all(&request).map_err(|e| map_io(&e))?;
    Ok(stream)
}

/// Performs a single call and returns the body of the answer.
pub(crate) fn request(
    socket: &Path,
    method: Method,
    target: &str,
    content: Option<&[u8]>,
    timeout: Duration,
) -> Result<Vec<u8>, Error> {
    let stream = send(socket, method, target, content, Some(timeout))?;
    read_response(BufReader::new(stream))
}

/// Reads a continuous answer line by line.
pub(crate) struct LineStream {
    state: LineState,
}

enum LineState {
    Waiting(BufReader<UnixStream>),
    Reading(BufReader<Body<BufReader<UnixStream>>>),
    Finished,
}

impl LineStream {
    /// Opens the answer. It has no time limit: the service sends something only with the first event, and
    /// when the service ends, the socket closes as well.
    pub(crate) fn open(socket: &Path, target: &str) -> Result<Self, Error> {
        let stream = send(socket, Method::Get, target, None, None)?;
        Ok(Self {
            state: LineState::Waiting(BufReader::new(stream)),
        })
    }

    /// Returns the next line, or `None` when the answer has ended.
    pub(crate) fn next_line(&mut self) -> Result<Option<Vec<u8>>, Error> {
        if let LineState::Waiting(_) = self.state {
            let LineState::Waiting(mut reader) = std::mem::replace(&mut self.state, LineState::Finished) else {
                return Ok(None);
            };
            let head = read_head(&mut reader)?;
            check_status(head.status)?;
            self.state = LineState::Reading(BufReader::new(Body::new(reader, head.framing)));
        }
        let LineState::Reading(reader) = &mut self.state else {
            return Ok(None);
        };
        read_limited_line(reader)
    }
}

fn read_limited_line<R: BufRead>(reader: &mut R) -> Result<Option<Vec<u8>>, Error> {
    let mut line = Vec::new();
    reader
        .take(MAX_RESPONSE + 1)
        .read_until(b'\n', &mut line)
        .map_err(|e| map_io(&e))?;
    if line.is_empty() {
        return Ok(None);
    }
    if line.len() as u64 > MAX_RESPONSE {
        return Err(Error::ResponseTooLarge);
    }
    Ok(Some(line))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse(raw: &[u8]) -> Result<Vec<u8>, Error> {
        read_response(Cursor::new(raw.to_vec()))
    }

    #[test]
    fn liest_antwort_mit_laengenangabe() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhallo";
        assert_eq!(parse(raw).unwrap(), b"hallo");
    }

    #[test]
    fn grossschreibung_der_kopfzeilen_ist_egal() {
        let raw = b"HTTP/1.1 200 OK\r\nCONTENT-LENGTH: 2\r\n\r\nok";
        assert_eq!(parse(raw).unwrap(), b"ok");
    }

    #[test]
    fn liest_chunked_antwort() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nhal\r\n2;x=1\r\nlo\r\n0\r\n\r\n";
        assert_eq!(parse(raw).unwrap(), b"hallo");
    }

    #[test]
    fn liest_bis_zum_ende_ohne_angabe() {
        assert_eq!(parse(b"HTTP/1.1 200 OK\r\n\r\nhallo").unwrap(), b"hallo");
    }

    #[test]
    fn http_409_heisst_beschaeftigt() {
        assert_eq!(
            parse(b"HTTP/1.1 409 Conflict\r\nContent-Length: 0\r\n\r\n"),
            Err(Error::Busy)
        );
    }

    #[test]
    fn http_400_liefert_den_text_des_dienstes() {
        let body = r#"{"error":{"code":"config","message":"json: cannot unmarshal string"}}"#;
        let raw = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        assert_eq!(
            parse(raw.as_bytes()),
            Err(Error::Service("json: cannot unmarshal string".to_owned()))
        );
        assert_eq!(
            parse(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n"),
            Err(Error::Status(400))
        );
    }

    #[test]
    fn andere_fehler_behalten_ihren_code() {
        assert_eq!(parse(b"HTTP/1.1 502 Bad Gateway\r\n\r\n"), Err(Error::Status(502)));
    }

    #[test]
    fn unlesbare_antworten_sind_fehler() {
        assert_eq!(parse(b"kein http\r\n\r\n"), Err(Error::InvalidResponse));
        assert_eq!(
            parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nZZ\r\n"),
            Err(Error::InvalidResponse)
        );
    }

    #[test]
    fn abgeschnittene_antworten_sind_fehler() {
        assert_eq!(parse(b""), Err(Error::ConnectionLost));
        assert_eq!(
            parse(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nkurz"),
            Err(Error::ConnectionLost)
        );
    }

    #[test]
    fn genau_ein_mib_ist_erlaubt() {
        let mut raw = format!("HTTP/1.1 200 OK\r\nContent-Length: {MAX_RESPONSE}\r\n\r\n").into_bytes();
        raw.extend(std::iter::repeat_n(b'a', usize::try_from(MAX_RESPONSE).unwrap()));
        assert_eq!(parse(&raw).unwrap().len() as u64, MAX_RESPONSE);
    }

    #[test]
    fn mehr_als_ein_mib_wird_abgelehnt() {
        let size = MAX_RESPONSE + 1;
        let mut raw = format!("HTTP/1.1 200 OK\r\nContent-Length: {size}\r\n\r\n").into_bytes();
        raw.extend(std::iter::repeat_n(b'a', usize::try_from(size).unwrap()));
        assert_eq!(parse(&raw), Err(Error::ResponseTooLarge));
    }

    #[test]
    fn zu_lange_zeilen_werden_abgelehnt() {
        let line = vec![b'a'; usize::try_from(MAX_RESPONSE).unwrap() + 2];
        assert_eq!(read_limited_line(&mut Cursor::new(line)), Err(Error::ResponseTooLarge));
    }

    /// A few trailer lines after the last chunk are fine, an endless series is an error.
    #[test]
    fn abschlusszeilen_sind_begrenzt() {
        let head = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nok\r\n0\r\n";
        let few = format!("{head}{}\r\n", "X-Trailer: 1\r\n".repeat(3));
        assert_eq!(parse(few.as_bytes()).unwrap(), b"ok");
        let many = format!("{head}{}\r\n", "X-Trailer: 1\r\n".repeat(1000));
        assert_eq!(parse(many.as_bytes()), Err(Error::InvalidResponse));
    }

    #[test]
    fn fehlerarten_werden_zugeordnet() {
        assert_eq!(map_io(&io::ErrorKind::NotFound.into()), Error::NotRunning);
        assert_eq!(map_io(&io::ErrorKind::ConnectionRefused.into()), Error::NotRunning);
        assert_eq!(map_io(&io::ErrorKind::PermissionDenied.into()), Error::PermissionDenied);
        assert_eq!(map_io(&io::ErrorKind::WouldBlock.into()), Error::Timeout);
        assert_eq!(map_io(&io::ErrorKind::UnexpectedEof.into()), Error::ConnectionLost);
    }
}
