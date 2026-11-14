//! Read cancellation independent of whether socket shutdown wakes a blocked read.

use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use ureq::{
    Error,
    unversioned::transport::{
        Buffers, ConnectionDetails, Connector, Either, NextTimeout, ShutdownHandle, TcpConnector,
        Transport, time::Duration as TransportDuration,
    },
};

const READ_SLICE: Duration = Duration::from_millis(50);

#[derive(Debug, Default)]
struct ReadShutdown(AtomicBool);

impl ShutdownHandle for ReadShutdown {
    fn shutdown(&self) {
        self.0.store(true, Ordering::Release);
    }
}

#[derive(Debug)]
pub(super) struct ReadPollConnector;

impl<In: Transport> Connector<In> for ReadPollConnector {
    type Out = Either<In, ReadPollTransport<<TcpConnector as Connector>::Out>>;

    fn connect(
        &self,
        details: &ConnectionDetails<'_>,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, Error> {
        if let Some(inner) = chained {
            // CONNECT recursively creates its own protected raw connection.
            // Its tunnel may already contain TLS to an HTTPS proxy: another
            // polling layer here would expose short timeouts to that TLS.
            return Ok(Some(Either::A(inner)));
        }
        let Some(inner) = TcpConnector::default().connect(details, None::<()>)? else {
            return Ok(None);
        };
        let shutdown = Arc::new(ReadShutdown::default());
        // Register before TLS/proxy negotiation and keep the flag alive with
        // the transport. Agent shutdown also prevents a late registration.
        let handle = Arc::downgrade(&shutdown);
        let handle: std::sync::Weak<dyn ShutdownHandle> = handle;
        (details.register_shutdown)(handle)?;
        Ok(Some(Either::B(ReadPollTransport { inner, shutdown })))
    }
}

#[derive(Debug)]
pub(super) struct ReadPollTransport<In> {
    inner: In,
    shutdown: Arc<ReadShutdown>,
}

fn cancelled(shutdown: &ReadShutdown) -> Result<(), Error> {
    if shutdown.0.load(Ordering::Acquire) {
        Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "HTTPS transport cancelled.",
        )
        .into())
    } else {
        Ok(())
    }
}

fn read_with_poll(
    inner: &mut impl Transport,
    shutdown: &ReadShutdown,
    timeout: NextTimeout,
    mut elapsed: impl FnMut() -> Duration,
) -> Result<bool, Error> {
    let timeout = timeout.for_read();
    loop {
        cancelled(shutdown)?;
        // Charge every slice to the original read/phase/global allowance.
        // A timeout from an individual slice must neither escape into TLS nor
        // reset that allowance. Zero must not reach TcpTransport::not_zero.
        let left = match timeout.after {
            TransportDuration::Exact(limit) => limit.saturating_sub(elapsed()),
            TransportDuration::NotHappening => READ_SLICE,
        };
        if left.is_zero() {
            return Err(Error::Timeout(timeout.reason));
        }
        let slice = NextTimeout {
            after: left.min(READ_SLICE).into(),
            per_read: None,
            ..timeout
        };
        let result = inner.await_input(slice);
        cancelled(shutdown)?;
        match result {
            Err(Error::Timeout(reason)) if reason == timeout.reason => {}
            result => return result,
        }
    }
}

impl<In: Transport> Transport for ReadPollTransport<In> {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.inner.buffers()
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), Error> {
        cancelled(&self.shutdown)?;
        // Writes can make partial progress: never retry them as read slices.
        self.inner.transmit_output(amount, timeout)?;
        cancelled(&self.shutdown)
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, Error> {
        let started = Instant::now();
        read_with_poll(&mut self.inner, &self.shutdown, timeout, || {
            started.elapsed()
        })
    }

    fn is_open(&mut self) -> bool {
        !self.shutdown.0.load(Ordering::Acquire) && self.inner.is_open()
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU64;

    use ureq::{
        Agent, Timeout,
        unversioned::{
            resolver::{DefaultResolver, Resolver as _},
            transport::{LazyBuffers, time::Instant as TransportInstant},
        },
    };

    use super::*;

    #[derive(Debug)]
    struct StalledTransport {
        buffers: LazyBuffers,
        elapsed: Arc<AtomicU64>,
        slices: Vec<Duration>,
        last_timeout: Option<NextTimeout>,
        stop: Option<Arc<ReadShutdown>>,
    }

    impl StalledTransport {
        fn new() -> Self {
            Self {
                buffers: LazyBuffers::new(8, 8),
                elapsed: Arc::new(AtomicU64::new(0)),
                slices: Vec::new(),
                last_timeout: None,
                stop: None,
            }
        }
    }

    impl Transport for StalledTransport {
        fn buffers(&mut self) -> &mut dyn Buffers {
            &mut self.buffers
        }

        fn transmit_output(&mut self, _amount: usize, _timeout: NextTimeout) -> Result<(), Error> {
            Err(io::Error::other("Unexpected fixture write.").into())
        }

        fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, Error> {
            self.last_timeout = Some(timeout);
            let TransportDuration::Exact(slice) = timeout.after else {
                return Err(io::Error::other("Read slice must be finite.").into());
            };
            self.slices.push(slice);
            let millis = u64::try_from(slice.as_millis()).map_err(io::Error::other)?;
            self.elapsed.fetch_add(millis, Ordering::AcqRel);
            if let Some(stop) = &self.stop {
                stop.shutdown();
            }
            Err(Error::Timeout(timeout.reason))
        }

        fn is_open(&mut self) -> bool {
            true
        }
    }

    #[test]
    fn read_poll_stops_without_socket_shutdown_or_waiting_for_request_deadline() {
        let shutdown = Arc::new(ReadShutdown::default());
        let mut inner = StalledTransport::new();
        inner.stop = Some(Arc::clone(&shutdown));
        let clock = Arc::clone(&inner.elapsed);
        let result = read_with_poll(
            &mut inner,
            &shutdown,
            NextTimeout {
                after: Duration::from_secs(60).into(),
                ..NextTimeout::default()
            },
            || Duration::from_millis(clock.load(Ordering::Acquire)),
        );
        assert!(
            matches!(result, Err(Error::Io(error)) if error.kind() == io::ErrorKind::ConnectionAborted)
        );
        assert_eq!(inner.slices, [Duration::from_millis(50)]);
    }

    #[test]
    fn read_poll_preserves_original_global_and_per_read_deadlines() {
        for (global, per_read, reason) in [
            (120, None, Timeout::Global),
            (5_000, Some(120), Timeout::PerRead),
            (120, Some(5_000), Timeout::Global),
        ] {
            let mut inner = StalledTransport::new();
            let clock = Arc::clone(&inner.elapsed);
            let result = read_with_poll(
                &mut inner,
                &ReadShutdown::default(),
                NextTimeout {
                    after: Duration::from_millis(global).into(),
                    per_read: per_read.map(|millis| Duration::from_millis(millis).into()),
                    ..NextTimeout::default()
                },
                || Duration::from_millis(clock.load(Ordering::Acquire)),
            );
            assert!(matches!(result, Err(Error::Timeout(actual)) if actual == reason));
            assert_eq!(inner.slices, [50, 50, 20].map(Duration::from_millis));
            assert_eq!(clock.load(Ordering::Acquire), 120);
        }
    }

    #[test]
    fn read_poll_preserves_existing_proxy_tunnel_without_another_read_slice() -> crate::Result<()> {
        let resolver = DefaultResolver::default();
        let config = Agent::config_builder().proxy(None).build();
        let uri = ureq::http::Uri::from_static("https://upstream.invalid/");
        let timeout = NextTimeout {
            after: Duration::from_secs(7).into(),
            per_read: Some(Duration::from_secs(3).into()),
            ..NextTimeout::default()
        };
        let details = ConnectionDetails {
            uri: &uri,
            addrs: resolver.empty(),
            config: &config,
            request_level: false,
            resolver: &resolver,
            now: TransportInstant::now(),
            timeout,
            current_time: Arc::new(TransportInstant::now),
            run_connector: Arc::new(|_| Err(Error::ConnectionFailed)),
            // The recursive raw transport already owns its shutdown flag.
            register_shutdown: Arc::new(|_| Err(Error::ConnectionFailed)),
        };
        // This models an existing tunnel whose is_tls() is false for the
        // endpoint, even though its underlying HTTPS-proxy connection is TLS.
        let Some(Either::A(mut tunnel)) = ReadPollConnector
            .connect(&details, Some(StalledTransport::new()))
            .map_err(|error| error.to_string())?
        else {
            return Err("Existing proxy tunnel was replaced or wrapped.".into());
        };
        assert!(matches!(
            tunnel.await_input(timeout),
            Err(Error::Timeout(Timeout::Global))
        ));
        assert_eq!(tunnel.last_timeout, Some(timeout));
        assert_eq!(tunnel.slices, [Duration::from_secs(7)]);
        Ok(())
    }
}
