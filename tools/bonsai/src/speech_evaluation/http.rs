//! Fixed loopback HTTP/1.1 subset: no DNS, proxies, cookies or redirects.

use std::io::{self, BufRead as _, BufReader, Read, Write as _};
use std::net::{Ipv4Addr, TcpStream};
use std::time::{Duration, Instant};

const MAX_HEAD: usize = 16_384;
const INVALID: &str = "Invalid or incomplete bounded evaluation HTTP response.";

enum Framing {
    Length(usize),
    Chunked,
}

fn headers(head: &[u8], media: &str, maximum: usize) -> crate::Result<Framing> {
    if !head.starts_with(b"HTTP/1.1 ")
        || head.iter().enumerate().any(|(index, &byte)| {
            (byte == b'\n' && (index == 0 || head[index - 1] != b'\r'))
                || (byte == b'\r' && head.get(index + 1) != Some(&b'\n'))
        })
    {
        return Err(INVALID.into());
    }
    let mut storage = [httparse::EMPTY_HEADER; 64];
    let mut response = httparse::Response::new(&mut storage);
    if response.parse(head).ok() != Some(httparse::Status::Complete(head.len()))
        || response.code != Some(200)
        || response.version != Some(1)
    {
        return Err(INVALID.into());
    }
    let mut length = None;
    let mut chunked = false;
    let mut content_type = false;
    let mut close = false;
    for (index, field) in response.headers.iter().enumerate() {
        if response.headers[..index]
            .iter()
            .any(|earlier| earlier.name.eq_ignore_ascii_case(field.name))
            || field.value.iter().any(|&byte| byte < 32 || byte == 127)
            || ["content-encoding", "trailer", "upgrade", "location"]
                .iter()
                .any(|name| field.name.eq_ignore_ascii_case(name))
        {
            return Err(INVALID.into());
        }
        if field.name.eq_ignore_ascii_case("content-type") {
            content_type = field.value == media.as_bytes();
        } else if field.name.eq_ignore_ascii_case("content-length") {
            if field.value.is_empty() || !field.value.iter().all(u8::is_ascii_digit) {
                return Err(INVALID.into());
            }
            length = Some(
                std::str::from_utf8(field.value)
                    .map_err(|_| INVALID)?
                    .parse::<usize>()
                    .map_err(|_| INVALID)?,
            );
        } else if field.name.eq_ignore_ascii_case("transfer-encoding") {
            if !field.value.eq_ignore_ascii_case(b"chunked") {
                return Err(INVALID.into());
            }
            chunked = true;
        } else if field.name.eq_ignore_ascii_case("connection") {
            close = field.value.eq_ignore_ascii_case(b"close");
        }
    }
    if !content_type || !close {
        return Err(INVALID.into());
    }
    match (length, chunked) {
        (Some(length), false) if (1..=maximum).contains(&length) => Ok(Framing::Length(length)),
        (None, true) if media == "audio/pcm" => Ok(Framing::Chunked),
        _ => Err(INVALID.into()),
    }
}

fn line(reader: &mut impl Read, maximum: usize) -> crate::Result<Vec<u8>> {
    let mut line = Vec::new();
    while !line.ends_with(b"\r\n") {
        if line.len() == maximum {
            return Err(INVALID.into());
        }
        let mut byte = [0];
        reader.read_exact(&mut byte).map_err(|_| INVALID)?;
        line.push(byte[0]);
    }
    Ok(line)
}

fn payload(
    reader: &mut impl Read,
    length: usize,
    bytes: &mut Vec<u8>,
    first: &mut Option<Duration>,
    started: Instant,
) -> crate::Result<()> {
    let mut remaining = length;
    let mut buffer = [0; 16_384];
    while remaining != 0 {
        let bound = remaining.min(buffer.len());
        let count = reader.read(&mut buffer[..bound]).map_err(|_| INVALID)?;
        if count == 0 {
            return Err(INVALID.into());
        }
        first.get_or_insert_with(|| started.elapsed());
        bytes.extend_from_slice(&buffer[..count]);
        remaining -= count;
    }
    Ok(())
}

fn decode(
    reader: impl Read,
    media: &str,
    maximum: usize,
    started: Instant,
) -> crate::Result<Response> {
    let mut reader = BufReader::with_capacity(16_384, reader);
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let next = line(&mut reader, MAX_HEAD.saturating_sub(head.len()))?;
        head.extend_from_slice(&next);
    }
    let framing = headers(&head, media, maximum)?;
    let mut bytes = Vec::new();
    let mut first = None;
    match framing {
        Framing::Length(length) => payload(&mut reader, length, &mut bytes, &mut first, started)?,
        Framing::Chunked => loop {
            let chunk_line = line(&mut reader, 10)?;
            let digits = &chunk_line[..chunk_line.len() - 2];
            if digits.is_empty() || !digits.iter().all(u8::is_ascii_hexdigit) {
                return Err(INVALID.into());
            }
            let length =
                usize::from_str_radix(std::str::from_utf8(digits).map_err(|_| INVALID)?, 16)
                    .map_err(|_| INVALID)?;
            if length == 0 {
                if line(&mut reader, 2)? != b"\r\n" {
                    return Err(INVALID.into());
                }
                break;
            }
            if !length.is_multiple_of(2)
                || bytes
                    .len()
                    .checked_add(length)
                    .is_none_or(|total| total > maximum)
            {
                return Err(INVALID.into());
            }
            payload(&mut reader, length, &mut bytes, &mut first, started)?;
            if line(&mut reader, 2)? != b"\r\n" {
                return Err(INVALID.into());
            }
        },
    }
    if !reader.fill_buf().map_err(|_| INVALID)?.is_empty() {
        return Err(INVALID.into());
    }
    Ok(Response {
        bytes,
        first_body: first.ok_or(INVALID)?,
        elapsed: started.elapsed(),
    })
}

pub(super) struct Response {
    pub(super) bytes: Vec<u8>,
    pub(super) first_body: Duration,
    pub(super) elapsed: Duration,
}

struct Wire<'a, F> {
    stream: TcpStream,
    deadline: Instant,
    check: &'a F,
}

impl<F: Fn() -> crate::Result<()>> Wire<'_, F> {
    fn allowance(&self) -> io::Result<Duration> {
        (self.check)().map_err(io::Error::other)?;
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Evaluation HTTP deadline expired.",
            ));
        }
        Ok(remaining.min(Duration::from_millis(100)))
    }
    fn send(&mut self, mut bytes: &[u8]) -> crate::Result<()> {
        while !bytes.is_empty() {
            self.stream
                .set_write_timeout(Some(self.allowance().map_err(|error| error.to_string())?))
                .map_err(|_| "Evaluation socket setup failed.")?;
            match self.stream.write(bytes) {
                Ok(0) => return Err("Evaluation HTTP request was incomplete.".into()),
                Ok(count) => bytes = &bytes[count..],
                Err(error) if retry(&error) => (),
                Err(_) => return Err("Evaluation loopback request failed.".into()),
            }
        }
        Ok(())
    }
}

fn retry(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

impl<F: Fn() -> crate::Result<()>> Read for Wire<'_, F> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            self.stream.set_read_timeout(Some(self.allowance()?))?;
            match self.stream.read(bytes) {
                Err(error) if retry(&error) => (),
                result => return result,
            }
        }
    }
}

pub(super) struct Call<'a> {
    pub(super) port: u16,
    pub(super) token: &'a [u8],
    pub(super) body: Option<&'a [u8]>,
    pub(super) media: &'a str,
    pub(super) maximum: usize,
    pub(super) deadline: Instant,
}

pub(super) fn exchange(
    call: &Call<'_>,
    check: impl Fn() -> crate::Result<()>,
) -> crate::Result<Response> {
    check()?;
    if call.port == 0
        || call.token.len() != 64
        || !call
            .token
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err("Invalid evaluation endpoint or credential framing.".into());
    }
    let started = Instant::now();
    let timeout = call
        .deadline
        .saturating_duration_since(started)
        .min(Duration::from_millis(100));
    if timeout.is_zero() {
        return Err("Evaluation HTTP deadline expired.".into());
    }
    let stream = TcpStream::connect_timeout(&(Ipv4Addr::LOCALHOST, call.port).into(), timeout)
        .map_err(|_| "Evaluation loopback service is unavailable.")?;
    let mut wire = Wire {
        stream,
        deadline: call.deadline,
        check: &check,
    };
    let route = if call.body.is_some() {
        "POST /v1/audio/speech"
    } else {
        "GET /health"
    };
    wire.send(
        format!(
            "{route} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer ",
            call.port
        )
        .as_bytes(),
    )?;
    // Borrow the protected token directly: no secret-bearing command, log or String.
    wire.send(call.token)?;
    let body = call.body.unwrap_or_default();
    wire.send(
        format!(
            "\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .as_bytes(),
    )?;
    wire.send(body)?;
    decode(wire, call.media, call.maximum, started)
}

#[cfg(test)]
mod tests;
