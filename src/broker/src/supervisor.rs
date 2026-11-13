//! One-request orchestration over owned client and private backend streams.
//!
//! No listener, process, task, retry or connection reuse is created here. The
//! caller must bound concurrency and enforce immutable backend ownership and
//! bypass prevention before this adapter is suitable for daily use.

use crate::{
    Rejection, RequestPolicy,
    backend::{BackendError, BackendPolicy, count_and_admit, request_chat_owned},
    deadline::RequestDeadline,
    reception::{
        BudgetedChat, INTAKE_TIMEOUT, ReceiveError, ReceivedChat, discard_rate_body,
        receive_chat_with_budget,
    },
    response::{
        ErrorReply, ResponseError, send_buffered_chat_response_mode, send_error_reply,
        send_rate_rejection,
    },
    service::BurstBudget,
    transport::CONNECT_TIMEOUT,
};
use std::{future::poll_fn, pin::pin, task::Poll};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, split},
    time::Instant,
};

/// A connector preserving one fixed private backend endpoint and HTTP policy.
///
/// Both calls for one request must use the same endpoint and separate protected
/// credential. The launcher must additionally establish model/runtime/template
/// identity and prevent backend replacement and bypass; fixed TCP metadata and
/// this trait cannot establish those properties or process isolation.
pub trait BackendConnector {
    /// Owned connection whose Drop closes the backend stream.
    type Stream: AsyncRead + AsyncWrite + Unpin + 'static;

    /// Fixed private backend HTTP metadata, shared by counting and generation.
    fn policy(&self) -> &BackendPolicy;

    /// Connect once before the supplied absolute deadline, without retries.
    ///
    /// Dropping the returned future must close any partially established stream
    /// and cancel pending connection work. No detached task may outlive it.
    fn connect(
        &self,
        deadline: Instant,
    ) -> impl Future<Output = Result<Self::Stream, BackendError>>;
}

/// Value-free lifecycle errors; no prompts, credentials or I/O messages escape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServeError {
    /// The explicitly supplied absolute deadline has expired.
    Deadline,
    /// The complete client request could not be received and authorized.
    Receive(ReceiveError),
    /// The private connection, count, admission or generation exchange failed.
    Backend(BackendError),
    /// The bounded response could not be forwarded completely.
    Response(ResponseError),
    /// A post-request read returned EOF, an error or any surplus input byte.
    ClientAbandoned,
}

/// Receive, count, admit and serve exactly one client request.
///
/// Requires a Tokio time driver. The caller supplies owned streams whose Drop
/// closes their connections. An optional caller deadline bounds the complete
/// request. Pass [`RequestDeadline::NONE`] for ordinary chat without a total
/// elapsed-time cap; intake, connection and write operations remain bounded.
///
/// After the complete body arrives, an actual read races the entire backend
/// pipeline. EOF, read failure and any surplus byte cancel the pipeline and drop
/// both endpoints. A client TCP write-half-close is deliberately treated as
/// abandonment: clients must keep their sending side open until the response
/// finishes. Pipelining and additional requests on this connection are rejected.
///
/// Counting and generation use the same canonical allocation and connector.
/// Generation is never connected until exact-count admission succeeds. Failure
/// or cancellation drops all owned streams immediately, including cancellation
/// while connecting or waiting for backend data. Backend generation/slot release
/// after connection closure must be qualified separately. No response is retried; writes
/// may already be partial. Exact context-limit admission rejection sends a fixed
/// 413 JSON error before generation. Fully received authenticated request rejections
/// and backend failures before response forwarding receive fixed JSON error codes.
/// No error response is appended after response forwarding begins.
///
/// # Errors
/// Returns [`ServeError`] for request rejection, backend/response failure,
/// deadline expiry or client abandonment.
pub async fn serve_chat<C>(
    client: C,
    policy: &RequestPolicy,
    backend: &impl BackendConnector,
    deadline: impl Into<RequestDeadline>,
) -> Result<(), ServeError>
where
    C: AsyncRead + AsyncWrite + Unpin + 'static,
{
    serve_chat_inner(client, policy, backend, deadline.into(), None).await
}

pub(crate) async fn serve_chat_with_budget<C>(
    client: C,
    policy: &RequestPolicy,
    backend: &impl BackendConnector,
    deadline: RequestDeadline,
    budget: &BurstBudget,
) -> Result<(), ServeError>
where
    C: AsyncRead + AsyncWrite + Unpin + 'static,
{
    serve_chat_inner(client, policy, backend, deadline, Some(budget)).await
}

async fn serve_chat_inner<C>(
    client: C,
    policy: &RequestPolicy,
    backend: &impl BackendConnector,
    deadline: RequestDeadline,
    budget: Option<&BurstBudget>,
) -> Result<(), ServeError>
where
    C: AsyncRead + AsyncWrite + Unpin + 'static,
{
    ensure_time(deadline)?;
    let Some(received) = receive_admitted(client, policy, deadline, budget).await? else {
        return Ok(());
    };
    let (client, pending) = received.into_parts();
    let (mut input, output) = split(client);
    let mut surplus = [0u8; 1];
    let mut abandoned = pin!(input.read(&mut surplus));
    let mut pipeline = Box::pin(async move {
        let count_started = Instant::now();
        let counted = async {
            let counter = backend
                .connect(deadline.limit(Instant::now() + CONNECT_TIMEOUT))
                .await?;
            count_and_admit(counter, pending, backend.policy(), deadline).await
        }
        .await;
        let chat = match counted {
            Ok(chat) => chat,
            Err(error) => {
                let reply = if error == BackendError::Admission(Rejection::ContextLimit) {
                    ErrorReply::ContextLimit
                } else {
                    note_backend_failure("token-count", error, count_started);
                    ErrorReply::BackendTokenCountFailed
                };
                send_error_reply(output, reply, deadline)
                    .await
                    .map_err(ServeError::Response)?;
                return Err(ServeError::Backend(error));
            }
        };
        let streaming = chat.streams_response();
        let generation_started = Instant::now();
        let generated = async {
            let generation = backend
                .connect(deadline.limit(Instant::now() + CONNECT_TIMEOUT))
                .await?;
            request_chat_owned(generation, chat, backend.policy(), deadline).await
        }
        .await;
        let response = match generated {
            Ok(response) => response,
            Err(error) => {
                note_backend_failure("generation", error, generation_started);
                send_error_reply(output, ErrorReply::BackendGenerationFailed, deadline)
                    .await
                    .map_err(ServeError::Response)?;
                return Err(ServeError::Backend(error));
            }
        };
        send_buffered_chat_response_mode(output, response, streaming, deadline)
            .await
            .map_err(ServeError::Response)
    });
    let mut timer = pin!(deadline.wait());
    let result = poll_fn(|cx| {
        // Explicit time checks prevent immediately-ready futures from winning
        // after expiry. Abandonment wins ties over successful pipeline progress.
        if deadline.expired() || timer.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(ServeError::Deadline));
        }
        if abandoned.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(ServeError::ClientAbandoned));
        }
        pipeline.as_mut().poll(cx)
    })
    .await;
    ensure_time(deadline)?;
    result
}

// Retain the failure category before a client write or teardown can obscure it.
// BackendError contains only fixed classifications, never upstream bytes,
// prompts, credentials or native I/O messages. Expected context rejection is
// handled above without recording a model failure.
fn note_backend_failure(stage: &str, error: BackendError, started: Instant) {
    use std::io::Write as _;

    let _recorded = writeln!(
        std::io::stderr().lock(),
        "Local model request failed: stage={stage}, error={error:?}, elapsed_ms={}.",
        started.elapsed().as_millis()
    );
}

async fn receive_admitted<C>(
    client: C,
    policy: &RequestPolicy,
    deadline: RequestDeadline,
    budget: Option<&BurstBudget>,
) -> Result<Option<ReceivedChat<C>>, ServeError>
where
    C: AsyncRead + AsyncWrite + Unpin + 'static,
{
    match receive_chat_with_budget(
        client,
        policy,
        deadline.limit(Instant::now() + INTAKE_TIMEOUT),
        budget,
    )
    .await
    .map_err(ServeError::Receive)?
    {
        BudgetedChat::Ready(received) => Ok(Some(received)),
        BudgetedChat::RateLimited {
            mut stream,
            remaining,
            intake_deadline,
        } => {
            send_rate_rejection(&mut stream, intake_deadline.into())
                .await
                .map_err(ServeError::Response)?;
            discard_rate_body(&mut stream, remaining, intake_deadline)
                .await
                .map_err(ServeError::Receive)?;
            Ok(None)
        }
        BudgetedChat::Rejected(client, rejection) => {
            let reply = match rejection {
                Rejection::ChatControlTokens => ErrorReply::ControlTokens,
                Rejection::BodySize => ErrorReply::RequestBodyTooLarge,
                Rejection::ChatSchema | Rejection::CompletionLimit => {
                    ErrorReply::InvalidChatRequest
                }
                _ => return Err(ServeError::Receive(ReceiveError::Policy(rejection))),
            };
            send_error_reply(client, reply, deadline)
                .await
                .map_err(ServeError::Response)?;
            if rejection == Rejection::ChatControlTokens {
                return Ok(None);
            }
            Err(ServeError::Receive(ReceiveError::Policy(rejection)))
        }
    }
}

fn ensure_time(deadline: RequestDeadline) -> Result<(), ServeError> {
    if deadline.expired() {
        Err(ServeError::Deadline)
    } else {
        Ok(())
    }
}
