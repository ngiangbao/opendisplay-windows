//! `opendisplay.exe`: the receiver and the sender in one executable.
//!
//! ```text
//! opendisplay                      receiver in a window (what the Start menu runs)
//! opendisplay receive [options]    this PC is the display (--help for options)
//! opendisplay send [options]       extend this PC onto a receiver (--help)
//! ```
#![cfg_attr(not(windows), allow(dead_code))]
// GUI subsystem: double-clicking shows no console window. CLI use re-attaches to
// the parent terminal (see `attach_console`).
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod gui;

#[cfg(not(windows))]
fn main() {
    eprintln!("opendisplay is the Windows build; use od-receiver / od-sender on Linux.");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() {
    use std::ffi::OsString;

    let mut args = std::env::args_os();
    let _program = args.next();
    let rest: Vec<OsString> = args.collect();
    let first = rest.first().and_then(|a| a.to_str()).unwrap_or("");
    if first.is_empty() {
        // Double-click / Start menu: the control panel.
        gui::run();
        return;
    }
    attach_console();
    od_sender::init_tracing();

    let code = match first {
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

/// A GUI-subsystem exe has no console. When started from cmd/PowerShell with
/// arguments, attach to that terminal so `--help`, logs and `doctor` print.
/// Skipped when stdout/stderr are already valid (a pipe from the control panel).
#[cfg(windows)]
fn attach_console() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
        SetStdHandle,
    };
    // SAFETY: plain console API calls; the leaked File keeps the handle valid.
    unsafe {
        let err = GetStdHandle(STD_ERROR_HANDLE);
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        let valid = |h: windows_sys::Win32::Foundation::HANDLE| !h.is_null() && h as isize != -1;
        if valid(err) && valid(out) {
            return;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
            let h = f.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
            if !valid(out) {
                SetStdHandle(STD_OUTPUT_HANDLE, h);
            }
            if !valid(err) {
                SetStdHandle(STD_ERROR_HANDLE, h);
            }
            std::mem::forget(f);
        }
    }
}
