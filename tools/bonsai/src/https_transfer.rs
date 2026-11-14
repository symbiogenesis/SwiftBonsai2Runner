//! Owned, bounded HTTPS with native TLS and cancellation of blocked sockets.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Startup and update modules share an explicit crate-local transport API."
)]

use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs::File,
    io::{Read as _, Seek as _, SeekFrom, Write as _},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use ureq::{
    Agent, Body,
    http::{self, Uri},
    unversioned::{
        resolver::DefaultResolver,
        transport::{ConnectionDetails, Connector, DefaultConnector},
    },
};

thread_local! {
    // Separate preparation workers must never cancel each other's connections.
    static AGENT: RefCell<Option<Agent>> = const { RefCell::new(None) };
}

pub(crate) struct Response {
    pub(crate) status: u16,
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) body: Vec<u8>,
}

pub(crate) fn validate_uri(uri: &str) -> crate::Result<()> {
    validate_target(uri, false)
}

fn validate_target(uri: &str, loopback: bool) -> crate::Result<()> {
    let parsed: Uri = uri.parse().map_err(|_| "Invalid HTTPS artifact URL.")?;
    let authority = parsed.authority().ok_or("Missing HTTPS authority.")?;
    let scheme_valid = parsed.scheme_str() == Some("https")
        || (loopback && parsed.scheme_str() == Some("http") && parsed.host() == Some("127.0.0.1"));
    if !scheme_valid
        || parsed.host().is_none_or(str::is_empty)
        || authority.as_str().contains(['@', '%', '\\'])
        || uri.contains('#')
        || uri.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err("A credential-free, unambiguous HTTPS artifact URL is required.".into());
    }
    Ok(())
}

// This small adapter is the sole use of ureq's unversioned connector API. The
// exact dependency pin and redirect regression protect this upgrade boundary.
// Pool keys include the complete authority: a credentialed redirect cannot
// bypass validation by borrowing a credential-free connection.
#[derive(Debug)]
struct CredentialFree {
    proxy: Option<Uri>,
    loopback: bool,
}

impl Connector for CredentialFree {
    type Out = ();

    fn connect(
        &self,
        details: &ConnectionDetails<'_>,
        chained: Option<()>,
    ) -> Result<Option<()>, ureq::Error> {
        // CONNECT recursively invokes the chain for the explicitly configured
        // proxy. Its credentials/scheme belong to proxy policy, not artifact URLs.
        if !(self.proxy.as_ref() == Some(details.uri) && details.config.proxy().is_none()) {
            validate_target(&details.uri.to_string(), self.loopback)
                .map_err(ureq::Error::BadUri)?;
        }
        Ok(chained)
    }
}

fn new_agent(loopback: bool) -> Agent {
    let config = Agent::config_builder()
        .https_only(!loopback)
        .http_status_as_error(false)
        .max_response_header_size(64 * 1024)
        .timeout_resolve(Some(Duration::from_secs(5)))
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_per_read(Some(Duration::from_secs(60)))
        .timeout_per_write(Some(Duration::from_secs(60)))
        .user_agent(format!("SwiftBonsai2Runner/{}", env!("CARGO_PKG_VERSION")));
    #[cfg(windows)]
    let config = config.tls_config(
        ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::NativeTls)
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build(),
    );
    let config = if loopback { config.proxy(None) } else { config }.build();
    let guard = CredentialFree {
        proxy: config.proxy().map(|p| p.uri().clone()),
        loopback,
    };
    Agent::with_parts(
        config,
        guard.chain(DefaultConnector::default()),
        DefaultResolver::default(),
    )
}

struct RequestGuard {
    agent: Agent,
    abort: bool,
}

struct UnwindShutdown<'a> {
    agent: Agent,
    cancelled: &'a AtomicBool,
}

impl Drop for UnwindShutdown<'_> {
    fn drop(&mut self) {
        if thread::panicking() {
            self.cancelled.store(true, Ordering::Release);
            self.agent.clone().shutdown();
        }
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        if self.abort {
            self.agent.clone().shutdown();
            AGENT.with(|cached| *cached.borrow_mut() = None);
        }
    }
}

fn remaining(until: Instant) -> crate::Result<Duration> {
    until
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| "Native HTTPS operation exceeded its aggregate allowance.".into())
}

fn active(until: Instant, cancelled: &AtomicBool) -> crate::Result<()> {
    if cancelled.load(Ordering::Acquire) {
        return Err("Native HTTPS operation cancelled.".into());
    }
    remaining(until).map(|_| ())
}

fn owned<T: Send>(
    agent: Agent,
    until: Instant,
    checkpoint: &mut dyn FnMut() -> crate::Result<()>,
    operation: impl FnOnce(&Agent, &AtomicBool) -> crate::Result<T> + Send,
) -> crate::Result<T> {
    checkpoint()?;
    remaining(until)?;
    let mut guard = RequestGuard { agent, abort: true };
    let cancelled = AtomicBool::new(false);
    let result = thread::scope(|scope| {
        // Drop during owner unwinding BEFORE scope joins the worker. An outer
        // guard alone would close sockets only after the implicit scope join.
        let _unwind = UnwindShutdown {
            agent: guard.agent.clone(),
            cancelled: &cancelled,
        };
        let (send, receive) = mpsc::sync_channel(1);
        let worker_agent = guard.agent.clone();
        let stopped = &cancelled;
        let worker = thread::Builder::new()
            .name("bonsai-https".into())
            .spawn_scoped(scope, move || {
                let result = operation(&worker_agent, stopped);
                let _sent = send.send(());
                result
            })
            .map_err(|e| format!("Cannot start HTTPS worker: {e}"))?;
        let observed = loop {
            if let Err(error) = checkpoint().and_then(|()| remaining(until).map(|_| ())) {
                cancelled.store(true, Ordering::Release);
                guard.agent.clone().shutdown();
                break Err(error);
            }
            match receive.recv_timeout(Duration::from_millis(50)) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break Ok(()),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        };
        let result = worker.join().map_err(|_| "Native HTTPS worker failed.")?;
        observed?;
        checkpoint()?;
        remaining(until)?;
        result
    });
    guard.abort = result.is_err();
    result
}

fn agent() -> Agent {
    AGENT.with(|cached| {
        cached
            .borrow_mut()
            .get_or_insert_with(|| new_agent(false))
            .clone()
    })
}

fn get(
    agent: &Agent,
    uri: &str,
    headers: &[(String, String)],
    until: Instant,
    redirects: u32,
) -> crate::Result<http::Response<Body>> {
    let mut request = agent
        .get(uri)
        .config()
        .timeout_global(Some(remaining(until)?))
        .max_redirects(redirects)
        .build();
    for (name, value) in headers {
        request = request.header(name, value);
    }
    request
        .call()
        .map_err(|e| format!("Native HTTPS request failed: {e}"))
}

fn response_headers(response: &http::Response<Body>) -> crate::Result<BTreeMap<String, String>> {
    let mut headers = BTreeMap::new();
    for (name, value) in response.headers() {
        if !matches!(
            name.as_str(),
            "content-length"
                | "content-range"
                | "content-encoding"
                | "etag"
                | "last-modified"
                | "retry-after"
                | "x-ratelimit-remaining"
                | "x-ratelimit-reset"
        ) {
            continue;
        }
        let value = value
            .to_str()
            .map_err(|_| "Invalid HTTP response header.")?;
        // Ambiguous integrity/framing headers must never be silently overwritten.
        if headers.insert(name.as_str().into(), value.into()).is_some() {
            return Err("Duplicate HTTP response header.".into());
        }
    }
    Ok(headers)
}

fn content_length(headers: &BTreeMap<String, String>) -> crate::Result<Option<u64>> {
    headers
        .get("content-length")
        .map(|v| v.parse().map_err(|_| "Invalid HTTP Content-Length.".into()))
        .transpose()
}

fn metadata_with(
    agent: &Agent,
    uri: &str,
    headers: &[(String, String)],
    until: Instant,
    maximum: u64,
    cancelled: &AtomicBool,
) -> crate::Result<Response> {
    let mut response = get(agent, uri, headers, until, 3)?;
    active(until, cancelled)?;
    let status = response.status().as_u16();
    let headers = response_headers(&response)?;
    let mut body = Vec::new();
    // Preserve Retry-After and rate-limit headers without consuming error bodies.
    if status == 200 {
        if content_length(&headers)?.is_some_and(|n| n > maximum) {
            return Err("Update metadata exceeds its byte limit.".into());
        }
        let mut reader = response.body_mut().as_reader();
        let mut buffer = [0; 16 * 1024];
        loop {
            active(until, cancelled)?;
            let count = reader.read(&mut buffer).map_err(|e| e.to_string())?;
            active(until, cancelled)?;
            if count == 0 {
                break;
            }
            if u64::try_from(body.len())
                .map_err(|e| e.to_string())?
                .saturating_add(u64::try_from(count).map_err(|e| e.to_string())?)
                > maximum
            {
                return Err("Update metadata exceeds its byte limit.".into());
            }
            body.extend_from_slice(&buffer[..count]);
        }
    }
    Ok(Response {
        status,
        headers,
        body,
    })
}

pub(crate) fn metadata(
    uri: &str,
    headers: &[(String, String)],
    timeout: Duration,
    maximum: u64,
) -> crate::Result<Response> {
    validate_uri(uri)?;
    let until = Instant::now()
        .checked_add(timeout)
        .ok_or("Invalid HTTPS allowance.")?;
    owned(
        agent(),
        until,
        &mut crate::startup_cancel::checkpoint,
        |agent, cancelled| metadata_with(agent, uri, headers, until, maximum, cancelled),
    )
}

fn response_offset(
    status: u16,
    headers: &BTreeMap<String, String>,
    offset: u64,
    bytes: u64,
) -> crate::Result<u64> {
    if headers
        .get("content-encoding")
        .is_some_and(|v| !v.eq_ignore_ascii_case("identity"))
    {
        return Err("Pinned artifacts require identity content encoding.".into());
    }
    let start = match status {
        200 if !headers.contains_key("content-range") => 0,
        206 => {
            let expected = format!("bytes {offset}-{}/{bytes}", bytes - 1);
            if headers.get("content-range") != Some(&expected) {
                return Err("Pinned download Content-Range differs from its requested offset and locked length.".into());
            }
            offset
        }
        _ => return Err(format!("Pinned download returned HTTP {status}.")),
    };
    if content_length(headers)?.is_some_and(|n| n != bytes - start) {
        return Err("Pinned download Content-Length differs from its locked length.".into());
    }
    Ok(start)
}

fn download_with(
    agent: &Agent,
    uri: &str,
    file: &mut File,
    offset: u64,
    bytes: u64,
    until: Instant,
    cancelled: &AtomicBool,
) -> crate::Result<()> {
    if bytes == 0 || offset >= bytes || file.metadata().map_err(|e| e.to_string())?.len() != offset
    {
        return Err("Invalid pinned download length or changed partial offset.".into());
    }
    let mut headers = vec![("Accept-Encoding".into(), "identity".into())];
    if offset != 0 {
        headers.push(("Range".into(), format!("bytes={offset}-")));
    }
    let mut response = get(agent, uri, &headers, until, 10)?;
    active(until, cancelled)?;
    let headers = response_headers(&response)?;
    let start = response_offset(response.status().as_u16(), &headers, offset, bytes)?;
    // Validate status, range, encoding and length before altering the partial.
    if start != offset {
        file.set_len(0).map_err(|e| e.to_string())?;
    }
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut written = start;
    let mut buffer = vec![0; 64 * 1024];
    let mut reader = response.body_mut().as_reader();
    loop {
        active(until, cancelled)?;
        let count = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        active(until, cancelled)?;
        if count == 0 {
            break;
        }
        let end = written
            .checked_add(u64::try_from(count).map_err(|e| e.to_string())?)
            .ok_or("Download length overflow.")?;
        if end > bytes {
            return Err("Pinned download exceeded its locked byte length.".into());
        }
        file.write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
        written = end;
    }
    if written != bytes {
        return Err("Pinned download ended before its locked byte length.".into());
    }
    file.sync_all().map_err(|e| e.to_string())?;
    active(until, cancelled)
}

pub(crate) fn download(
    uri: &str,
    file: &mut File,
    offset: u64,
    bytes: u64,
    until: Instant,
    checkpoint: &mut dyn FnMut() -> crate::Result<()>,
) -> crate::Result<()> {
    validate_uri(uri)?;
    owned(agent(), until, checkpoint, |agent, cancelled| {
        download_with(agent, uri, file, offset, bytes, until, cancelled)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{self, OpenOptions},
        net::{TcpListener, TcpStream},
        path::PathBuf,
        sync::Arc,
    };

    struct Partial {
        path: PathBuf,
        file: File,
    }

    impl Partial {
        fn new(bytes: &[u8]) -> crate::Result<Self> {
            let path = std::env::temp_dir().join(format!(
                "bonsai-native-https-{}",
                crate::startup_process::unique()
            ));
            let mut file = OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .open(&path)
                .map_err(|e| e.to_string())?;
            file.write_all(bytes).map_err(|e| e.to_string())?;
            Ok(Self { path, file })
        }

        fn bytes(&mut self) -> crate::Result<Vec<u8>> {
            self.file
                .seek(SeekFrom::Start(0))
                .map_err(|e| e.to_string())?;
            let mut bytes = Vec::new();
            self.file
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            Ok(bytes)
        }
    }

    impl Drop for Partial {
        fn drop(&mut self) {
            let _removed = fs::remove_file(&self.path);
        }
    }

    fn request(stream: &mut TcpStream) -> crate::Result<String> {
        stream.set_nonblocking(false).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
            let mut byte = [0];
            if stream.read(&mut byte).map_err(|e| e.to_string())? == 0 {
                return Err("Fixture request truncated.".into());
            }
            request.push(byte[0]);
        }
        if !request.ends_with(b"\r\n\r\n") {
            return Err("Fixture request exceeded its bound.".into());
        }
        String::from_utf8(request).map_err(|e| e.to_string())
    }

    fn fixture<T>(
        response: &[u8],
        operation: impl FnOnce(Agent, &str) -> crate::Result<T>,
    ) -> crate::Result<(T, String)> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let uri = format!(
            "http://{}/artifact",
            listener.local_addr().map_err(|e| e.to_string())?
        );
        thread::scope(|scope| {
            let server = scope.spawn(move || {
                let until = Instant::now() + Duration::from_secs(5);
                loop {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let request = request(&mut stream)?;
                            stream.write_all(response).map_err(|e| e.to_string())?;
                            return Ok::<_, String>(request);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            remaining(until)?;
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(e) => return Err(e.to_string()),
                    }
                }
            });
            let result = operation(new_agent(true), &uri);
            let request = server.join().map_err(|_| "HTTPS fixture panicked.")??;
            Ok((result?, request))
        })
    }

    #[test]
    fn native_https_rejects_ambiguous_urls_and_mismatched_ranges() {
        for uri in [
            "http://example.com/a",
            "https://user:pass@example.com/a",
            "https://example.com/a b",
            "https://ex%61mple.com/a",
            "https://",
            "https://example.com/a#b",
        ] {
            assert!(validate_uri(uri).is_err());
        }
        assert!(validate_uri("https://example.com/a?token=123").is_ok());
        let mut headers = BTreeMap::from([
            ("content-range".into(), "bytes 5-9/10".into()),
            ("content-length".into(), "5".into()),
        ]);
        assert_eq!(response_offset(206, &headers, 5, 10), Ok(5));
        for bad in [
            "bytes 0-4/10",
            "bytes 5-9/*",
            "bytes 5-9/11",
            "bytes 5-8/10",
        ] {
            headers.insert("content-range".into(), bad.into());
            assert!(response_offset(206, &headers, 5, 10).is_err());
        }
        headers.remove("content-range");
        assert!(response_offset(200, &headers, 5, 10).is_err());
        headers.insert("content-length".into(), "10".into());
        assert_eq!(response_offset(200, &headers, 5, 10), Ok(0));
        headers.insert("content-encoding".into(), "gzip".into());
        assert!(response_offset(200, &headers, 5, 10).is_err());
    }

    #[test]
    fn native_https_loopback_resumes_restarts_and_retains_bounded_partials() -> crate::Result<()> {
        for (response, expected, success) in [
            (b"HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 5-9/10\r\nConnection: close\r\n\r\nworld".as_slice(), b"helloworld".as_slice(), true),
            (b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\nConnection: close\r\n\r\n0123456789", b"0123456789", true),
            (b"HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 4-8/10\r\nConnection: close\r\n\r\nworld", b"hello", false),
            (b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\nB\r\n01234567890\r\n0\r\n\r\n", b"", false),
            (b"HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 5-9/10\r\nConnection: close\r\n\r\nwo", b"hellowo", false),
        ] {
            let mut partial = Partial::new(b"hello")?;
            let (result, request) = fixture(response, |agent, uri| {
                Ok(download_with(&agent, uri, &mut partial.file, 5, 10, Instant::now() + Duration::from_secs(5), &AtomicBool::new(false)))
            })?;
            assert_eq!(result.is_ok(), success, "{result:?}");
            assert!(request.to_ascii_lowercase().contains("range: bytes=5-"));
            assert_eq!(partial.bytes()?, expected);
        }
        Ok(())
    }

    #[test]
    fn native_https_metadata_bounds_status_and_redirect_credentials() -> crate::Result<()> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(b"metadata").map_err(|e| e.to_string())?;
        let compressed = encoder.finish().map_err(|e| e.to_string())?;
        let mut raw = format!("HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", compressed.len()).into_bytes();
        raw.extend_from_slice(&compressed);
        let (response, _) = fixture(&raw, |agent, uri| {
            metadata_with(
                &agent,
                uri,
                &[],
                Instant::now() + Duration::from_secs(5),
                1024,
                &AtomicBool::new(false),
            )
        })?;
        assert_eq!(response.body, compressed);
        assert_eq!(
            response.headers.get("content-encoding").map(String::as_str),
            Some("gzip")
        );
        let (response, _) = fixture(b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 90\r\nContent-Length: 999999\r\nConnection: close\r\n\r\n", |agent, uri| {
            metadata_with(&agent, uri, &[], Instant::now() + Duration::from_secs(5), 4, &AtomicBool::new(false))
        })?;
        assert_eq!(response.status, 429);
        assert_eq!(
            response.headers.get("retry-after").map(String::as_str),
            Some("90")
        );
        assert_eq!(response.body, Vec::<u8>::new());
        let (response, _) = fixture(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n0\r\n\r\n", |agent, uri| {
            Ok(metadata_with(&agent, uri, &[], Instant::now() + Duration::from_secs(5), 4, &AtomicBool::new(false)))
        })?;
        assert!(response.is_err());
        let (response, _) = fixture(b"HTTP/1.1 302 Found\r\nLocation: http://user:secret@127.0.0.1:9/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", |agent, uri| {
            Ok(metadata_with(&agent, uri, &[], Instant::now() + Duration::from_secs(5), 4, &AtomicBool::new(false)))
        })?;
        assert!(
            response
                .err()
                .is_some_and(|e| e.contains("credential-free"))
        );
        Ok(())
    }

    #[test]
    fn native_https_rejects_duplicate_integrity_headers_before_restarting_partial()
    -> crate::Result<()> {
        for header in [
            "Content-Range: bytes 5-9/10",
            "Content-Encoding: identity",
            "ETag: \"version\"",
            "Last-Modified: Sat, 14 Nov 2026 00:00:00 GMT",
        ] {
            // Identical repetitions are still ambiguous. A 200 response would
            // otherwise discard the existing prefix before downloading afresh.
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: 10\r\n{header}\r\n{header}\r\nConnection: close\r\n\r\n0123456789"
            );
            let mut partial = Partial::new(b"hello")?;
            let (result, _) = fixture(response.as_bytes(), |agent, uri| {
                Ok(download_with(
                    &agent,
                    uri,
                    &mut partial.file,
                    5,
                    10,
                    Instant::now() + Duration::from_secs(5),
                    &AtomicBool::new(false),
                ))
            })?;
            assert_eq!(result, Err("Duplicate HTTP response header.".into()));
            assert_eq!(partial.bytes()?, b"hello");
        }
        Ok(())
    }

    #[test]
    fn native_https_metadata_checks_declared_length_before_reading_body() -> crate::Result<()> {
        let (result, _) = fixture(
            b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n",
            |agent, uri| {
                Ok(metadata_with(
                    &agent,
                    uri,
                    &[],
                    Instant::now() + Duration::from_secs(5),
                    4,
                    &AtomicBool::new(false),
                ))
            },
        )?;
        // The response deliberately contains no body: an attempted body read
        // would produce truncation instead of the declared-length rejection.
        assert!(matches!(result, Err(error) if error == "Update metadata exceeds its byte limit."));
        let (response, _) = fixture(
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nokay",
            |agent, uri| {
                metadata_with(
                    &agent,
                    uri,
                    &[],
                    Instant::now() + Duration::from_secs(5),
                    4,
                    &AtomicBool::new(false),
                )
            },
        )?;
        assert_eq!(response.body, b"okay");
        Ok(())
    }

    #[test]
    fn native_https_expired_allowance_never_starts_worker() {
        let started = AtomicBool::new(false);
        let result = owned(new_agent(true), Instant::now(), &mut || Ok(()), |_, _| {
            started.store(true, Ordering::Release);
            Ok(())
        });
        assert_eq!(
            result,
            Err("Native HTTPS operation exceeded its aggregate allowance.".into())
        );
        assert!(!started.load(Ordering::Acquire));
    }

    #[derive(Clone, Copy)]
    enum Stop {
        Cancel,
        Panic,
        Deadline,
    }

    #[test]
    fn native_https_cancellation_shuts_down_stalled_body_and_joins_worker() -> crate::Result<()> {
        for stop in [Stop::Cancel, Stop::Panic] {
            stalled_body(stop)?;
        }
        Ok(())
    }

    #[test]
    fn native_https_deadline_shuts_down_stalled_body_and_joins_worker() -> crate::Result<()> {
        stalled_body(Stop::Deadline)
    }

    fn cached_metadata(
        uri: &str,
        stop: &AtomicBool,
        timeout: Duration,
    ) -> (crate::Result<Response>, bool) {
        // Use the production per-owner cache with loopback-only fixture transport.
        AGENT.with(|cached| *cached.borrow_mut() = Some(new_agent(true)));
        let until = Instant::now() + timeout;
        let response = owned(
            agent(),
            until,
            &mut || {
                if stop.load(Ordering::Acquire) {
                    Err("Synthetic owner cancellation.".into())
                } else {
                    Ok(())
                }
            },
            |agent, cancelled| metadata_with(agent, uri, &[], until, 5, cancelled),
        );
        (response, AGENT.with(|cached| cached.borrow().is_some()))
    }

    fn pending_metadata(listener: &TcpListener) -> crate::Result<TcpStream> {
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let until = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    remaining(until)?;
                    thread::sleep(Duration::from_millis(5));
                }
                Err(e) => return Err(e.to_string()),
            }
        };
        request(&mut stream)?;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n")
            .map_err(|e| e.to_string())?;
        Ok(stream)
    }

    #[test]
    fn native_https_cancelled_cached_owner_preserves_peer_connection() -> crate::Result<()> {
        let cancelled_listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let peer_listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let cancelled_uri = format!(
            "http://{}/cancelled",
            cancelled_listener.local_addr().map_err(|e| e.to_string())?
        );
        let peer_uri = format!(
            "http://{}/peer",
            peer_listener.local_addr().map_err(|e| e.to_string())?
        );
        let stop = AtomicBool::new(false);
        let peer_stop = AtomicBool::new(false);
        thread::scope(|scope| {
            let cancelled_owner =
                scope.spawn(|| cached_metadata(&cancelled_uri, &stop, Duration::from_secs(1)));
            let peer_owner =
                scope.spawn(|| cached_metadata(&peer_uri, &peer_stop, Duration::from_secs(5)));
            let mut cancelled_socket = pending_metadata(&cancelled_listener)
                .map_err(|e| format!("Cancelled owner fixture: {e}"))?;
            let mut peer_socket =
                pending_metadata(&peer_listener).map_err(|e| format!("Peer owner fixture: {e}"))?;
            // Both owners now have a live response body. Keep the peer blocked
            // until cancellation has shut down and joined the other owner.
            stop.store(true, Ordering::Release);
            let (cancelled, retained) = cancelled_owner
                .join()
                .map_err(|_| "Cancelled HTTPS owner panicked.")?;
            assert!(matches!(cancelled, Err(error) if error == "Synthetic owner cancellation."));
            assert!(!retained, "An aborted owner retained its shut-down agent.");
            let mut byte = [0];
            match cancelled_socket.read(&mut byte) {
                Ok(0) => {}
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {}
                result => return Err(format!("Cancelled owner retained its socket: {result:?}")),
            }
            peer_socket.write_all(b"alive").map_err(|e| e.to_string())?;
            let (peer, retained) = peer_owner
                .join()
                .map_err(|_| "Peer HTTPS owner panicked.")?;
            assert_eq!(peer?.body, b"alive");
            assert!(retained, "Successful peer lost its reusable agent.");
            Ok(())
        })
    }

    fn stalled_body(stop: Stop) -> crate::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let uri = format!(
            "http://{}/artifact",
            listener.local_addr().map_err(|e| e.to_string())?
        );
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let headers_sent = Arc::new(AtomicBool::new(false));
        thread::scope(|scope| {
            let started = Arc::clone(&headers_sent);
            let server = scope.spawn(move || {
                let until = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            remaining(until)?;
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(e) => return Err(e.to_string()),
                    }
                };
                request(&mut stream)?;
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n")
                    .map_err(|e| e.to_string())?;
                started.store(true, Ordering::Release);
                let mut byte = [0];
                match stream.read(&mut byte) {
                    Ok(0) => Ok(()),
                    Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => Ok(()),
                    _ => Err("Cancelled native HTTPS worker did not release its socket.".into()),
                }
            });
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                owned(
                    new_agent(true),
                    Instant::now()
                        + if matches!(stop, Stop::Deadline) {
                            Duration::from_secs(1)
                        } else {
                            Duration::from_secs(5)
                        },
                    &mut || {
                        if headers_sent.load(Ordering::Acquire) && !matches!(stop, Stop::Deadline) {
                            if matches!(stop, Stop::Panic) {
                                std::panic::resume_unwind(Box::new("Synthetic caller panic."));
                            }
                            Err("Synthetic caller cancellation.".into())
                        } else {
                            Ok(())
                        }
                    },
                    |agent, cancelled| {
                        metadata_with(
                            agent,
                            &uri,
                            &[],
                            Instant::now() + Duration::from_secs(5),
                            5,
                            cancelled,
                        )
                    },
                )
            }));
            let released = server
                .join()
                .map_err(|_| "Stalled HTTPS fixture panicked.")?;
            assert!(headers_sent.load(Ordering::Acquire));
            if matches!(stop, Stop::Panic) {
                assert!(result.is_err());
            } else {
                let expected = if matches!(stop, Stop::Deadline) {
                    "Native HTTPS operation exceeded its aggregate allowance."
                } else {
                    "Synthetic caller cancellation."
                };
                assert!(
                    result
                        .map_err(|_| "Unexpected owner panic.")?
                        .err()
                        .is_some_and(|e| e == expected)
                );
            }
            released
        })
    }
}
