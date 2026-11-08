//! Windows stand-in for `detect.rs`: no backend exists yet (T2.3 adds the
//! first one, see windows/DESIGN-sender.md), so every request is refused with
//! the "unsupported" exit code and a pointer to the task.

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

/// Always refuses until the Windows backend lands.
pub fn detect(_req: &Request) -> Result<Box<dyn Backend>> {
    Err(Unsupported(format!(
        "the Windows sender has no display backend yet (task T2.3); see {ISSUE}"
    ))
    .into())
}
