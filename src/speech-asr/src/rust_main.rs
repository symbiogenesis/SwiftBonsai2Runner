//! Explicit Rust-only qualification service; never a production fallback.
#![forbid(unsafe_code)]

use bonsai_speech_asr::rust_engine::{ARITHMETIC_IDENTITY, RustRecognizer};
use bonsai_speech_service::{
    Engine, Kind, Output, Request,
    launch::{self, Command, ModelGuards},
};
use std::{
    io::{self, Write as _},
    process::ExitCode,
};

struct RustEngine {
    recognizer: RustRecognizer,
    _models: ModelGuards,
}

fn recognition_samples<'a>(request: &Request<'a>) -> io::Result<&'a [f32]> {
    match request {
        Request::Transcribe {
            samples,
            sample_rate: 16_000,
        } => Ok(samples),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unsupported Rust qualification recognition request.",
        )),
    }
}

impl Engine for RustEngine {
    type Waveform = Vec<f32>;

    fn kind(&self) -> Kind {
        Kind::Canary
    }

    fn infer(&mut self, request: Request<'_>) -> io::Result<Output> {
        let samples = recognition_samples(&request)?;
        self.recognizer
            .transcribe(samples)
            .map(Output::Transcript)
            .map_err(|_| io::Error::other("Rust qualification recognition failed."))
    }
}

fn version() -> serde_json::Value {
    let mut identity = launch::version(Kind::Canary);
    identity["Name"] = "bonsai-asr-rust-qualification".into();
    identity["Provider"] = "rust-qualification".into();
    identity["ArithmeticIdentity"] = ARITHMETIC_IDENTITY.into();
    identity["ProductionQualified"] = false.into();
    identity
}

fn run() -> io::Result<()> {
    launch::require_native_host()?;
    match launch::parse(Kind::Canary, std::env::args_os().skip(1))? {
        Command::Version => {
            serde_json::to_writer(io::stdout().lock(), &version())?;
            writeln!(io::stdout().lock())
        }
        Command::Serve(arguments) => {
            let guards = launch::verify_models(
                Kind::Canary,
                &arguments,
                include_str!("../model.lock.json"),
            )?;
            let recognizer = RustRecognizer::load(&arguments.model)
                .map_err(|_| io::Error::other("Rust qualification model loading failed."))?;
            bonsai_speech_service::serve_with_ready(
                RustEngine {
                    recognizer,
                    _models: guards,
                },
                arguments.port,
                &arguments.token_file,
                |port| {
                    writeln!(
                        io::stdout().lock(),
                        "Rust Qwen3-ASR qualification ready at 127.0.0.1:{port}"
                    )
                },
            )
        }
    }
}

fn main() -> ExitCode {
    std::panic::set_hook(Box::new(|_| {
        let _written = writeln!(
            io::stderr().lock(),
            "Rust ASR qualification service failed."
        );
    }));
    if run().is_ok() {
        ExitCode::SUCCESS
    } else {
        let _written = writeln!(
            io::stderr().lock(),
            "Rust ASR qualification service failed; check verified models and local configuration."
        );
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::{ARITHMETIC_IDENTITY, recognition_samples, version};
    use bonsai_speech_service::Request;
    use std::io;

    #[test]
    fn rust_asr_qualification_identity_cannot_match_production_name() {
        let identity = version();
        assert_eq!(identity["Name"], "bonsai-asr-rust-qualification");
        assert_eq!(identity["Provider"], "rust-qualification");
        assert_eq!(identity["ArithmeticIdentity"], ARITHMETIC_IDENTITY);
        assert_eq!(identity["ProductionQualified"], false);
    }

    #[test]
    fn rust_asr_qualification_adapter_accepts_only_selected_audio_contract() -> io::Result<()> {
        let pcm = [0.0, -1.0, 1.0];
        assert!(std::ptr::eq(
            recognition_samples(&Request::Transcribe {
                samples: &pcm,
                sample_rate: 16_000,
            })?
            .as_ptr(),
            pcm.as_ptr()
        ));
        assert!(
            recognition_samples(&Request::Transcribe {
                samples: &pcm,
                sample_rate: 48_000,
            })
            .is_err()
        );
        assert!(
            recognition_samples(&Request::Synthesize {
                text: "test",
                voice_description: "test",
                language: "Auto",
            })
            .is_err()
        );
        Ok(())
    }
}
