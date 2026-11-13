//! One active transcription owner, with a bounded lease after complete success.

use super::{AtomicU64, Duration, Instant, Ordering, Service, WARM_IDLE, canary};

pub(super) struct Input {
    pub(super) client: canary::Client,
    pub(super) config: canary::Config,
    pub(super) generation: Option<u64>,
    pub(super) service: Option<Service>,
    pub(super) warm_until: Option<Instant>,
}

impl Input {
    pub(super) fn new(config: &canary::Config) -> Option<Self> {
        let client = canary::Client::new(config).ok()?;
        if config.launch.is_none() && client.ready().is_err() {
            return None;
        }
        Some(Self {
            client,
            config: config.clone(),
            generation: None,
            service: None,
            warm_until: None,
        })
    }

    pub(super) fn clear(&mut self) {
        self.service = None;
        self.generation = None;
        self.warm_until = None;
    }

    pub(super) fn maintain(&mut self, current: &AtomicU64, now: Instant) {
        let stale = self
            .generation
            .is_some_and(|generation| current.load(Ordering::Acquire) != generation);
        let expired = self.warm_until.is_some_and(|until| now >= until);
        let failed = self
            .service
            .as_ref()
            .is_some_and(|service| service.check().is_err());
        if stale || expired || failed {
            self.clear();
        }
    }

    // Only the complete microphone request or complete file import may detach
    // its owner from cancellation of that generation. Segment success is not
    // completion of an import, and therefore cannot renew the idle lease.
    pub(super) fn complete(&mut self, generation: u64, current: &AtomicU64, now: Instant) {
        if self.generation != Some(generation) || current.load(Ordering::Acquire) != generation {
            self.clear();
            return;
        }
        self.generation = None;
        if self
            .service
            .as_ref()
            .is_some_and(|service| service.check().is_ok())
        {
            self.warm_until = Some(now + WARM_IDLE);
        } else {
            self.clear();
        }
    }

    /// Bind preparation to the active capture, without granting an idle lease.
    pub(super) fn prepare(
        &mut self,
        generation: u64,
        current: &AtomicU64,
    ) -> Result<(), canary::Failure> {
        self.activate(generation, current, None)
    }

    fn activate(
        &mut self,
        generation: u64,
        current: &AtomicU64,
        wav: Option<&[u8]>,
    ) -> Result<(), canary::Failure> {
        let call = canary::Call::new(generation, current, Duration::from_mins(15));
        call.check()?;
        self.maintain(current, Instant::now());
        if self.generation != Some(generation) {
            let retained = self.warm_until.take().is_some()
                && self
                    .service
                    .as_ref()
                    .is_some_and(|service| service.check_listener(self.config.port).is_ok());
            if retained {
                // Authentication, exact model/format and listener ownership
                // are rechecked before every reuse of a completed owner.
                self.client.ready_for(generation, current)?;
                if let Some(service) = &self.service {
                    service.check_listener(self.config.port)?;
                }
            } else {
                self.clear();
                self.service = match wav {
                    Some(wav) => {
                        self.client
                            .start_service(&self.config, generation, wav, current)?
                    }
                    None => self
                        .client
                        .prepare_recording(&self.config, generation, current)?,
                };
            }
            self.generation = Some(generation);
        }
        if let Some(service) = &self.service {
            service.check()?;
        }
        call.check()
    }

    pub(super) fn transcribe(
        &mut self,
        generation: u64,
        wav: &[u8],
        current: &AtomicU64,
    ) -> Result<String, canary::Failure> {
        let call = canary::Call::new(generation, current, Duration::from_mins(15));
        self.activate(generation, current, Some(wav))?;
        let result = self.client.transcribe(generation, wav, current);
        if let Some(service) = &self.service {
            service.check()?;
        }
        call.check()?;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::{AtomicU64, Duration, Input, Instant, Ordering, Service, WARM_IDLE, canary};
    use bonsai_windows_credentials::PrivateCredentials;
    use bonsai_windows_process::{OwnedProcess, ProcessSpec};
    use std::{io, path::PathBuf, sync::Arc};

    struct Fixture {
        input: Input,
        process: Arc<OwnedProcess>,
        credentials: PrivateCredentials,
        directory: PathBuf,
    }

    impl Fixture {
        fn new() -> io::Result<Self> {
            Self::create(false)
        }

        fn create(listener: bool) -> io::Result<Self> {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_nanos();
            let directory = std::env::temp_dir().join(format!("Bonsai-warm-input-{stamp}"));
            std::fs::create_dir(&directory)?;
            let credentials = PrivateCredentials::create(&directory)?;
            let mut config = canary::Config {
                port: 8766,
                key_file: credentials.frontend_path().to_owned(),
                launch: None,
            };
            let process = Arc::new(OwnedProcess::start(&ProcessSpec {
                executable: crate::native_speech::tests::resource_guard_executable(&directory)?,
                arguments: if listener {
                    vec![config.key_file.clone().into_os_string()]
                } else {
                    Vec::new()
                },
                working_directory: directory.clone(),
                stdout: directory.join("fake.stdout"),
                stderr: directory.join("fake.stderr"),
                memory_limit_bytes: 512 * 1024 * 1024,
                active_process_limit: 1,
            })?);
            crate::native_speech::tests::await_resource_ready(&process, &directory, "fake")?;
            if listener {
                let ready = std::fs::read_to_string(directory.join("fake.stdout"))?;
                config.port = ready
                    .split_whitespace()
                    .nth(1)
                    .ok_or_else(|| io::Error::other("Fixture listener port missing"))?
                    .parse()
                    .map_err(io::Error::other)?;
            }
            let service = Service::monitored(Arc::clone(&process), || Ok(9 * 1024 * 1024 * 1024))
                .map_err(|_| {
                io::Error::other("Could not monitor disposable recognition owner.")
            })?;
            Ok(Self {
                input: Input {
                    client: canary::Client::new(&config).map_err(io::Error::other)?,
                    config,
                    generation: Some(41),
                    service: Some(service),
                    warm_until: None,
                },
                process,
                credentials,
                directory,
            })
        }

        fn finish(mut self) -> io::Result<()> {
            self.input.clear();
            assert!(
                self.process
                    .wait_for_exit(Duration::from_secs(5))?
                    .is_some()
            );
            drop(self.input);
            drop(self.credentials);
            std::fs::remove_dir_all(self.directory)
        }
    }

    #[test]
    fn warm_input_success_retains_its_original_owner_only_until_idle_expiry() -> io::Result<()> {
        let mut fixture = Fixture::new()?;
        let now = Instant::now();
        let before_expiry = now + WARM_IDLE.saturating_sub(Duration::from_millis(1));
        let current = AtomicU64::new(41);
        fixture.input.complete(41, &current, now);
        assert!(fixture.input.generation.is_none());
        assert_eq!(fixture.input.warm_until, Some(now + WARM_IDLE));
        current.store(super::super::NO_INPUT, Ordering::Release);
        fixture.input.maintain(&current, before_expiry);
        assert!(fixture.input.service.is_some());
        assert!(fixture.process.try_wait()?.is_none());
        // Starting another capture neither extends the lease nor binds its
        // cancellation to a request that has already completed.
        current.store(42, Ordering::Release);
        fixture.input.maintain(&current, before_expiry);
        assert_eq!(fixture.input.warm_until, Some(now + WARM_IDLE));
        fixture.input.maintain(&current, now + WARM_IDLE);
        assert!(fixture.input.service.is_none());
        assert!(fixture.input.warm_until.is_none());
        fixture.finish()
    }

    #[test]
    fn warm_input_cancelled_completion_and_explicit_stop_release_the_owner() -> io::Result<()> {
        for completed in [false, true] {
            let mut fixture = Fixture::new()?;
            let current = AtomicU64::new(if completed { 41 } else { 42 });
            fixture.input.complete(41, &current, Instant::now());
            assert_eq!(fixture.input.service.is_some(), completed);
            fixture.input.clear();
            assert!(fixture.input.service.is_none());
            assert!(fixture.input.generation.is_none());
            assert!(fixture.input.warm_until.is_none());
            fixture.finish()?;
        }
        Ok(())
    }

    #[test]
    fn warm_input_new_generation_transcribes_on_the_same_authenticated_owned_listener()
    -> io::Result<()> {
        let mut fixture = Fixture::create(true)?;
        let current = AtomicU64::new(41);
        fixture.input.complete(41, &current, Instant::now());
        current.store(42, Ordering::Release);
        let header = bonsai_audio::mono_pcm16_header(16_000, 2)
            .ok_or_else(|| io::Error::other("Could not frame fixture PCM"))?;
        let mut wave = header.to_vec();
        wave.extend_from_slice(&[0, 0, 1, 0]);
        let text = fixture
            .input
            .transcribe(42, &wave, &current)
            .map_err(|_| io::Error::other("Retained recognition request failed"))?;
        assert_eq!(text, "fresh request");
        assert_eq!(fixture.input.generation, Some(42));
        assert!(fixture.input.service.is_some());
        assert!(fixture.input.warm_until.is_none());
        assert_eq!(
            bonsai_windows_process::listener_process_id(fixture.input.config.port)?,
            Some(fixture.process.id())
        );
        assert!(fixture.process.try_wait()?.is_none());
        fixture.finish()
    }

    #[test]
    fn recording_preparation_reuses_the_owner_without_completing_or_renewing_input()
    -> io::Result<()> {
        let mut fixture = Fixture::create(true)?;
        let current = AtomicU64::new(41);
        fixture.input.complete(41, &current, Instant::now());
        current.store(42, Ordering::Release);
        let (events, received) = std::sync::mpsc::channel();
        super::super::prepare_input(Some(&mut fixture.input), 42, &current, &events);
        assert!(received.try_recv().is_err());
        assert_eq!(fixture.input.generation, Some(42));
        assert!(fixture.input.warm_until.is_none());
        assert!(fixture.process.try_wait()?.is_none());
        // A duplicate start notification is idempotent, without a new owner.
        fixture
            .input
            .prepare(42, &current)
            .map_err(|_| io::Error::other("Repeated recording preparation failed"))?;
        assert!(fixture.input.warm_until.is_none());
        // The prepared owner still validates the complete final recording.
        assert!(fixture.input.transcribe(42, &[], &current).is_err());
        assert!(fixture.input.warm_until.is_none());
        let mut wave = bonsai_audio::mono_pcm16_header(16_000, 2)
            .ok_or_else(|| io::Error::other("Could not frame fixture PCM"))?
            .to_vec();
        wave.extend_from_slice(&[0, 0, 1, 0]);
        assert_eq!(
            fixture
                .input
                .transcribe(42, &wave, &current)
                .map_err(|_| { io::Error::other("Prepared recording transcription failed") })?,
            "fresh request"
        );
        assert!(fixture.input.warm_until.is_none());
        assert_eq!(
            bonsai_windows_process::listener_process_id(fixture.input.config.port)?,
            Some(fixture.process.id())
        );
        // Cancellation during capture releases the prepared owner, even though
        // no complete successful input granted an idle lease.
        current.store(super::super::NO_INPUT, Ordering::Release);
        fixture.input.maintain(&current, Instant::now());
        assert!(fixture.input.service.is_none());
        assert!(fixture.input.generation.is_none());
        fixture.finish()
    }
}
