//! DXGI Desktop Duplication capture of one monitor on its own thread.
//!
//! Frames are copied to a CPU staging texture and sent as `Xrgb8888` (B, G, R,
//! X in memory, which is what the duplication surface holds). Desktop
//! Duplication only delivers a frame when the screen changes, so the last
//! picture is re-sent about once a second: it keeps the stream alive, lets a
//! keyframe request be served, and shows a late-joining receiver something.
//!
//! The mouse cursor is not part of the duplicated surface; when the session
//! asks for it (`CaptureOptions::cursor`) it is painted into each frame by
//! [`super::cursor`], and a pointer-only update re-sends the last picture.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use tracing::{debug, info, warn};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
    IDXGIAdapter, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
};
use windows::core::Interface;

use super::cursor::CursorPainter;
use super::monitors::Monitor;
use crate::backend::{CaptureEvent, Frame, FrameBuf, FrameSink, PixelFormat};

pub struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    /// Open the duplication on the calling thread (so errors come back here),
    /// then run the frame loop on a new thread.
    pub fn start(monitor: Monitor, sink: FrameSink, paint_cursor: bool) -> Result<Self> {
        let dup = Duplication::open(&monitor)?;
        info!(
            "capturing {} {}x{} with Desktop Duplication",
            monitor.name, dup.width, dup.height
        );
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let thread = std::thread::Builder::new()
            .name("capture".into())
            .spawn(move || run(monitor, dup, sink, stop2, paint_cursor))
            .context("spawning the capture thread")?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// D3D11/DXGI interfaces are free-threaded; the wrapper only moves them to
/// the capture thread, after which one thread uses them.
struct Duplication {
    ctx: ID3D11DeviceContext,
    device: ID3D11Device,
    dup: IDXGIOutputDuplication,
    staging: Option<ID3D11Texture2D>,
    width: u32,
    height: u32,
}

// SAFETY: see the type comment: ownership moves to exactly one thread.
unsafe impl Send for Duplication {}

impl Duplication {
    fn open(m: &Monitor) -> Result<Self> {
        // SAFETY: COM calls on objects created here; out-pointers are locals.
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1().context("CreateDXGIFactory1")?;
            let adapter = factory
                .EnumAdapters1(m.adapter)
                .context("the monitor's graphics adapter is gone")?;
            let output = adapter
                .EnumOutputs(m.output)
                .context("the monitor is gone")?;
            let mut device = None;
            let mut ctx = None;
            let base: IDXGIAdapter = adapter.cast()?;
            // The device must live on the adapter that owns the output, or
            // DuplicateOutput fails on hybrid-GPU laptops.
            D3D11CreateDevice(
                &base,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            )
            .context("D3D11CreateDevice")?;
            let (device, ctx) = (device.context("no D3D11 device")?, ctx.context("no context")?);
            let output1: IDXGIOutput1 = output.cast().context("IDXGIOutput1")?;
            let dup = output1.DuplicateOutput(&device).map_err(|e| {
                anyhow::anyhow!(
                    "DuplicateOutput failed ({e}); another app may already duplicate this monitor, \
                     or the session is locked / on a secure desktop"
                )
            })?;
            let d = dup.GetDesc();
            let (width, height) = (d.ModeDesc.Width, d.ModeDesc.Height);
            if width == 0 || height == 0 {
                bail!("duplication reports an empty surface");
            }
            Ok(Self {
                ctx,
                device,
                dup,
                staging: None,
                width,
                height,
            })
        }
    }

    /// Copy the acquired surface to CPU memory, tightly packed rows of
    /// `width * 4` bytes.
    fn read(&mut self, res: &IDXGIResource) -> Result<Vec<u8>> {
        // SAFETY: the texture, staging texture and context belong to this
        // object; the mapped pointer is only read while mapped.
        unsafe {
            let tex: ID3D11Texture2D = res.cast().context("surface is not a texture")?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            tex.GetDesc(&mut desc);
            if self.staging.is_none() {
                let mut sd = desc;
                sd.Usage = D3D11_USAGE_STAGING;
                sd.BindFlags = 0;
                sd.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
                sd.MiscFlags = 0;
                let mut t = None;
                self.device
                    .CreateTexture2D(&sd, None, Some(&mut t))
                    .context("CreateTexture2D(staging)")?;
                self.staging = t;
            }
            let staging = self.staging.as_ref().unwrap();
            self.ctx.CopyResource(staging, &tex);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx
                .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .context("Map(staging)")?;
            let (w, h) = (desc.Width as usize, desc.Height as usize);
            let mut out = vec![0u8; w * h * 4];
            let src = mapped.pData as *const u8;
            for y in 0..h {
                let row = std::slice::from_raw_parts(src.add(y * mapped.RowPitch as usize), w * 4);
                out[y * w * 4..(y + 1) * w * 4].copy_from_slice(row);
            }
            self.ctx.Unmap(staging, 0);
            // Desktop Duplication surfaces are opaque; make alpha 255 so
            // converters that read it behave.
            Ok(out)
        }
    }
}

fn run(
    monitor: Monitor,
    mut dup: Duplication,
    mut sink: FrameSink,
    stop: Arc<AtomicBool>,
    paint_cursor: bool,
) {
    let mut painter = CursorPainter::default();
    let mut last: Option<Vec<u8>> = None;
    let mut last_sent = Instant::now();
    let mut lost_since: Option<Instant> = None;
    let mut send = |sink: &mut FrameSink, mut pixels: Vec<u8>, w: u32, h: u32| {
        if paint_cursor {
            painter.paint(&mut pixels, w as usize, h as usize, monitor.x, monitor.y);
        }
        sink(CaptureEvent::Frame(Frame {
            captured_at: Instant::now(),
            presentation_ns: None,
            buf: FrameBuf::Cpu {
                pixels: pixels.into(),
                width: w,
                height: h,
                stride: w * 4,
                format: PixelFormat::Xrgb8888,
            },
        }));
    };
    while !stop.load(Ordering::Relaxed) {
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut res: Option<IDXGIResource> = None;
        // SAFETY: out-pointers are locals; ReleaseFrame follows every success.
        let r = unsafe { dup.dup.AcquireNextFrame(100, &mut info, &mut res) };
        match r {
            Ok(()) => {
                lost_since = None;
                let mouse_only = paint_cursor && info.LastPresentTime == 0 && info.LastMouseUpdateTime != 0;
                let frame = match res.as_ref() {
                    Some(res) if info.LastPresentTime != 0 => Some(dup.read(res)),
                    _ => None,
                };
                // SAFETY: a frame was acquired above.
                let _ = unsafe { dup.dup.ReleaseFrame() };
                match frame {
                    Some(Ok(px)) => {
                        last = Some(px.clone());
                        last_sent = Instant::now();
                        send(&mut sink, px, dup.width, dup.height);
                    }
                    Some(Err(e)) => {
                        sink(CaptureEvent::Ended(format!("reading the frame: {e:#}")));
                        return;
                    }
                    None if mouse_only => {
                        if let Some(px) = &last {
                            last_sent = Instant::now();
                            send(&mut sink, px.clone(), dup.width, dup.height);
                        }
                    }
                    None => {}
                }
            }
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                if last_sent.elapsed() >= Duration::from_secs(1) {
                    if let Some(px) = &last {
                        last_sent = Instant::now();
                        send(&mut sink, px.clone(), dup.width, dup.height);
                    }
                }
            }
            Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                // Mode change, UAC / lock screen, or another duplication
                // took over: re-open, retrying for a while.
                let since = *lost_since.get_or_insert_with(Instant::now);
                match Duplication::open(&monitor) {
                    Ok(d) => {
                        debug!("duplication re-opened");
                        dup = d;
                        lost_since = None;
                    }
                    Err(err) if since.elapsed() > Duration::from_secs(30) => {
                        sink(CaptureEvent::Ended(format!(
                            "lost the monitor and could not re-open it: {err:#}"
                        )));
                        return;
                    }
                    Err(err) => {
                        debug!("re-open failed, retrying: {err:#}");
                        std::thread::sleep(Duration::from_millis(250));
                    }
                }
            }
            Err(e) => {
                warn!("AcquireNextFrame: {e}");
                sink(CaptureEvent::Ended(format!("Desktop Duplication: {e}")));
                return;
            }
        }
    }
}
