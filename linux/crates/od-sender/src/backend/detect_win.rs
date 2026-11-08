//! Non-Linux stand-in for `detect.rs`. On Windows it builds the monitor-adopting
//! backend (design: windows/DESIGN-sender.md); elsewhere it refuses.

use anyhow::Result;

use super::capture::CaptureChoice;
use super::{Backend, BackendId};

pub const ISSUE: &str = "https://github.com/ngiangbao/opendisplay-windows/blob/main/windows/DESIGN-sender.md";

/// What the user asked for on the command line.
#[derive(Debug, Clone, Default)]
pub struct Request {
    pub backend: Option<BackendId>,
    pub output: Option<String>,
    pub capture: CaptureChoice,
}

/// Exit code for an unsupported desktop.
pub const EXIT_UNSUPPORTED: i32 = 2;

/// An unsupported desktop, carried as an error so `main` can exit with
/// [`EXIT_UNSUPPORTED`].
#[derive(Debug)]
pub struct Unsupported(pub String);

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Unsupported {}

/// Windows: adopt a monitor (`--output \.\DISPLAY2`, else the first non-primary).
#[cfg(windows)]
pub fn detect(req: &Request) -> Result<Box<dyn Backend>> {
    if let Some(b) = req.backend {
        if b != BackendId::Adopt {
            return Err(Unsupported(format!(
                "backend {b} does not exist on Windows; only the monitor-adopting backend does"
            ))
            .into());
        }
    }
    Ok(Box::new(super::win::WindowsBackend::new(req.output.clone())))
}

#[cfg(not(windows))]
pub fn detect(_req: &Request) -> Result<Box<dyn Backend>> {
    Err(Unsupported(format!("no display backend for this platform; see {ISSUE}")).into())
}
