//! Inert release command fixture with exact Unicode streams and native exit codes.
#![forbid(unsafe_code)]
use std::{io::Write as _, process::ExitCode};
fn main() -> ExitCode {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let (stdout, stderr, code) = match mode.as_str() {
        "success" => (
            "exact-\u{03b2}\nsecond line\n".into(),
            "success diagnostic-\u{03b2}\n".into(),
            0,
        ),
        "failure" => (
            "Native diagnostic-\u{03b2}\n".into(),
            "stderr diagnostic-\u{03b2}\n".into(),
            17,
        ),
        "empty" => (String::new(), String::new(), 19),
        "large" => (
            format!("stdout-head{}stdout-tail", "x".repeat(12000)),
            format!("stderr-head{}stderr-tail", "y".repeat(12000)),
            23,
        ),
        _ => (String::new(), "Unknown probe mode".into(), 1),
    };
    if std::io::stdout()
        .write_all(stdout.as_bytes())
        .and_then(|()| std::io::stderr().write_all(stderr.as_bytes()))
        .is_err()
    {
        return ExitCode::FAILURE;
    }
    ExitCode::from(code)
}
