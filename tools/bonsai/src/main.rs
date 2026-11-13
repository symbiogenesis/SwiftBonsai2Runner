//! Console entry point for packaged Bonsai maintenance tools.
#![forbid(unsafe_code)]

mod entry;

fn main() -> std::process::ExitCode {
    entry::run(false)
}
