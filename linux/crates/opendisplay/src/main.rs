//! `opendisplay.exe`: the receiver and the sender in one executable.
//!
//! ```text
//! opendisplay                      receiver in a window (what the Start menu runs)
//! opendisplay receive [options]    this PC is the display (--help for options)
//! opendisplay send [options]       extend this PC onto a receiver (--help)
//! ```
#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(not(windows))]
fn main() {
    eprintln!("opendisplay is the Windows build; use od-receiver / od-sender on Linux.");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() {
    use std::ffi::OsString;

    od_sender::init_tracing();
    let mut args = std::env::args_os();
    let _program = args.next();
    let rest: Vec<OsString> = args.collect();
    let first = rest.first().and_then(|a| a.to_str()).unwrap_or("");

    let code = match first {
        "" => receive(vec!["--windowed".into()]),
        "receive" | "recv" | "display" => receive(rest[1..].to_vec()),
        "send" | "extend" => send(rest[1..].to_vec()),
        "-h" | "--help" | "help" => {
            print!("{HELP}");
            0
        }
        "-V" | "--version" | "version" => {
            println!("opendisplay {}", env!("CARGO_PKG_VERSION"));
            0
        }
        // `opendisplay --windowed`, `opendisplay --listen ...`: receiver options.
        a if a.starts_with('-') => receive(rest),
        other => {
            eprintln!("unknown command `{other}`\n\n{HELP}");
            2
        }
    };
    std::process::exit(code);
}

#[cfg(windows)]
const HELP: &str = "\
OpenDisplay: use another screen as a monitor, in one app.

USAGE:
    opendisplay                       Receiver in a window (this PC becomes a display)
    opendisplay receive [OPTIONS]     Same, with options (see `opendisplay receive --help`)
    opendisplay send [OPTIONS]        Extend this PC onto a receiver (`opendisplay send --help`)
    opendisplay --version

The sender needs a display backend that is still being written; `opendisplay send doctor`
shows its status.
";

#[cfg(windows)]
fn receive(args: Vec<std::ffi::OsString>) -> i32 {
    let argv = std::iter::once(std::ffi::OsString::from("opendisplay receive")).chain(args);
    match od_receiver::cli::run_cli(argv) {
        Ok(()) => 0,
        Err(e) => {
            tracing::error!("{e:#}");
            1
        }
    }
}

#[cfg(windows)]
fn send(args: Vec<std::ffi::OsString>) -> i32 {
    let argv = std::iter::once(std::ffi::OsString::from("opendisplay send")).chain(args);
    od_sender::run_cli(argv)
}
