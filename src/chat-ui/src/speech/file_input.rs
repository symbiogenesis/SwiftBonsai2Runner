//! Sequential file transcription keeps only one decoded ASR request in flight.

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
    let result = input.as_deref_mut().map_or_else(
        || Err(canary::Failure::Message("Qwen3-ASR 1.7B is unavailable. Restart the app to prepare speech, then try the audio file again.")),
        |input| run(generation, first, &mut import, current, events,
            &mut |wav| input.transcribe(generation, wav, current), &mut admit_text),
    );
    if result.is_err()
        && let Some(input) = input
    {
        input.clear();
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
            message: format!(
                "Transcribing audio: {:.1}–{:.1} of {:.1} seconds. Cancel or Escape stops transcription.",
                chunk.start.as_secs_f64(),
                chunk.end.as_secs_f64(),
                chunk.total.as_secs_f64(),
            ),
        });
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
        import.advance();
        let _sent = events.send(Event::FileProgress {
            generation,
            message: "Preparing the next audio segment. Cancel or Escape stops transcription."
                .to_owned(),
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
    use std::{io, sync::mpsc, time::Duration};

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
        let mut import =
            Session::fixture(vec![Ok(Some(chunk(1, false))), Ok(Some(chunk(2, true)))])?;
        let current = AtomicU64::new(7);
        let (events, incoming) = mpsc::channel();
        let mut seen = Vec::new();
        let result = run(
            7,
            chunk(0, false),
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
    fn audio_file_stream_cancellation_drops_late_recognition() -> io::Result<()> {
        let mut import = Session::fixture(vec![])?;
        let current = AtomicU64::new(7);
        let (events, incoming) = mpsc::channel();
        let result = run(
            7,
            chunk(0, true),
            &mut import,
            &current,
            &events,
            &mut |_| {
                current.store(8, Ordering::Release);
                Ok("Late text".to_owned())
            },
            &mut |_| Ok(()),
        );
        assert_eq!(result, Err(canary::Failure::Cancelled));
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
        let mut import = Session::fixture(vec![Err(error)])?;
        let current = AtomicU64::new(7);
        let (events, incoming) = mpsc::channel();
        let result = run(
            7,
            chunk(0, false),
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
