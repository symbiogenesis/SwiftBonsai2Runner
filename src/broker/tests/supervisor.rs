#![forbid(unsafe_code)]
//! Focused regressions for one-request orchestration and real disconnect reads.
#![expect(
    clippy::unwrap_used,
    reason = "test setup and successful-operation assertions must panic on failure"
)]

use bonsai_broker::{
    Rejection, RequestPolicy,
    backend::{BackendError, BackendPolicy},
    deadline::RequestDeadline,
    supervisor::{BackendConnector, ServeError, serve_chat},
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    future::{pending, poll_fn},
    io,
    pin::{Pin, pin},
    rc::Rc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf, duplex},
    time::{Instant, Sleep, sleep_until},
};

const CLIENT_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const BACKEND_KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const COUNT: &[u8] = br#"{"input_tokens":100,"object":"response.input_tokens"}"#;

fn run(test: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(test);
}

fn policy() -> RequestPolicy {
    RequestPolicy::new(8080, CLIENT_KEY).unwrap()
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_mins(1)
}

fn request(stream: bool) -> Vec<u8> {
    let body = format!(
        r#"{{"messages":[{{"role":"user","content":"Keep the rain.\nKeep the voice."}}],"max_completion_tokens":128,"stream":{stream}}}"#
    );
    request_body(body.as_bytes())
}

fn request_body(body: &[u8]) -> Vec<u8> {
    let mut bytes = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:8080\r\nAuthorization: Bearer {CLIENT_KEY}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn assert_error_reply(bytes: &[u8], status: &str, code: &str) {
    let wire = std::str::from_utf8(bytes).unwrap();
    let (head, body) = wire.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with(status));
    assert!(head.contains("\r\nContent-Type: application/json\r\n"));
    assert!(head.contains(&format!("\r\nContent-Length: {}", body.len())));
    assert!(head.contains("\r\nConnection: close\r\n"));
    assert!(head.contains("\r\nCache-Control: no-store\r\n"));
    assert_eq!(body, format!(r#"{{"error":{{"code":"{code}"}}}}"#));
    assert_eq!(wire.matches("HTTP/1.1 ").count(), 1);
    assert!(!wire.contains(CLIENT_KEY) && !wire.contains(BACKEND_KEY));
    assert!(!wire.contains("private diagnostic") && !wire.contains("Keep the rain"));
}

fn response(body: &[u8], stream: bool) -> Vec<u8> {
    let mut bytes = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
        if stream {
            "text/event-stream"
        } else {
            "application/json"
        },
        body.len()
    )
    .into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

#[derive(Default)]
struct State {
    written: Vec<u8>,
    reads: usize,
    dropped: bool,
}

struct BackendStream {
    input: Vec<u8>,
    offset: usize,
    stall: bool,
    delayed: Option<(usize, Pin<Box<Sleep>>)>,
    state: Rc<RefCell<State>>,
}

impl BackendStream {
    fn new(input: Vec<u8>) -> Self {
        Self {
            input,
            offset: 0,
            stall: false,
            delayed: None,
            state: Rc::new(RefCell::new(State::default())),
        }
    }

    fn stalled() -> Self {
        let mut stream = Self::new(Vec::new());
        stream.stall = true;
        stream
    }
}

impl AsyncRead for BackendStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.state.borrow_mut().reads += 1;
        let offset = self.offset;
        let remaining_before_delay = match &mut self.delayed {
            Some((after, timer)) if offset >= *after => {
                if timer.as_mut().poll(cx).is_pending() {
                    return Poll::Pending;
                }
                usize::MAX
            }
            Some((after, _)) => *after - offset,
            None => usize::MAX,
        };
        let count = buffer
            .remaining()
            .min(self.input.len() - self.offset)
            .min(remaining_before_delay);
        if count == 0 && self.stall {
            return Poll::Pending;
        }
        buffer.put_slice(&self.input[self.offset..self.offset + count]);
        self.offset += count;
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for BackendStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.state.borrow_mut().written.extend_from_slice(bytes);
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl Drop for BackendStream {
    fn drop(&mut self) {
        self.state.borrow_mut().dropped = true;
    }
}

struct Connector {
    policy: BackendPolicy,
    streams: RefCell<VecDeque<BackendStream>>,
    calls: Cell<usize>,
    stall_connect: bool,
}

impl Connector {
    fn new(streams: impl IntoIterator<Item = BackendStream>) -> Self {
        Self {
            policy: BackendPolicy::new(8081, BACKEND_KEY).unwrap(),
            streams: RefCell::new(streams.into_iter().collect()),
            calls: Cell::new(0),
            stall_connect: false,
        }
    }
}

impl BackendConnector for Connector {
    type Stream = BackendStream;

    fn policy(&self) -> &BackendPolicy {
        &self.policy
    }

    #[expect(
        clippy::future_not_send,
        reason = "the owned fixture intentionally tracks cancellation on a local runtime"
    )]
    async fn connect(&self, _deadline: Instant) -> Result<Self::Stream, BackendError> {
        self.calls.set(self.calls.get() + 1);
        let stream = self
            .streams
            .borrow_mut()
            .pop_front()
            .ok_or(BackendError::Io)?;
        if self.stall_connect {
            pending::<()>().await;
        }
        Ok(stream)
    }
}

struct Client {
    stream: DuplexStream,
    dropped: Rc<Cell<bool>>,
}

impl AsyncRead for Client {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buffer)
    }
}

impl AsyncWrite for Client {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, bytes)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.dropped.set(true);
    }
}

fn clients() -> (Client, DuplexStream, Rc<Cell<bool>>) {
    let (stream, peer) = duplex(8192);
    let dropped = Rc::new(Cell::new(false));
    (
        Client {
            stream,
            dropped: dropped.clone(),
        },
        peer,
        dropped,
    )
}

async fn both<A: Future, B: Future>(a: A, b: B) -> (A::Output, B::Output) {
    let (mut a, mut b) = (pin!(a), pin!(b));
    let (mut a_out, mut b_out) = (None, None);
    poll_fn(|cx| {
        if a_out.is_none()
            && let Poll::Ready(value) = a.as_mut().poll(cx)
        {
            a_out = Some(value);
        }
        if b_out.is_none()
            && let Poll::Ready(value) = b.as_mut().poll(cx)
        {
            b_out = Some(value);
        }
        if a_out.is_some() && b_out.is_some() {
            Poll::Ready((a_out.take().unwrap(), b_out.take().unwrap()))
        } else {
            Poll::Pending
        }
    })
    .await
}

async fn response_head(peer: &mut DuplexStream) {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        head.push(peer.read_u8().await.unwrap());
        assert!(head.len() < 1024);
    }
    assert!(head.starts_with(b"HTTP/1.1 200 OK\r\n"));
}

#[test]
fn canonical_count_and_generation_share_body_and_drop_streams() {
    run(async {
        let counter = BackendStream::new(response(COUNT, false));
        let generation = BackendStream::new(response(br#"{"choices":[]}"#, false));
        let count_state = counter.state.clone();
        let generation_state = generation.state.clone();
        let backend = Connector::new([counter, generation]);
        let (client, mut peer, dropped) = clients();
        let policy = policy();
        let (result, bytes) = both(serve_chat(client, &policy, &backend, deadline()), async {
            peer.write_all(&request(false)).await.unwrap();
            let mut bytes = Vec::new();
            peer.read_to_end(&mut bytes).await.unwrap();
            bytes
        })
        .await;
        assert_eq!(result, Ok(()));
        assert_eq!(backend.calls.get(), 2);
        let count = count_state.borrow();
        let generation = generation_state.borrow();
        let count_wire = String::from_utf8(count.written.clone()).unwrap();
        let generation_wire = String::from_utf8(generation.written.clone()).unwrap();
        assert!(count_wire.starts_with("POST /v1/chat/completions/input_tokens HTTP/1.1\r\n"));
        assert!(generation_wire.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
        let counted_body = count_wire.split_once("\r\n\r\n").unwrap().1;
        let generated_body = generation_wire.split_once("\r\n\r\n").unwrap().1;
        assert_eq!(counted_body, generated_body);
        let generated: serde_json::Value = serde_json::from_str(generated_body).unwrap();
        let original_request = String::from_utf8(request(false)).unwrap();
        let original: serde_json::Value =
            serde_json::from_str(original_request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(generated["messages"], original["messages"]);
        assert_eq!(
            generated["chat_template_kwargs"],
            serde_json::json!({
                "enable_thinking": false,
                "reasoning_effort": "xhigh",
                "preserve_thinking": true,
            })
        );
        assert!(generated_body.contains("\"cache_prompt\":false"));
        assert!(generated_body.contains("\"temperature\":1.0"));
        assert!(!generation_wire.contains(CLIENT_KEY));
        assert!(generation_wire.contains(BACKEND_KEY));
        assert!(bytes.ends_with(br#"{"choices":[]}"#));
        assert!(count.dropped && generation.dropped && dropped.get());
    });
}

#[test]
fn rejected_count_never_connects_generation() {
    run(async {
        for (body, rejection, stream) in [
            (
                br#"{"input_tokens":9000,"object":"response.input_tokens"}"#.as_slice(),
                Rejection::ContextLimit,
                false,
            ),
            (
                br#"{"input_tokens":9000,"object":"response.input_tokens"}"#.as_slice(),
                Rejection::ContextLimit,
                true,
            ),
            (
                br#"{"input_tokens":0,"object":"response.input_tokens"}"#.as_slice(),
                Rejection::TokenCountUnavailable,
                false,
            ),
        ] {
            let counter = BackendStream::new(response(body, false));
            let state = counter.state.clone();
            let backend = Connector::new([counter]);
            let (client, mut peer, dropped) = clients();
            let policy = policy();
            let (result, bytes) = both(serve_chat(client, &policy, &backend, deadline()), async {
                peer.write_all(&request(stream)).await.unwrap();
                let mut bytes = Vec::new();
                peer.read_to_end(&mut bytes).await.unwrap();
                bytes
            })
            .await;
            assert_eq!(
                result,
                Err(ServeError::Backend(BackendError::Admission(rejection)))
            );
            assert_eq!(backend.calls.get(), 1);
            if rejection == Rejection::ContextLimit {
                let wire = String::from_utf8(bytes).unwrap();
                let (head, body) = wire.split_once("\r\n\r\n").unwrap();
                assert!(head.starts_with("HTTP/1.1 413 Content Too Large\r\n"));
                assert!(head.contains("\r\nContent-Type: application/json\r\n"));
                assert!(head.contains(&format!("\r\nContent-Length: {}", body.len())));
                assert!(head.contains("\r\nCache-Control: no-store\r\n"));
                assert_eq!(body, r#"{"error":{"code":"context_length_exceeded"}}"#);
                assert!(!wire.contains(CLIENT_KEY));
                assert!(!wire.contains(BACKEND_KEY));
                assert!(!wire.contains("Keep the rain"));
            } else {
                assert_error_reply(
                    &bytes,
                    "HTTP/1.1 502 Bad Gateway\r\n",
                    "backend_token_count_failed",
                );
            }
            assert!(state.borrow().dropped && dropped.get());
        }
    });
}

#[test]
fn typed_private_context_rejection_sends_only_sanitized_public_admission_error() {
    run(async {
        for streaming in [false, true] {
            for recognized in [false, true] {
                let body = if recognized {
                    r#"{"error":{"code":"context_length_exceeded"}}"#
                } else {
                    r#"{"error":{"code":"unknown_failure"}}"#
                };
                let bytes = format!(
                    "HTTP/1.1 413 Content Too Large\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nSet-Cookie: private-data\r\n\r\n{body}",body.len()
                ).into_bytes();
                let counter = BackendStream::new(bytes);
                let state = counter.state.clone();
                let backend = Connector::new([counter]);
                let (client, mut peer, dropped) = clients();
                let policy = policy();
                let (result, bytes) =
                    both(serve_chat(client, &policy, &backend, deadline()), async {
                        peer.write_all(&request(streaming)).await.unwrap();
                        let mut bytes = Vec::new();
                        peer.read_to_end(&mut bytes).await.unwrap();
                        bytes
                    })
                    .await;
                assert_eq!(backend.calls.get(), 1);
                if recognized {
                    assert_eq!(
                        result,
                        Err(ServeError::Backend(BackendError::Admission(
                            Rejection::ContextLimit
                        )))
                    );
                    let wire = String::from_utf8(bytes).unwrap();
                    let (head, body) = wire.split_once("\r\n\r\n").unwrap();
                    assert!(head.starts_with("HTTP/1.1 413 Content Too Large\r\n"));
                    assert!(head.contains("\r\nCache-Control: no-store\r\n"));
                    assert_eq!(body, r#"{"error":{"code":"context_length_exceeded"}}"#);
                    assert!(!wire.contains("private-data") && !wire.contains("Set-Cookie"));
                    assert!(!wire.contains(CLIENT_KEY) && !wire.contains(BACKEND_KEY));
                } else {
                    assert_eq!(result, Err(ServeError::Backend(BackendError::CountSchema)));
                    assert_error_reply(
                        &bytes,
                        "HTTP/1.1 502 Bad Gateway\r\n",
                        "backend_token_count_failed",
                    );
                }
                assert!(state.borrow().dropped && dropped.get());
            }
        }
    });
}

#[test]
fn fully_received_policy_rejections_report_canonical_size_and_invalid_requests() {
    run(async {
        let expanded = serde_json::to_vec(&serde_json::json!({
            "messages": [{"role": "user", "content": "x".repeat(bonsai_broker::MAX_BODY_BYTES - 128)}]
        }))
        .unwrap();
        assert!(expanded.len() <= bonsai_broker::MAX_BODY_BYTES);
        for (body, rejection, status, code) in [
            (
                expanded.as_slice(),
                Rejection::BodySize,
                "HTTP/1.1 413 Content Too Large\r\n",
                "request_body_too_large",
            ),
            (
                b"{}".as_slice(),
                Rejection::ChatSchema,
                "HTTP/1.1 400 Bad Request\r\n",
                "invalid_chat_request",
            ),
            (
                br#"{"messages":[{"role":"user","content":"private diagnostic"}],"max_completion_tokens":4097}"#,
                Rejection::CompletionLimit,
                "HTTP/1.1 400 Bad Request\r\n",
                "invalid_chat_request",
            ),
            (
                br#"{"messages":[{"role":"user","content":"<think>"}]}"#,
                Rejection::ChatControlTokens,
                "HTTP/1.1 400 Bad Request\r\n",
                "input_control_tokens_rejected",
            ),
        ] {
            let backend = Connector::new([]);
            let (client, mut peer, dropped) = clients();
            let policy = policy();
            let (result, bytes) = both(serve_chat(client, &policy, &backend, deadline()), async {
                peer.write_all(&request_body(body)).await.unwrap();
                let mut bytes = Vec::new();
                peer.read_to_end(&mut bytes).await.unwrap();
                bytes
            })
            .await;
            let expected = if rejection == Rejection::ChatControlTokens {
                Ok(())
            } else {
                Err(ServeError::Receive(
                    bonsai_broker::reception::ReceiveError::Policy(rejection),
                ))
            };
            assert_eq!(result, expected);
            assert_eq!(backend.calls.get(), 0);
            assert_error_reply(&bytes, status, code);
            assert!(dropped.get());
        }
    });
}

#[test]
fn backend_failures_before_response_report_counting_or_generation_stage() {
    run(async {
        let private = b"HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: 18\r\n\r\nprivate diagnostic";
        for (streams, calls, error, code) in [
            (vec![], 1, BackendError::Io, "backend_token_count_failed"),
            (
                vec![BackendStream::new(private.to_vec())],
                1,
                BackendError::Metadata,
                "backend_token_count_failed",
            ),
            (
                vec![BackendStream::new(response(
                    br#"{"private diagnostic":1}"#,
                    false,
                ))],
                1,
                BackendError::CountSchema,
                "backend_token_count_failed",
            ),
            (
                vec![BackendStream::new(response(COUNT, false))],
                2,
                BackendError::Io,
                "backend_generation_failed",
            ),
            (
                vec![
                    BackendStream::new(response(COUNT, false)),
                    BackendStream::new(private.to_vec()),
                ],
                2,
                BackendError::Metadata,
                "backend_generation_failed",
            ),
            (
                vec![
                    BackendStream::new(response(COUNT, false)),
                    BackendStream::new(b"private diagnostic\r\n\r\n".to_vec()),
                ],
                2,
                BackendError::WireSyntax,
                "backend_generation_failed",
            ),
        ] {
            let backend = Connector::new(streams);
            let (client, mut peer, dropped) = clients();
            let policy = policy();
            let (result, bytes) = both(serve_chat(client, &policy, &backend, deadline()), async {
                peer.write_all(&request(true)).await.unwrap();
                let mut bytes = Vec::new();
                peer.read_to_end(&mut bytes).await.unwrap();
                bytes
            })
            .await;
            assert_eq!(result, Err(ServeError::Backend(error)));
            assert_eq!(backend.calls.get(), calls);
            assert_error_reply(&bytes, "HTTP/1.1 502 Bad Gateway\r\n", code);
            assert!(dropped.get());
        }
    });
}

#[test]
fn backend_failure_diagnostics_preserve_stage_and_redact_private_values() {
    use std::process::{Child, Command, Stdio};

    // Direct writes to stderr bypass libtest's capture. Replay only the named,
    // already bounded in-memory fixture in a child; no environment mutation or
    // recursive invocation of this test is needed.
    struct Replay(Option<Child>);

    impl Drop for Replay {
        fn drop(&mut self) {
            if let Some(mut child) = self.0.take() {
                let _killed = child.kill();
                let _joined = child.wait();
            }
        }
    }

    let mut replay = Replay(Some(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "backend_failures_before_response_report_counting_or_generation_stage",
                "--nocapture",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ));
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while replay.0.as_mut().unwrap().try_wait().unwrap().is_none() {
        assert!(
            std::time::Instant::now() < until,
            "Diagnostic replay timed out."
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = replay.0.take().unwrap().wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    let observed = stderr
        .lines()
        .map(|line| {
            let (category, elapsed) = line.split_once(", elapsed_ms=").unwrap();
            // Require one complete, numeric elapsed observation on each record.
            elapsed.strip_suffix('.').unwrap().parse::<u128>().unwrap();
            category
        })
        .collect::<Vec<_>>();
    assert_eq!(
        observed,
        [
            "Local model request failed: stage=token-count, error=Io",
            "Local model request failed: stage=token-count, error=Metadata",
            "Local model request failed: stage=token-count, error=CountSchema",
            "Local model request failed: stage=generation, error=Io",
            "Local model request failed: stage=generation, error=Metadata",
            "Local model request failed: stage=generation, error=WireSyntax",
        ]
    );
    for private in [
        CLIENT_KEY,
        BACKEND_KEY,
        "private diagnostic",
        "Keep the rain",
    ] {
        assert!(!stderr.contains(private));
    }
}

#[test]
fn truncated_generation_does_not_append_an_admission_error() {
    run(async {
        let counter = BackendStream::new(response(COUNT, false));
        let generation = BackendStream::new(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 10\r\n\r\npartial".to_vec(),
        );
        let state = generation.state.clone();
        let backend = Connector::new([counter, generation]);
        let (client, mut peer, dropped) = clients();
        let policy = policy();
        let (result, bytes) = both(serve_chat(client, &policy, &backend, deadline()), async {
            peer.write_all(&request(true)).await.unwrap();
            let mut bytes = Vec::new();
            peer.read_to_end(&mut bytes).await.unwrap();
            bytes
        })
        .await;
        assert_eq!(
            result,
            Err(ServeError::Response(
                bonsai_broker::response::ResponseError::BackendIo
            ))
        );
        assert_eq!(backend.calls.get(), 2);
        let wire = String::from_utf8(bytes).unwrap();
        assert!(wire.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(!wire.contains("413"));
        assert!(!wire.contains("context_length_exceeded"));
        assert!(!wire.contains("backend_generation_failed"));
        assert_eq!(wire.matches("HTTP/1.1 ").count(), 1);
        assert!(!wire.ends_with("0\r\n\r\n"));
        assert!(state.borrow().dropped && dropped.get());
    });
}

#[test]
fn disconnect_during_count_drops_backend_and_client() {
    run(async {
        let counter = BackendStream::stalled();
        let state = counter.state.clone();
        let backend = Connector::new([counter]);
        let (client, mut peer, dropped) = clients();
        let policy = policy();
        let (result, ()) = both(serve_chat(client, &policy, &backend, deadline()), async {
            peer.write_all(&request(false)).await.unwrap();
            tokio::task::yield_now().await;
            assert!(state.borrow().reads > 0);
            peer.shutdown().await.unwrap();
        })
        .await;
        assert_eq!(result, Err(ServeError::ClientAbandoned));
        assert_eq!(backend.calls.get(), 1);
        assert!(state.borrow().dropped && dropped.get());
    });
}

#[test]
fn disconnect_or_surplus_during_generation_drops_backend() {
    run(async {
        for surplus in [false, true] {
            let counter = BackendStream::new(response(COUNT, false));
            let mut generation = BackendStream::new(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 100\r\n\r\n".to_vec());
            generation.stall = true;
            let state = generation.state.clone();
            let backend = Connector::new([counter, generation]);
            let (client, mut peer, dropped) = clients();
            let policy = policy();
            let (result, ()) = both(
                serve_chat(client, &policy, &backend, RequestDeadline::NONE),
                async {
                    peer.write_all(&request(true)).await.unwrap();
                    response_head(&mut peer).await;
                    if surplus {
                        peer.write_all(b"x").await.unwrap();
                    } else {
                        peer.shutdown().await.unwrap();
                    }
                },
            )
            .await;
            assert_eq!(result, Err(ServeError::ClientAbandoned));
            assert_eq!(backend.calls.get(), 2);
            assert!(state.borrow().dropped && dropped.get());
        }
    });
}

#[test]
fn untimed_generation_accepts_delayed_headers_and_body_with_virtual_time() {
    run(async {
        // Tokio time is paused: this tests policy without a model or wall-time wait.
        for streaming in [false, true] {
            for delay_body in [false, true] {
                let payload = if streaming {
                    b"data: [DONE]\n\n".as_slice()
                } else {
                    b"{}".as_slice()
                };
                let wire = response(payload, streaming);
                let after = if delay_body {
                    wire.len() - payload.len()
                } else {
                    0
                };
                let start = Instant::now();
                let delay = Duration::from_mins(11);
                let mut generation = BackendStream::new(wire);
                generation.delayed = Some((after, Box::pin(sleep_until(start + delay))));
                let state = generation.state.clone();
                let backend =
                    Connector::new([BackendStream::new(response(COUNT, false)), generation]);
                let (client, mut peer, dropped) = clients();
                let policy = policy();
                let (result, bytes) = both(
                    serve_chat(client, &policy, &backend, RequestDeadline::NONE),
                    async {
                        peer.write_all(&request(streaming)).await.unwrap();
                        let mut bytes = Vec::new();
                        peer.read_to_end(&mut bytes).await.unwrap();
                        bytes
                    },
                )
                .await;
                assert_eq!(result, Ok(()));
                assert_eq!(Instant::now() - start, delay);
                assert!(bytes.windows(payload.len()).any(|part| part == payload));
                assert_eq!(backend.calls.get(), 2);
                assert!(state.borrow().dropped && dropped.get());
            }
        }
    });
}

#[test]
fn supplied_deadline_cancels_an_unfinished_connection() {
    run(async {
        let counter = BackendStream::stalled();
        let state = counter.state.clone();
        let mut backend = Connector::new([counter]);
        backend.stall_connect = true;
        let (client, mut peer, dropped) = clients();
        let policy = policy();
        let start = Instant::now();
        let limit = Duration::from_secs(2);
        let (result, ()) = both(
            serve_chat(client, &policy, &backend, start + limit),
            async {
                peer.write_all(&request(false)).await.unwrap();
            },
        )
        .await;
        assert_eq!(result, Err(ServeError::Deadline));
        assert_eq!(Instant::now() - start, limit);
        assert_eq!(backend.calls.get(), 1);
        assert!(state.borrow().dropped && dropped.get());
    });
}

#[test]
fn supplied_deadline_is_not_shortened_by_a_default_request_cap() {
    run(async {
        for bound in [Duration::from_secs(2), Duration::from_mins(20)] {
            let counter = BackendStream::stalled();
            let state = counter.state.clone();
            let backend = Connector::new([counter]);
            let (client, mut peer, dropped) = clients();
            let policy = policy();
            let start = Instant::now();
            let (result, ()) = both(
                serve_chat(client, &policy, &backend, start + bound),
                async {
                    peer.write_all(&request(false)).await.unwrap();
                },
            )
            .await;
            assert_eq!(result, Err(ServeError::Deadline));
            assert_eq!(Instant::now() - start, bound);
            assert_eq!(backend.calls.get(), 1);
            assert!(state.borrow().dropped && dropped.get());
        }
    });
}

#[test]
fn expired_deadline_never_connects_or_reads_request() {
    run(async {
        let backend = Connector::new([]);
        let (client, _peer, dropped) = clients();
        assert_eq!(
            serve_chat(client, &policy(), &backend, Instant::now()).await,
            Err(ServeError::Deadline)
        );
        assert_eq!(backend.calls.get(), 0);
        assert!(dropped.get());
    });
}
