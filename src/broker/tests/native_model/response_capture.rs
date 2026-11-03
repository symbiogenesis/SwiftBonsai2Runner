//! Response identities survive failed reads and cancellation without retaining payloads.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Only sibling acceptance observers share these bounded response identities."
)]

use super::*;
use std::{
    cell::RefCell,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, ReadBuf};

const HEADER_LIMIT: usize = 16 * 1024;

struct Capture {
    hash: Option<bonsai_windows_credentials::Sha256>,
    bytes: u64,
    header: Vec<u8>,
    header_complete: bool,
    header_limit_exceeded: bool,
    connection_closed: bool,
    error_kind: Option<io::ErrorKind>,
    failure_phase: Option<&'static str>,
}

impl Capture {
    fn record(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            self.connection_closed = true;
            return;
        }
        self.bytes += u64::try_from(bytes.len()).unwrap();
        if self
            .hash
            .as_mut()
            .is_some_and(|hash| hash.update(bytes).is_err())
        {
            self.hash = None;
        }
        // Keep only the bounded header while classifying it. Incoming bodies
        // are hashed once and never copied into an observation buffer.
        for byte in bytes {
            if self.header_complete || self.header_limit_exceeded {
                break;
            }
            if self.header.len() == HEADER_LIMIT {
                self.header_limit_exceeded = true;
                break;
            }
            self.header.push(*byte);
            self.header_complete = self.header.ends_with(b"\r\n\r\n");
        }
    }

    fn status_code(&self) -> Option<u16> {
        let status = self.header.strip_prefix(b"HTTP/1.1 ")?;
        if status.get(3) != Some(&b' ') {
            return None;
        }
        let code = status.get(..3)?;
        if !code.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let code = std::str::from_utf8(code).ok()?.parse().ok()?;
        (100..=599).contains(&code).then_some(code)
    }
}

/// One incremental identity shared with the reader until its deadline or result.
pub(super) struct Observation(Rc<RefCell<Capture>>);

impl Observation {
    pub(super) fn new() -> Self {
        Self(Rc::new(RefCell::new(Capture {
            hash: bonsai_windows_credentials::Sha256::new().ok(),
            bytes: 0,
            header: Vec::new(),
            header_complete: false,
            header_limit_exceeded: false,
            connection_closed: false,
            error_kind: None,
            failure_phase: None,
        })))
    }

    pub(super) fn reader<S>(&self, stream: S) -> ObservedResponse<S> {
        ObservedResponse {
            stream,
            capture: self.0.clone(),
        }
    }

    pub(super) fn connection_error(&self, kind: io::ErrorKind) {
        let mut capture = self.0.borrow_mut();
        capture.error_kind = Some(kind);
        capture.failure_phase = Some("connect-or-write");
    }

    /// Finalize once after the bounded read future returned or was cancelled.
    /// A held prefix reader may keep the socket alive for Stop, but is not read again.
    pub(super) fn finish(self) -> Value {
        let mut capture = self.0.borrow_mut();
        let digest = capture.hash.take().and_then(|hash| hash.finish().ok());
        let digest = digest.map(|digest| {
            digest
                .into_iter()
                .flat_map(|byte| [byte >> 4, byte & 0x0f])
                .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
                .collect::<String>()
        });
        json!({"responseBytes":capture.bytes,"responseSha256":digest,
            "responseSha256Status":if digest.is_some() {"observed-prefix-hashed"} else {"native-hash-unavailable"},
            "responseHeaderBytesObserved":capture.header.len(),
            "responseHeaderComplete":capture.header_complete,
            "responseHeaderLimitExceeded":capture.header_limit_exceeded,
            "responseHttpStatus":capture.status_code(),
            "connectionClosed":capture.connection_closed,
            "transportErrorObserved":capture.error_kind.is_some(),
            "transportErrorKind":capture.error_kind.map(|kind|format!("{kind:?}")),
            "transportFailurePhase":capture.failure_phase,
            "responseIdentityScope":"Exact bytes returned by transport reads, including parser read-ahead; a partial digest never establishes complete framing or generation. Raw headers and bodies are not retained."})
    }
}

/// Captures only byte identity and fixed classification while preserving socket ownership.
pub(super) struct ObservedResponse<S> {
    stream: S,
    capture: Rc<RefCell<Capture>>,
}

impl<S: AsyncRead + Unpin> AsyncRead for ObservedResponse<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if buffer.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let first = buffer.filled().len();
        match Pin::new(&mut this.stream).poll_read(cx, buffer) {
            Poll::Ready(Ok(())) => {
                this.capture.borrow_mut().record(&buffer.filled()[first..]);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => {
                let mut capture = this.capture.borrow_mut();
                capture.error_kind = Some(error.kind());
                capture.failure_phase = Some("read");
                Poll::Ready(Err(error))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[test]
fn native_response_capture_hashes_fragmented_errors_without_retaining_payloads() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let payload = b"HTTP/1.1 413 private-reason\r\nX-Private: secret-header\r\n\r\nsecret-body";
        let observation = Observation::new();
        let mut reader = observation.reader(io::Cursor::new(payload));
        let mut received = Vec::new();
        let mut scratch = [0; 3];
        loop {
            let count = reader.read(&mut scratch).await.unwrap();
            if count == 0 {
                break;
            }
            received.extend_from_slice(&scratch[..count]);
        }
        let observed = observation.finish();
        assert_eq!(received, payload);
        assert_eq!(observed["responseBytes"], payload.len());
        assert_eq!(observed["responseSha256"], adapter_digest(payload));
        assert_eq!(observed["responseSha256Status"], "observed-prefix-hashed");
        assert_eq!(observed["responseHeaderComplete"], true);
        assert_eq!(observed["responseHttpStatus"], 413);
        assert_eq!(observed["connectionClosed"], true);
        assert_eq!(observed["transportErrorObserved"], false);
        let retained = observed.to_string();
        for private in [
            "private-reason",
            "secret-header",
            "secret-body",
            "X-Private",
        ] {
            assert!(!retained.contains(private));
        }
    });
}

#[test]
fn native_response_capture_cancelled_read_retains_prefix_and_original_deadline() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .unwrap();
    runtime.block_on(async {
        let (stream, mut peer) = tokio::io::duplex(128);
        let prefix = b"HTTP/1.1 200 OK\r\n";
        peer.write_all(prefix).await.unwrap();
        let observation = Observation::new();
        let mut reader = observation.reader(stream);
        let mut received = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        assert!(
            tokio::time::timeout_at(deadline, reader.read_to_end(&mut received))
                .await
                .is_err()
        );
        let observed = observation.finish();
        assert_eq!(tokio::time::Instant::now(), deadline);
        assert_eq!(observed["responseBytes"], prefix.len());
        assert_eq!(observed["responseSha256"], adapter_digest(prefix));
        assert_eq!(observed["responseHeaderComplete"], false);
        assert_eq!(observed["connectionClosed"], false);
        assert_eq!(observed["transportErrorObserved"], false);
    });
}

#[test]
fn native_response_capture_caps_header_storage_and_classifies_connection_failure() {
    let observation = Observation::new();
    observation
        .0
        .borrow_mut()
        .record(&vec![b'x'; HEADER_LIMIT + 1]);
    let observed = observation.finish();
    assert_eq!(observed["responseHeaderBytesObserved"], HEADER_LIMIT);
    assert_eq!(observed["responseHeaderLimitExceeded"], true);
    assert!(observed["responseHttpStatus"].is_null());
    for malformed in [
        b"HTTP/1.1 2000 extra\r\n\r\n".as_slice(),
        b"HTTP/1.1 200",
        b"HTTP/1.1 099 invalid\r\n\r\n",
        b"HTTP/1.1 600 invalid\r\n\r\n",
    ] {
        let observation = Observation::new();
        observation.0.borrow_mut().record(malformed);
        assert!(observation.finish()["responseHttpStatus"].is_null());
    }
    let observation = Observation::new();
    observation.connection_error(io::ErrorKind::ConnectionRefused);
    let observed = observation.finish();
    assert_eq!(observed["responseBytes"], 0);
    assert_eq!(observed["responseSha256"], adapter_digest(b""));
    assert_eq!(observed["transportErrorKind"], "ConnectionRefused");
    assert_eq!(observed["transportFailurePhase"], "connect-or-write");
    assert_eq!(observed["connectionClosed"], false);
}
