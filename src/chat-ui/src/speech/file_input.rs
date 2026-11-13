//! Sequential ASR requests overlap decoding of at most one following segment.

use super::{AtomicU64, Event, Input, Ordering, POLL, Sender, canary};
use crate::audio_import::{Chunk, Session};

pub(super) fn transcribe(
    mut input: Option<&mut Input>,
    generation: u64,
    first: Chunk,
    mut import: Session,
    current: &AtomicU64,
    events: &Sender<Event>,
) {
    let result = input.as_deref_mut().map_or(
        Err(canary::Failure::Message("Qwen3-ASR 1.7B is unavailable. Restart the app to prepare speech, then try the audio file again.")),
        |input| run(generation, first, &mut import, current, events,
            &mut |wav| input.transcribe(generation, wav, current), &mut admit_text),
    );
    if let Some(input) = input {
        if result.is_ok() {
            input.complete(generation, current, std::time::Instant::now());
        } else {
            input.clear();
        }
    }
    if current.load(Ordering::Acquire) != generation {
        return;
    }
    match result {
        Ok(()) => {
            let _sent = events.send(Event::FileFinished { generation });
        }
        Err(canary::Failure::Cancelled) => {}
        Err(canary::Failure::Message(message)) => {
            let _sent = events.send(Event::InputFailed {
                generation,
                message,
            });
        }
    }
}

fn run(
    generation: u64,
    mut chunk: Chunk,
    import: &mut Session,
    current: &AtomicU64,
    events: &Sender<Event>,
    transcribe: &mut impl FnMut(&[u8]) -> Result<String, canary::Failure>,
    admit: &mut impl FnMut(usize) -> Result<(), canary::Failure>,
) -> Result<(), canary::Failure> {
    let mut retained_bytes = 0_usize;
    loop {
        check(generation, current)?;
        let _sent = events.send(Event::FileProgress {
            generation,
            progress: Some(super::FileProgress {
                start: chunk.start,
                end: chunk.end,
                total: chunk.total,
            }),
        });
        // The current segment has left the importer's single delivery slot.
        // Let its one decoder fill that slot while ASR handles this request;
        // another advance is issued only after the prefetched segment is taken.
        if !chunk.last {
            import.advance();
        }
        let text = transcribe(&chunk.wav)?;
        check(generation, current)?;
        // The response is request-bounded, while the complete transcript has no
        // fixed length cap. Check actual RAM/commit before retaining each part.
        retained_bytes = retained_bytes.saturating_add(text.len()).saturating_add(1);
        // Growing a String may allocate a new complete buffer before releasing
        // its previous allocation. Admit that peak, not just this small part.
        admit(retained_bytes)?;
        if events.send(Event::FilePart { generation, text }).is_err() {
            return Err(canary::Failure::Cancelled);
        }
        if chunk.last {
            return Ok(());
        }
        drop(chunk);
        let _sent = events.send(Event::FileProgress {
            generation,
            progress: None,
        });
        chunk = loop {
            check(generation, current)?;
            match import.poll() {
                Some(Ok(Some(chunk))) => break chunk,
                Some(Ok(None)) => {
                    return Err(canary::Failure::Message(
                        "The audio file ended unexpectedly. Any completed transcript is available to view or copy.",
                    ));
                }
                Some(Err(message)) => return Err(canary::Failure::Message(message)),
                None => std::thread::sleep(POLL),
            }
        };
    }
}

fn admit_text(length: usize) -> Result<(), canary::Failure> {
    let memory = bonsai_windows_process::memory_status().map_err(|_| canary::Failure::Message(
        "Transcription stopped because available memory could not be checked. Any completed transcript is available to view or copy.",
    ))?;
    // Allow a new complete String allocation and its handoff copies while the
    // previous retained allocation is still live.
    let growth = u64::try_from(length).unwrap_or(u64::MAX).saturating_mul(3);
    if memory
        .available_physical_bytes
        .min(memory.available_commit_bytes)
        < growth
    {
        return Err(canary::Failure::Message(
            "Not enough available memory to retain more transcript. Close other apps and try again. Any completed transcript is available to view or copy.",
        ));
    }
    Ok(())
}

fn check(generation: u64, current: &AtomicU64) -> Result<(), canary::Failure> {
    if current.load(Ordering::Acquire) == generation {
        Ok(())
    } else {
        Err(canary::Failure::Cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io,
        sync::mpsc,
        time::{Duration, Instant},
    };

    fn first_chunk(import: &mut Session) -> io::Result<Chunk> {
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(result) = import.poll() {
                return result
                    .map_err(io::Error::other)?
                    .ok_or_else(|| io::Error::other("Missing first file segment"));
            }
            if Instant::now() >= until {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "First file segment was not delivered",
                ));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn chunk(index: u8, last: bool) -> Chunk {
        Chunk {
            wav: vec![index],
            start: Duration::from_secs(u64::from(index) * 30),
            end: Duration::from_secs(u64::from(index + 1) * 30),
            total: Duration::from_secs(90),
            last,
        }
    }

    #[test]
    fn audio_file_stream_transcribes_segments_in_order_including_the_tail() -> io::Result<()> {
        let mut import = Session::fixture(vec![
            Ok(Some(chunk(0, false))),
            Ok(Some(chunk(1, false))),
            Ok(Some(chunk(2, true))),
        ])?;
        let first = first_chunk(&mut import)?;
        let current = AtomicU64::new(7);
        let (events, incoming) = mpsc::channel();
        let mut seen = Vec::new();
        let result = run(
            7,
            first,
            &mut import,
            &current,
            &events,
            &mut |wav| {
                seen.push(wav[0]);
                Ok(format!("Part {}", wav[0]))
            },
            &mut |_| Ok(()),
        );
        assert_eq!(result, Ok(()));
        assert_eq!(seen, vec![0, 1, 2]);
        let parts: Vec<_> = incoming
            .try_iter()
            .filter_map(|event| match event {
                Event::FilePart { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(parts, vec!["Part 0", "Part 1", "Part 2"]);
        Ok(())
    }

    #[test]
    fn audio_file_stream_decodes_one_segment_ahead_during_recognition() -> io::Result<()> {
        let (mut import, decoded) = Session::fixture_observed(vec![
            Ok(Some(chunk(0, false))),
            Ok(Some(chunk(1, false))),
            Ok(Some(chunk(2, true))),
        ])?;
        let first = first_chunk(&mut import)?;
        decoded
            .recv_timeout(Duration::from_secs(2))
            .map_err(io::Error::other)?;
        let current = AtomicU64::new(7);
        let (events, _incoming) = mpsc::channel();
        let mut bounded = true;
        let mut seen = Vec::new();
        let result = run(
            7,
            first,
            &mut import,
            &current,
            &events,
            &mut |wav| {
                seen.push(wav[0]);
                if wav[0] < 2 {
                    // A rendezvous establishes overlap without timing model
                    // inference or accepting a later decode after it returns.
                    decoded.recv_timeout(Duration::from_secs(2)).map_err(|_| {
                        canary::Failure::Message("Next segment was not decoded during recognition.")
                    })?;
                    bounded &= decoded.try_recv().is_err();
                }
                Ok(format!("Part {}", wav[0]))
            },
            &mut |_| Ok(()),
        );
        drop(import);
        assert_eq!(result, Ok(()));
        assert_eq!(seen, vec![0, 1, 2]);
        assert!(bounded);
        Ok(())
    }

    #[test]
    fn audio_file_stream_cancellation_discards_prefetch_and_late_recognition() -> io::Result<()> {
        let (mut import, decoded) = Session::fixture_observed(vec![
            Ok(Some(chunk(0, false))),
            Ok(Some(chunk(1, false))),
            Ok(Some(chunk(2, true))),
        ])?;
        let first = first_chunk(&mut import)?;
        decoded
            .recv_timeout(Duration::from_secs(2))
            .map_err(io::Error::other)?;
        let current = AtomicU64::new(7);
        let (events, incoming) = mpsc::channel();
        let result = run(
            7,
            first,
            &mut import,
            &current,
            &events,
            &mut |_| {
                decoded.recv_timeout(Duration::from_secs(2)).map_err(|_| {
                    canary::Failure::Message("Cancellation fixture did not prefetch a segment.")
                })?;
                current.store(8, Ordering::Release);
                Ok("Late text".to_owned())
            },
            &mut |_| Ok(()),
        );
        // Joined cancellation needs no extra advance or draining of the
        // prefetched segment, and must not decode the remaining tail.
        drop(import);
        assert_eq!(result, Err(canary::Failure::Cancelled));
        assert!(decoded.try_recv().is_err());
        assert!(
            !incoming
                .try_iter()
                .any(|event| matches!(event, Event::FilePart { .. } | Event::FileFinished { .. }))
        );
        Ok(())
    }

    #[test]
    fn audio_file_stream_failure_keeps_completed_parts_without_finishing() -> io::Result<()> {
        let error = "The audio file could not be decoded.";
        let mut import = Session::fixture(vec![Ok(Some(chunk(0, false))), Err(error)])?;
        let first = first_chunk(&mut import)?;
        let current = AtomicU64::new(7);
        let (events, incoming) = mpsc::channel();
        let result = run(
            7,
            first,
            &mut import,
            &current,
            &events,
            &mut |_| Ok("Completed text".to_owned()),
            &mut |_| Ok(()),
        );
        assert_eq!(result, Err(canary::Failure::Message(error)));
        assert!(incoming.try_iter().any(
            |event| matches!(event, Event::FilePart { text, .. } if text == "Completed text")
        ));
        Ok(())
    }
}
