//! Windows backend (design: windows/DESIGN-sender.md, option D): adopt an
//! existing monitor. Frames come from DXGI Desktop Duplication, pointer input
//! goes back through `SendInput`. Nothing is created or removed, so the stream
//! has the monitor's own size, like Linux `--output` (adopt) mode.
//!
//! Pick the monitor with `--output \\.\DISPLAY2`; without it the first
//! non-primary monitor is used, else the primary (which mirrors your main
//! screen). `od-sender doctor` lists them.

mod capture;
mod input;
pub mod monitors;

use anyhow::{Result, bail};
use tracing::{info, warn};

use super::capture::CapturePath;
use super::lock::lock_output;
use super::{
    Backend, BackendId, Caps, CaptureOptions, DisplayInfo, DisplaySpec, FrameSink, InputSink,
    VirtualDisplay,
};
use monitors::Monitor;

pub struct WindowsBackend {
    output: Option<String>,
}

impl WindowsBackend {
    pub fn new(output: Option<String>) -> Self {
        monitors::make_dpi_aware();
        Self { output }
    }

    fn choose(&self) -> Result<Monitor> {
        let all = monitors::list()?;
        if all.is_empty() {
            bail!("no active monitors found");
        }
        if let Some(want) = &self.output {
            return all
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case(want))
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "no monitor named {want}; have: {}",
                        all.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", ")
                    )
                });
        }
        if let Some(m) = all.iter().find(|m| !m.primary) {
            return Ok(m.clone());
        }
        warn!(
            "only one monitor: streaming the primary {} (the receiver will mirror your main screen)",
            all[0].name
        );
        Ok(all[0].clone())
    }
}

/// `\\.\DISPLAY2` is not a legal file name; the lock key must be.
fn lock_key(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("win{}", clean.trim_matches('-'))
}

impl Backend for WindowsBackend {
    fn id(&self) -> BackendId {
        BackendId::Adopt
    }

    fn describe(&self) -> String {
        match &self.output {
            Some(o) => format!("windows monitor {o} (Desktop Duplication, adopt)"),
            None => "windows monitor (Desktop Duplication, adopt; first non-primary)".into(),
        }
    }

    fn caps(&self) -> Caps {
        Caps {
            cursor_metadata: false,
            dmabuf: false,
            resize_in_place: false,
            creates_outputs: false,
        }
    }

    fn set_capture(&mut self, _path: CapturePath) {}

    fn wayland_capture(&self) -> bool {
        false
    }

    fn open(&mut self, spec: &DisplaySpec) -> Result<Box<dyn VirtualDisplay>> {
        let m = self.choose()?;
        let lock = lock_output(&lock_key(&m.name))?;
        let info = m.display_info();
        if (spec.width, spec.height) != (info.width, info.height) {
            info!(
                "adopted monitor {} stays {}x{} (receiver asked for {}x{})",
                info.name, info.width, info.height, spec.width, spec.height
            );
        }
        info!("adopted display {info}");
        Ok(Box::new(WindowsDisplay {
            monitor: m,
            info,
            capture: None,
            _lock: lock,
        }))
    }

    fn clean(&mut self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn lock_name(&self, _: &DisplaySpec) -> String {
        match self.choose() {
            Ok(m) => lock_key(&m.name),
            Err(_) => "windows".into(),
        }
    }
}

struct WindowsDisplay {
    monitor: Monitor,
    info: DisplayInfo,
    capture: Option<capture::Capture>,
    _lock: super::lock::OutputLock,
}

impl VirtualDisplay for WindowsDisplay {
    fn info(&self) -> &DisplayInfo {
        &self.info
    }

    fn start_capture(&mut self, _opts: CaptureOptions, sink: FrameSink) -> Result<()> {
        self.stop_capture();
        self.capture = Some(capture::Capture::start(self.monitor.clone(), sink)?);
        Ok(())
    }

    fn stop_capture(&mut self) {
        if let Some(c) = self.capture.take() {
            c.stop();
        }
    }

    fn reconfigure(&mut self, spec: &DisplaySpec) -> Result<()> {
        self.stop_capture();
        // The monitor is not ours to resize; re-read it in case its owner did.
        if let Some(m) = monitors::list()?.into_iter().find(|m| m.name == self.monitor.name) {
            self.monitor = m;
            self.info = self.monitor.display_info();
        }
        if (spec.width, spec.height) != (self.info.width, self.info.height) {
            info!(
                "adopted monitor {} stays {}x{} (receiver asked for {}x{})",
                self.info.name, self.info.width, self.info.height, spec.width, spec.height
            );
        }
        Ok(())
    }

    fn input(&mut self) -> Result<Box<dyn InputSink>> {
        Ok(Box::new(input::WinInput::new(self.monitor.clone())))
    }

    fn close(mut self: Box<Self>, _keep_output: bool) -> Result<()> {
        self.stop_capture();
        Ok(())
    }
}

impl Drop for WindowsDisplay {
    fn drop(&mut self) {
        self.stop_capture();
    }
}
