//! Narrow HTTP exchange over an owned, already connected private backend stream.
//!
//! No socket, process, task, retry, redirect or connection reuse is provided.
//! The supervisor must supply streams to the same immutable, owned backend for
//! counting and generation, prevent bypass, and ensure Drop closes each stream.

use crate::{
    AdmittedChat, MAX_HEADER_BYTES, MAX_HEADERS, Object, PendingChat, Rejection, configuration,
    deadline::RequestDeadline,
    response::{MAX_RESPONSE_BYTES, valid_content_type},
    wire,
};
use http::{Response, StatusCode, Version};
use serde::{Deserialize, Deserializer};
use std::{
    borrow::Cow,
    io,
    pin::Pin,
    task::{Context, Poll, ready},
    time::Duration,
};
use tokio::{
    io::{
        AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt,
        BufReader, ReadBuf,
    },
    time::{Instant, Sleep},
};

/// Maximum time allowed for writing one backend request.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// Largest accepted token-counter response body in bytes.
pub const MAX_COUNT_BYTES: usize = 4096;
/// Largest accepted chunk-size line in bytes, including its CRLF.
pub const MAX_CHUNK_LINE_BYTES: usize = 32;
/// Largest accumulated chunk framing metadata in bytes for one response.
pub const MAX_CHUNK_METADATA_BYTES: usize = 64 * 1024;
const COUNT_ROUTE: &str = "/v1/chat/completions/input_tokens";

/// Contains no private header, body, credential or native I/O diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendError {
    /// The private backend port or credential format is invalid.
    Configuration,
    /// The absolute exchange or write deadline has expired.
    Deadline,
    /// The supplied stream failed while exchanging HTTP headers or request data.
    Io,
    /// The backend response exceeded the header count or byte limit.
    HeadLimit,
    /// The backend response has invalid HTTP/1.1 header syntax.
    WireSyntax,
    /// The response status, content type or encoding is unsupported.
    Metadata,
    /// The response uses unsupported or ambiguous HTTP body framing.
    Framing,
    /// The body is oversized, truncated or fails while being read.
    Body,
    /// The token-counter response does not match the bounded strict schema.
    CountSchema,
    /// The counted prompt or a recognized private counter context rejection failed admission.
    Admission(Rejection),
    /// A verified adaptive runtime rejected against this observed memory-backed limit.
    ContextLimit(usize),
}

impl BackendError {
    pub(crate) const fn is_context_limit(self) -> bool {
        matches!(
            self,
            Self::Admission(Rejection::ContextLimit) | Self::ContextLimit(_)
        )
    }
}

/// Private backend connection metadata. No Debug/Serialize implementation.
/// The caller must use a separate random backend key and enforce its protection.
pub struct BackendPolicy {
    authority: String,
    authorization: String,
    adaptive_context: bool,
}

impl BackendPolicy {
    /// Configure the private backend endpoint and its separate credential.
    ///
    /// # Errors
    /// Returns [`BackendError::Configuration`] for a privileged port or invalid key.
    pub fn new(port: u16, key: &str) -> Result<Self, BackendError> {
        if port < 1024 || !configuration::valid_key(key.as_bytes()) {
            return Err(BackendError::Configuration);
        }
        Ok(Self {
            authority: format!("127.0.0.1:{port}"),
            authorization: format!("Bearer {key}"),
            adaptive_context: false,
        })
    }

    /// Enable the exact available-memory-v1 contract after immutable runtime
    /// identity and readiness verification. Client requests cannot select it.
    #[must_use]
    pub const fn with_adaptive_context(mut self) -> Self {
        self.adaptive_context = true;
        self
    }

    pub(crate) const fn adaptive_context(&self) -> bool {
        self.adaptive_context
    }

    #[cfg(windows)]
    pub(crate) const fn enable_adaptive_context(&mut self) {
        self.adaptive_context = true;
    }
}

/// Count the exact canonical body, then apply the existing context admission.
///
/// Ownership closes the supplied connection on failure, completion or cancellation.
/// Only the strict framed private counter context error is recoverable; an HTTP
/// status alone, an unknown error code or an untyped failure cannot classify it.
///
/// # Errors
/// Returns [`BackendError`] for an expired deadline, stream failure, invalid
/// HTTP response, token-count schema mismatch or rejected context admission.
pub async fn count_and_admit<S>(
    stream: S,
    pending: PendingChat,
    policy: &BackendPolicy,
    deadline: impl Into<RequestDeadline>,
) -> Result<AdmittedChat, BackendError>
where
    S: AsyncRead + AsyncWrite + Unpin + 'static,
{
    let deadline = deadline.into();
    let (status, body) =
        exchange(stream, &pending.body, false, COUNT_ROUTE, policy, deadline).await?;
    let bytes = bounded_count_body(body, deadline).await?;
    if status == StatusCode::PAYLOAD_TOO_LARGE {
        return Err(context_rejection(&bytes, policy)?);
    }
    let Object(count): Object<Count<'_>> =
        serde_json::from_slice(&bytes).map_err(|_| BackendError::CountSchema)?;
    ensure_time(deadline)?;
    if count.object != "response.input_tokens" {
        return Err(BackendError::CountSchema);
    }
    let context_tokens = match (policy.adaptive_context, count.context_tokens) {
        (false, None) => crate::CONTEXT_TOKENS,
        (true, Some(limit)) if (1..=crate::MAX_CONTEXT_TOKENS).contains(&limit) => limit,
        _ => return Err(BackendError::CountSchema),
    };
    // PendingChat checks positivity and retains the canonical allocation unchanged.
    let admitted = pending
        .admit_with_context_limit(context_tokens, |_| Ok(count.input_tokens))
        .map_err(|error| {
            if policy.adaptive_context && error == Rejection::ContextLimit {
                BackendError::ContextLimit(context_tokens)
            } else {
                BackendError::Admission(error)
            }
        })?;
    ensure_time(deadline)?;
    Ok(admitted)
}

async fn bounded_count_body<S>(
    body: BackendBody<S>,
    deadline: RequestDeadline,
) -> Result<Vec<u8>, BackendError>
where
    S: AsyncRead + AsyncWrite + Unpin + 'static,
{
    // Read directly into the final allocation, keeping one byte for the
    // oversized-body probe without allowing unbounded read_to_end growth.
    let limit = u64::try_from(MAX_COUNT_BYTES + 1).map_err(|_| BackendError::CountSchema)?;
    let mut body = body.take(limit);
    let mut bytes = Vec::with_capacity(MAX_COUNT_BYTES + 1);
    body.read_to_end(&mut bytes).await.map_err(|error| {
        if error.kind() == io::ErrorKind::TimedOut {
            BackendError::Deadline
        } else {
            BackendError::Body
        }
    })?;
    if bytes.len() > MAX_COUNT_BYTES {
        return Err(BackendError::CountSchema);
    }
    ensure_time(deadline)?;
    Ok(bytes)
}

fn context_rejection(bytes: &[u8], policy: &BackendPolicy) -> Result<BackendError, BackendError> {
    let Object(rejection): Object<CountRejection<'_>> =
        serde_json::from_slice(bytes).map_err(|_| BackendError::CountSchema)?;
    if rejection.error.0.code != "context_length_exceeded" {
        return Err(BackendError::CountSchema);
    }
    match (policy.adaptive_context, rejection.context_tokens) {
        (_, None) => Ok(BackendError::Admission(Rejection::ContextLimit)),
        (true, Some(limit)) if limit <= crate::MAX_CONTEXT_TOKENS => {
            Ok(BackendError::ContextLimit(limit))
        }
        _ => Err(BackendError::CountSchema),
    }
}

fn context_present<'de, D: Deserializer<'de>>(input: D) -> Result<Option<usize>, D::Error> {
    usize::deserialize(input).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Count<'a> {
    input_tokens: usize,
    #[serde(borrow)]
    object: Cow<'a, str>,
    #[serde(default, deserialize_with = "context_present")]
    context_tokens: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CountRejection<'a> {
    #[serde(borrow)]
    error: Object<CountRejectionCode<'a>>,
    #[serde(default, deserialize_with = "context_present")]
    context_tokens: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CountRejectionCode<'a> {
    #[serde(borrow)]
    code: Cow<'a, str>,
}

/// Send the admitted bytes unchanged to the sole generation route.
///
/// The returned owned decoder is suitable for [`crate::response::send_chat_response`], including errors
/// on incomplete framing. It enforces the same absolute body deadline itself.
///
/// # Errors
/// Returns [`BackendError`] for an expired deadline, stream failure, invalid
/// HTTP response metadata or unsupported body framing.
pub async fn request_chat<S>(
    stream: S,
    chat: &AdmittedChat,
    policy: &BackendPolicy,
    deadline: impl Into<RequestDeadline>,
) -> Result<Response<BackendBody<S>>, BackendError>
where
    S: AsyncRead + AsyncWrite + Unpin + 'static,
{
    let deadline = deadline.into();
    let (status, body) = exchange(
        stream,
        chat.backend_body(),
        chat.streams_response(),
        "/v1/chat/completions",
        policy,
        deadline,
    )
    .await?;
    if status == StatusCode::PAYLOAD_TOO_LARGE {
        let bytes = bounded_count_body(body, deadline).await?;
        return Err(context_rejection(&bytes, policy)?);
    }
    Response::builder()
        .version(Version::HTTP_11)
        .status(status)
        .header(
            http::header::CONTENT_TYPE,
            if chat.streams_response() {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .body(body)
        .map_err(|_| BackendError::Metadata)
}

// Production transfers the body into the exchange so it can be released before
// waiting for headers. Keep the borrowed public adapter for existing callers.
pub(crate) async fn request_chat_owned<S>(
    stream: S,
    chat: AdmittedChat,
    policy: &BackendPolicy,
    deadline: RequestDeadline,
) -> Result<BackendBody<S>, BackendError>
where
    S: AsyncRead + AsyncWrite + Unpin + 'static,
{
    let (status, body) = exchange(
        stream,
        chat.body,
        chat.stream,
        "/v1/chat/completions",
        policy,
        deadline,
    )
    .await?;
    if status == StatusCode::PAYLOAD_TOO_LARGE {
        let bytes = bounded_count_body(body, deadline).await?;
        return Err(context_rejection(&bytes, policy)?);
    }
    Ok(body)
}

async fn exchange<S, B: AsRef<[u8]>>(
    mut stream: S,
    body: B,
    streaming: bool,
    route: &str,
    policy: &BackendPolicy,
    deadline: RequestDeadline,
) -> Result<(StatusCode, BackendBody<S>), BackendError>
where
    S: AsyncRead + AsyncWrite + Unpin + 'static,
{
    let write_deadline = deadline.earlier(Instant::now() + WRITE_TIMEOUT);
    ensure_time(write_deadline)?;
    let head = format!(
        "POST {route} HTTP/1.1\r\nHost: {}\r\nAuthorization: {}\r\nContent-Type: application/json\r\nAccept-Encoding: identity\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        policy.authority,
        policy.authorization,
        body.as_ref().len()
    );
    write_deadline
        .timeout(async {
            stream.write_all(head.as_bytes()).await?;
            stream.write_all(body.as_ref()).await?;
            stream.flush().await
        })
        .await
        .map_err(|_| BackendError::Deadline)?
        .map_err(|_| BackendError::Io)?;
    ensure_time(write_deadline)?;
    // Counting borrows these exact bytes for subsequent generation. The owned
    // generation path needs no prompt storage once the complete write is flushed.
    drop(body);

    let mut reader = BufReader::with_capacity(8192, stream);
    let (status, framing) = read_head(
        &mut reader,
        streaming,
        route == COUNT_ROUTE || policy.adaptive_context,
        deadline,
    )
    .await?;
    ensure_time(deadline)?;
    Ok((
        status,
        BackendBody {
            reader: Some(reader),
            framing,
            remaining_budget: MAX_RESPONSE_BYTES,
            metadata_bytes: 0,
            deadline,
            timer: deadline.timer(),
        },
    ))
}

async fn read_head<S: AsyncRead + Unpin>(
    reader: &mut BufReader<S>,
    streaming: bool,
    counting: bool,
    deadline: RequestDeadline,
) -> Result<(StatusCode, Framing), BackendError> {
    ensure_time(deadline)?;
    let available = deadline
        .timeout(reader.fill_buf())
        .await
        .map_err(|_| BackendError::Deadline)?
        .map_err(|_| BackendError::Io)?;
    ensure_time(deadline)?;
    if available.is_empty() {
        return Err(BackendError::WireSyntax);
    }
    let available = &available[..available.len().min(MAX_HEADER_BYTES)];
    if let Some(end) = wire::head_end(available, 0) {
        // Complete ordinary heads borrow the reader allocation. Only fragmented
        // heads need a second buffer; coalesced body bytes stay in the reader.
        let parsed = parse_head(&available[..end], streaming, counting)?;
        reader.consume(end);
        return Ok(parsed);
    }
    let mut raw = Vec::with_capacity(MAX_HEADER_BYTES);
    raw.extend_from_slice(available);
    let count = available.len();
    reader.consume(count);
    loop {
        ensure_time(deadline)?;
        if raw.len() == MAX_HEADER_BYTES {
            return Err(BackendError::HeadLimit);
        }
        let available = deadline
            .timeout(reader.fill_buf())
            .await
            .map_err(|_| BackendError::Deadline)?
            .map_err(|_| BackendError::Io)?;
        ensure_time(deadline)?;
        if available.is_empty() {
            return Err(BackendError::WireSyntax);
        }
        let count = available.len().min(MAX_HEADER_BYTES - raw.len());
        let previous = raw.len();
        raw.extend_from_slice(&available[..count]);
        if let Some(end) = wire::head_end(&raw, previous) {
            // Leave coalesced body bytes in the owned reader, not the head.
            reader.consume(end - previous);
            raw.truncate(end);
            break;
        }
        reader.consume(count);
    }
    parse_head(&raw, streaming, counting)
}

fn parse_head(
    raw: &[u8],
    streaming: bool,
    counting: bool,
) -> Result<(StatusCode, Framing), BackendError> {
    if !raw.starts_with(b"HTTP/1.1 ") || !wire::valid_line_endings(raw) {
        return Err(BackendError::WireSyntax);
    }
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut parsed = httparse::Response::new(&mut headers);
    match parsed.parse(raw) {
        Ok(httparse::Status::Complete(end)) if end == raw.len() => {}
        Err(httparse::Error::TooManyHeaders) => return Err(BackendError::HeadLimit),
        _ => return Err(BackendError::WireSyntax),
    }
    let status = match parsed.code {
        Some(200) => StatusCode::OK,
        Some(413) if counting => StatusCode::PAYLOAD_TOO_LARGE,
        _ => return Err(BackendError::Metadata),
    };
    if parsed.version != Some(1) {
        return Err(BackendError::Metadata);
    }
    let headers = &*parsed.headers;
    if !wire::valid_header_syntax(headers) {
        return Err(BackendError::WireSyntax);
    }
    if status == StatusCode::PAYLOAD_TOO_LARGE {
        validate_count_rejection_metadata(headers)?;
    } else {
        validate_wire_content_metadata(headers, streaming)?;
    }
    if wire::has_header(headers, "trailer") || wire::has_header(headers, "upgrade") {
        return Err(BackendError::Framing);
    }
    if let Some(connection) =
        wire::singleton_slice(headers, "connection").map_err(|_| BackendError::Framing)?
        && !connection.eq_ignore_ascii_case(b"close")
        && !connection.eq_ignore_ascii_case(b"keep-alive")
    {
        return Err(BackendError::Framing);
    }
    let length =
        wire::singleton_slice(headers, "content-length").map_err(|_| BackendError::Framing)?;
    let transfer =
        wire::singleton_slice(headers, "transfer-encoding").map_err(|_| BackendError::Framing)?;
    let framing = match (length, transfer) {
        (Some(bytes), None) => {
            let length = wire::content_length(bytes).map_err(|_| BackendError::Framing)?;
            if length > MAX_RESPONSE_BYTES {
                return Err(BackendError::Body);
            }
            Framing::Length(length)
        }
        (None, Some(bytes)) if bytes.eq_ignore_ascii_case(b"chunked") => Framing::Size {
            value: 0,
            digits: 0,
            cr: false,
        },
        _ => return Err(BackendError::Framing),
    };
    Ok((status, framing))
}

fn validate_wire_content_metadata(
    headers: &[httparse::Header<'_>],
    streaming: bool,
) -> Result<(), BackendError> {
    if wire::has_header(headers, "content-encoding") {
        return Err(BackendError::Metadata);
    }
    let content_type = wire::singleton_slice(headers, "content-type")
        .map_err(|_| BackendError::Metadata)?
        .ok_or(BackendError::Metadata)?;
    if !valid_content_type(content_type, streaming) {
        return Err(BackendError::Metadata);
    }
    Ok(())
}

fn validate_count_rejection_metadata(headers: &[httparse::Header<'_>]) -> Result<(), BackendError> {
    validate_wire_content_metadata(headers, false)?;
    for (name, expected) in [
        ("connection", b"close".as_slice()),
        ("cache-control", b"no-store"),
        ("x-content-type-options", b"nosniff"),
    ] {
        if !wire::singleton_slice(headers, name)
            .map_err(|_| BackendError::Metadata)?
            .is_some_and(|value| value.eq_ignore_ascii_case(expected))
        {
            return Err(BackendError::Metadata);
        }
    }
    Ok(())
}

enum Framing {
    Length(usize),
    Size {
        value: usize,
        digits: usize,
        cr: bool,
    },
    Data(usize),
    DataCr,
    DataLf,
    EndCr,
    EndLf,
    Complete,
    Failed,
}

/// A bounded owned HTTP body decoder; raw socket EOF is never framed completion.
///
/// Errors are sticky and close the underlying stream immediately. Framed EOF
/// does not wait for socket EOF; surplus is discarded on Drop, never reused.
/// Chunk extensions/trailers are deliberately unsupported for the pinned server.
pub struct BackendBody<S> {
    reader: Option<BufReader<S>>,
    framing: Framing,
    remaining_budget: usize,
    metadata_bytes: usize,
    deadline: RequestDeadline,
    timer: Option<Pin<Box<Sleep>>>,
}

impl<S> BackendBody<S> {
    fn fail<T>(&mut self, kind: io::ErrorKind) -> Poll<io::Result<T>> {
        self.framing = Framing::Failed;
        self.reader = None;
        Poll::Ready(Err(io::Error::new(kind, "backend body rejected")))
    }
}

impl<S: AsyncRead + Unpin> BackendBody<S> {
    // Polonius lets success lend the reader's buffer while disjoint failure
    // branches tear down its owner through fail().
    fn poll_available(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        let Some(reader) = self.reader.as_mut() else {
            return self.fail(io::ErrorKind::InvalidData);
        };
        let Ok(available) = ready!(Pin::new(reader).poll_fill_buf(cx)) else {
            return self.fail(io::ErrorKind::Other);
        };
        // A ready read may cross the deadline after the state-machine check.
        if self.deadline.expired() {
            return self.fail(io::ErrorKind::TimedOut);
        }
        if available.is_empty() {
            return self.fail(io::ErrorKind::UnexpectedEof);
        }
        Poll::Ready(Ok(available))
    }

    fn poll_payload(
        &mut self,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
        remaining: usize,
    ) -> Poll<io::Result<usize>> {
        if output.remaining() <= remaining {
            // Let BufReader read directly into final payload storage when its
            // buffer is empty and the caller offers at least its capacity. The
            // output bound must fit inside this payload: framing bytes may
            // never escape here.
            let Some(reader) = self.reader.as_mut() else {
                return self.fail(io::ErrorKind::InvalidData);
            };
            let previous = output.filled().len();
            if ready!(Pin::new(reader).poll_read(cx, output)).is_err() {
                return self.fail(io::ErrorKind::Other);
            }
            if self.deadline.expired() {
                return self.fail(io::ErrorKind::TimedOut);
            }
            let count = output.filled().len() - previous;
            if count == 0 {
                return self.fail(io::ErrorKind::UnexpectedEof);
            }
            Poll::Ready(Ok(count))
        } else {
            // Near a framing boundary, copy only its payload prefix and leave
            // the coalesced ending/next chunk buffered.
            let available = ready!(self.poll_available(cx))?;
            let count = available.len().min(remaining).min(output.remaining());
            output.put_slice(&available[..count]);
            let Some(reader) = self.reader.as_mut() else {
                return self.fail(io::ErrorKind::InvalidData);
            };
            Pin::new(reader).consume(count);
            Poll::Ready(Ok(count))
        }
    }
}

impl<S: AsyncRead + Unpin> BackendBody<S> {
    const fn advance_payload(&mut self, count: usize) {
        self.framing = match self.framing {
            Framing::Length(remaining) => Framing::Length(remaining - count),
            Framing::Data(remaining) if count == remaining => Framing::DataCr,
            Framing::Data(remaining) => Framing::Data(remaining - count),
            _ => return,
        };
        self.remaining_budget -= count;
    }

    // Keep the complete framing state machine visible in one audit boundary.
    fn poll_framing(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        let this = self;
        // Bound immediately-ready metadata work so cancellation can be polled.
        for _ in 0..1024 {
            if matches!(this.framing, Framing::Failed) {
                return this.fail(io::ErrorKind::InvalidData);
            }
            if this.deadline.expired()
                || this
                    .timer
                    .as_mut()
                    .is_some_and(|timer| timer.as_mut().poll(cx).is_ready())
            {
                return this.fail(io::ErrorKind::TimedOut);
            }
            match this.framing {
                Framing::Length(0) | Framing::Complete => return Poll::Ready(Ok(0)),
                Framing::Length(remaining) | Framing::Data(remaining) => {
                    return Poll::Ready(Ok(remaining));
                }
                _ => {}
            }
            let byte = ready!(this.poll_available(cx))?[0];
            let Some(reader) = this.reader.as_mut() else {
                return this.fail(io::ErrorKind::InvalidData);
            };
            Pin::new(reader).consume(1);
            this.metadata_bytes += 1;
            if this.metadata_bytes > MAX_CHUNK_METADATA_BYTES {
                return this.fail(io::ErrorKind::InvalidData);
            }
            this.framing = match this.framing {
                Framing::Size {
                    value,
                    digits: _,
                    cr: true,
                } if byte == b'\n' => {
                    if value > this.remaining_budget {
                        return this.fail(io::ErrorKind::InvalidData);
                    }
                    if value == 0 {
                        Framing::EndCr
                    } else {
                        Framing::Data(value)
                    }
                }
                Framing::Size {
                    value,
                    digits,
                    cr: false,
                } if byte == b'\r' && digits > 0 => Framing::Size {
                    value,
                    digits,
                    cr: true,
                },
                Framing::Size {
                    value,
                    digits,
                    cr: false,
                } if byte.is_ascii_hexdigit() && digits + 3 <= MAX_CHUNK_LINE_BYTES => {
                    let digit = match byte {
                        b'0'..=b'9' => byte - b'0',
                        b'a'..=b'f' => byte - b'a' + 10,
                        _ => byte - b'A' + 10,
                    };
                    let Some(value) = value
                        .checked_mul(16)
                        .and_then(|n| n.checked_add(usize::from(digit)))
                    else {
                        return this.fail(io::ErrorKind::InvalidData);
                    };
                    Framing::Size {
                        value,
                        digits: digits + 1,
                        cr: false,
                    }
                }
                Framing::DataCr if byte == b'\r' => Framing::DataLf,
                Framing::DataLf if byte == b'\n' => Framing::Size {
                    value: 0,
                    digits: 0,
                    cr: false,
                },
                Framing::EndCr if byte == b'\r' => Framing::EndLf,
                Framing::EndLf if byte == b'\n' => Framing::Complete,
                _ => return this.fail(io::ErrorKind::InvalidData),
            };
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for BackendBody<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let this = self.get_mut();
        let remaining = ready!(this.poll_framing(cx))?;
        if remaining != 0 {
            let count = ready!(this.poll_payload(cx, output, remaining))?;
            this.advance_payload(count);
        }
        Poll::Ready(Ok(()))
    }
}

impl<S: AsyncRead + Unpin> AsyncBufRead for BackendBody<S> {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        let this = self.get_mut();
        let remaining = ready!(this.poll_framing(cx))?;
        if remaining == 0 {
            return Poll::Ready(Ok(&[]));
        }
        let available = ready!(this.poll_available(cx))?;
        Poll::Ready(Ok(&available[..available.len().min(remaining)]))
    }

    fn consume(self: Pin<&mut Self>, amount: usize) {
        let this = self.get_mut();
        let (Framing::Length(remaining) | Framing::Data(remaining)) = this.framing else {
            return;
        };
        if let Some(reader) = &mut this.reader {
            // AsyncBufRead callers may consume only the slice just exposed.
            // Clamp defensively so misuse cannot eat framing or underflow caps.
            let count = amount.min(remaining).min(reader.buffer().len());
            Pin::new(reader).consume(count);
            this.advance_payload(count);
        }
    }
}

fn ensure_time(deadline: RequestDeadline) -> Result<(), BackendError> {
    if deadline.expired() {
        Err(BackendError::Deadline)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod memory_tests;

#[cfg(test)]
mod payload_tests;
