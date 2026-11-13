//! Windowless entry point for the native application distribution.
#![forbid(unsafe_code)]
#![cfg_attr(windows, windows_subsystem = "windows")]

mod entry;

fn main() -> std::process::ExitCode {
    entry::run(true)
}
