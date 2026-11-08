//! Windows backend (design: windows/DESIGN-sender.md, option D): adopt an
//! existing monitor. Frames come from DXGI Desktop Duplication, pointer input
//! goes back through `SendInput`. Nothing is created or removed, so the stream
//! has the monitor's own size, like Linux `--output` (adopt) mode.
//!
//! Pick the monitor with `--output \\.\DISPLAY2`; without it the first
//! non-primary monitor is used, else the primary (which mirrors your main
//! screen). `od-sender doctor` lists them.

mod capture;
mod cursor;
mod dpi;
mod input;
pub mod monitors;
pub mod vdd;

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
                .find(|m| m.name.eq_ignore_ascii_case(want) || m.name.to_ascii_uppercase().ends_with(&want.to_ascii_uppercase()))
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
            resize_in_place: vdd::installed(),
            creates_outputs: vdd::installed(),
        }
    }

    fn set_capture(&mut self, _path: CapturePath) {}

    fn wayland_capture(&self) -> bool {
        false
    }

    fn open(&mut self, spec: &DisplaySpec) -> Result<Box<dyn VirtualDisplay>> {
        let (m, virtual_) = self.choose_for(spec)?;
        let lock = lock_output("windows-sender")?;
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
            virtual_,
            capture: None,
            _lock: lock,
        }))
    }

    fn clean(&mut self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn lock_name(&self, _: &DisplaySpec) -> String {
        // One sender per Windows machine: two would fight over input and capture.
        "windows-sender".into()
    }
}

struct WindowsDisplay {
    monitor: Monitor,
    info: DisplayInfo,
    /// A Virtual Display Driver monitor we sized to the receiver.
    virtual_: bool,
    capture: Option<capture::Capture>,
    _lock: super::lock::OutputLock,
}

impl VirtualDisplay for WindowsDisplay {
    fn info(&self) -> &DisplayInfo {
        &self.info
    }

    fn start_capture(&mut self, opts: CaptureOptions, sink: FrameSink) -> Result<()> {
        self.stop_capture();
        self.capture = Some(capture::Capture::start(self.monitor.clone(), sink, opts.cursor)?);
        Ok(())
    }

    fn stop_capture(&mut self) {
        if let Some(c) = self.capture.take() {
            c.stop();
        }
    }

    fn reconfigure(&mut self, spec: &DisplaySpec) -> Result<()> {
        self.stop_capture();
        if self.virtual_ {
            // Rotation or a new receiver: resize our virtual monitor.
            self.monitor = prepare_virtual(spec)?;
            self.info = self.monitor.display_info();
            return Ok(());
        }
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

    fn close(mut self: Box<Self>, keep_output: bool) -> Result<()> {
        self.stop_capture();
        if self.virtual_ && !keep_output {
            // Do not leave an iPad-sized phantom screen on the desktop; the
            // next session attaches it again.
            match monitors::detach(&self.monitor.name) {
                Ok(()) => info!("detached the virtual display {}", self.monitor.name),
                Err(e) => info!("could not detach the virtual display: {e:#}"),
            }
        }
        Ok(())
    }
}

impl Drop for WindowsDisplay {
    fn drop(&mut self) {
        self.stop_capture();
    }
}

impl WindowsBackend {
    /// The monitor to stream and whether it is a virtual one we size to the
    /// receiver. `--output` always adopts that exact monitor (mirror or a real
    /// second screen). Otherwise: the Virtual Display Driver if installed
    /// (extended screen at the receiver's own resolution), else a real second
    /// screen, else an explanatory error instead of silently mirroring.
    fn choose_for(&self, spec: &DisplaySpec) -> Result<(Monitor, bool)> {
        if self.output.is_some() {
            return Ok((self.choose()?, false));
        }
        if vdd::installed() {
            return Ok((prepare_virtual(spec)?, true));
        }
        let all = monitors::list()?;
        if let Some(m) = all.iter().find(|m| !m.primary) {
            return Ok((m.clone(), false));
        }
        bail!("{}", vdd::not_installed_message())
    }
}

/// Make the Virtual Display Driver's monitor exactly the receiver's size and
/// park it to the right of the other screens.
fn prepare_virtual(spec: &DisplaySpec) -> Result<Monitor> {
    use std::time::{Duration, Instant};
    let (w, h) = (spec.width & !1, spec.height & !1);
    let hz = vdd::pick_refresh(spec.hz);
    let find = || -> Result<Option<Monitor>> {
        Ok(monitors::list()?
            .into_iter()
            .find(|m| monitors::is_virtual(&m.name)))
    };
    if vdd::ensure_resolution(w, h)? {
        info!("added {w}x{h} to the virtual display driver; reloading it");
        vdd::reload()?;
        std::thread::sleep(Duration::from_millis(1500));
    }
    let right_edge = || -> i32 {
        monitors::list()
            .unwrap_or_default()
            .iter()
            .filter(|o| !monitors::is_virtual(&o.name))
            .map(|o| o.x + o.width as i32)
            .max()
            .unwrap_or(0)
    };
    // The previous session detached the virtual screen: attach it again.
    if find()?.is_none() {
        if let Some(name) = monitors::detached_virtual() {
            info!("re-attaching the virtual display {name}");
            monitors::set_mode(&name, w, h, hz, Some((right_edge(), 0)))?;
            std::thread::sleep(Duration::from_millis(800));
        }
    }
    let deadline = Instant::now() + Duration::from_secs(12);
    let m = loop {
        match find()? {
            Some(m) => break m,
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(300)),
            None => bail!(
                "the virtual display did not appear; open Settings > Display and check it is enabled"
            ),
        }
    };
    if (m.width, m.height) != (w, h) || m.hz.round() as u32 != hz {
        monitors::set_mode(&m.name, w, h, hz, Some((right_edge(), 0)))?;
        std::thread::sleep(Duration::from_millis(800));
    }
    // Match the receiver's own scale (an iPad is 2x) so the desktop is the
    // size the device would show natively; best effort.
    let want = (spec.scale * 100.0).round() as u32;
    match dpi::set(&m.name, want) {
        Ok(got) => info!("display scale of {}: {got}%", m.name),
        Err(e) => info!("could not set the display scale ({e:#}); leaving it as Windows chose"),
    }
    find()?.ok_or_else(|| anyhow::anyhow!("the virtual display disappeared after the mode change"))
}
