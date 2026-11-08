//! Windows (and any non-Wayland desktop) presenter: OpenH264 software decode
//! into a winit window drawn with softbuffer. H.264 only, which is all the
//! receiver advertises (`hello.videoCaps`). Pointer input is mapped to touch
//! and scroll like the GTK window (`input.rs`).
//!
//! The window owns the main thread (winit requirement); the receiver loop
//! runs on a worker thread and pushes decoded frames through [`SoftVideo`].

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use opendisplay_proto::control::TouchPhase;
use openh264::decoder::Decoder;
use openh264::formats::YUVSource;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, warn};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Fullscreen, Window, WindowId};

use crate::input::{ScrollUnit, UserInput, VideoRect, scroll_to_video};
use crate::video::VideoOutput;

/// Latest decoded picture, `0x00RRGGBB` as softbuffer wants.
struct Frame {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
}

#[derive(Default)]
struct Shared {
    frame: Mutex<Option<Frame>>,
    rendered: AtomicU64,
}

/// The `VideoOutput` half, used from the receiver thread.
pub struct SoftVideo {
    decoder: Decoder,
    shared: Arc<Shared>,
    proxy: EventLoopProxy<()>,
    errors: Vec<String>,
    decode_ms: Vec<f64>,
    rgb: Vec<u8>,
}

impl SoftVideo {
    fn new_decoder() -> Result<Decoder> {
        Decoder::new().context("creating the OpenH264 decoder")
    }

    /// Handle the window thread keeps to read decoded frames.
    pub fn shared_handle(&self) -> Presenter {
        Presenter(self.shared.clone())
    }
}

impl VideoOutput for SoftVideo {
    fn push(&mut self, annexb: &[u8], _is_idr: bool) -> Result<()> {
        let t0 = Instant::now();
        match self.decoder.decode(annexb) {
            Ok(Some(yuv)) => {
                let (w, h) = yuv.dimensions();
                self.rgb.resize(w * h * 3, 0);
                yuv.write_rgb8(&mut self.rgb);
                let mut pixels = Vec::with_capacity(w * h);
                pixels.extend(
                    self.rgb
                        .chunks_exact(3)
                        .map(|p| (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32),
                );
                *self.shared.frame.lock().unwrap() = Some(Frame {
                    width: w as u32,
                    height: h as u32,
                    pixels,
                });
                self.shared.rendered.fetch_add(1, Ordering::Relaxed);
                self.decode_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
                let _ = self.proxy.send_event(());
            }
            Ok(None) => {}
            Err(e) => self.errors.push(format!("decode: {e}")),
        }
        Ok(())
    }
    fn reset(&mut self) -> Result<()> {
        self.decoder = Self::new_decoder()?;
        Ok(())
    }
    fn poll_error(&mut self) -> Option<String> {
        if self.errors.is_empty() {
            None
        } else {
            Some(self.errors.remove(0))
        }
    }
    fn describe(&mut self) -> String {
        "openh264 (software) -> softbuffer window".into()
    }
    fn rendered(&self) -> Option<u64> {
        Some(self.shared.rendered.load(Ordering::Relaxed))
    }
    fn take_decode_ms(&mut self) -> Vec<f64> {
        std::mem::take(&mut self.decode_ms)
    }
}

/// Window-side handle on the decoded frames.
pub struct Presenter(Arc<Shared>);

pub struct WindowOptions {
    pub windowed: bool,
}

/// Monitor facts for `hello`: physical pixels, scale, refresh (Hz).
pub struct PrimaryMonitor {
    pub pixels: (u32, u32),
    pub scale: f64,
    pub refresh_hz: Option<u32>,
    pub name: String,
}

/// Create the event loop and the video half. Call on the main thread.
pub fn prepare() -> Result<(EventLoop<()>, SoftVideo)> {
    let event_loop = EventLoop::<()>::with_user_event()
        .build()
        .context("creating the event loop")?;
    let video = SoftVideo {
        decoder: SoftVideo::new_decoder()?,
        shared: Arc::new(Shared::default()),
        proxy: event_loop.create_proxy(),
        errors: Vec::new(),
        decode_ms: Vec::new(),
        rgb: Vec::new(),
    };
    Ok((event_loop, video))
}

/// The primary monitor, learned by pumping the loop once (winit only hands
/// out monitors from inside the loop). The loop is reusable afterwards.
pub fn primary_monitor(event_loop: &mut EventLoop<()>) -> Option<PrimaryMonitor> {
    use std::time::Duration;
    use winit::platform::pump_events::EventLoopExtPumpEvents;

    struct Probe(Option<PrimaryMonitor>);
    impl ApplicationHandler<()> for Probe {
        fn resumed(&mut self, el: &ActiveEventLoop) {
            let m = el.primary_monitor().or_else(|| el.available_monitors().next());
            self.0 = m.map(|m| {
                let size = m.size();
                PrimaryMonitor {
                    pixels: (size.width, size.height),
                    scale: m.scale_factor(),
                    refresh_hz: m
                        .refresh_rate_millihertz()
                        .map(|mhz| (mhz as f64 / 1000.0).round() as u32),
                    name: m.name().unwrap_or_default(),
                }
            });
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
    }
    let mut probe = Probe(None);
    for _ in 0..3 {
        let _ = event_loop.pump_app_events(Some(Duration::ZERO), &mut probe);
        if probe.0.is_some() {
            break;
        }
    }
    probe.0
}

/// Run the window until it closes. `on_close` fires once when it does.
pub fn run(
    event_loop: EventLoop<()>,
    presenter: Presenter,
    options: WindowOptions,
    input: UnboundedSender<UserInput>,
    on_close: impl FnOnce() + 'static,
) -> Result<()> {
    let mut app = App {
        shared: presenter.0,
        options,
        input,
        on_close: Some(Box::new(on_close)),
        window: None,
        surface: None,
        cursor: (0.0, 0.0),
        pressed: false,
        video_size: (0, 0),
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut app).context("window event loop")?;
    Ok(())
}

type Surface = softbuffer::Surface<Rc<Window>, Rc<Window>>;

struct App {
    shared: Arc<Shared>,
    options: WindowOptions,
    input: UnboundedSender<UserInput>,
    on_close: Option<Box<dyn FnOnce()>>,
    window: Option<Rc<Window>>,
    surface: Option<Surface>,
    cursor: (f64, f64),
    pressed: bool,
    video_size: (u32, u32),
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

impl App {
    fn rect(&self) -> Option<VideoRect> {
        let w = self.window.as_ref()?;
        let s = w.inner_size();
        VideoRect::fit(
            (s.width as f64, s.height as f64),
            (self.video_size.0 as f64, self.video_size.1 as f64),
        )
    }

    fn touch(&self, phase: TouchPhase) {
        let Some(rect) = self.rect() else { return };
        let (x, y) = rect.normalise(self.cursor.0, self.cursor.1);
        let _ = self.input.send(UserInput::Touch {
            phase,
            x,
            y,
            local_ms: now_ms(),
        });
    }

    fn redraw(&mut self) {
        let (Some(window), Some(surface)) = (self.window.as_ref(), self.surface.as_mut()) else {
            return;
        };
        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        if let Err(e) = surface.resize(w, h) {
            warn!("surface resize: {e}");
            return;
        }
        let Ok(mut buf) = surface.buffer_mut() else {
            return;
        };
        buf.fill(0);
        if let Some(f) = self.shared.frame.lock().unwrap().as_ref() {
            self.video_size = (f.width, f.height);
            if let Some(r) = VideoRect::fit(
                (size.width as f64, size.height as f64),
                (f.width as f64, f.height as f64),
            ) {
                // Nearest-neighbour scale of the picture into its letterbox.
                let (dw, dh) = (r.width.round() as u32, r.height.round() as u32);
                let (ox, oy) = (r.x.round() as u32, r.y.round() as u32);
                for dy in 0..dh.min(size.height.saturating_sub(oy)) {
                    let sy = (dy as u64 * f.height as u64 / dh as u64) as usize;
                    let row = &f.pixels[sy * f.width as usize..(sy + 1) * f.width as usize];
                    let base = ((oy + dy) * size.width + ox) as usize;
                    for dx in 0..dw.min(size.width.saturating_sub(ox)) {
                        let sx = (dx as u64 * f.width as u64 / dw as u64) as usize;
                        buf[base + dx as usize] = row[sx];
                    }
                }
            }
        }
        if let Err(e) = buf.present() {
            debug!("present: {e}");
        }
    }
}

impl ApplicationHandler<()> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let mut attrs = Window::default_attributes().with_title("OpenDisplay Receiver");
        if !self.options.windowed {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let window = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                warn!("creating the window: {e}");
                el.exit();
                return;
            }
        };
        let ctx = match softbuffer::Context::new(window.clone()) {
            Ok(c) => c,
            Err(e) => {
                warn!("softbuffer context: {e}");
                el.exit();
                return;
            }
        };
        self.surface = softbuffer::Surface::new(&ctx, window.clone()).ok();
        window.request_redraw();
        self.window = Some(window);
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, _: ()) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                if let Some(f) = self.on_close.take() {
                    f();
                }
                el.exit();
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Resized(_) => {
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                if self.pressed {
                    self.touch(TouchPhase::Moved);
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.pressed = state == ElementState::Pressed;
                self.touch(if self.pressed {
                    TouchPhase::Began
                } else {
                    TouchPhase::Ended
                });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let Some(rect) = self.rect() else { return };
                // winit: positive y = wheel away from the user (content
                // down); `scroll_to_video` expects the GTK sign (positive =
                // content up), so negate.
                let (dx, dy, unit) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (-x as f64, -y as f64, ScrollUnit::Wheel),
                    MouseScrollDelta::PixelDelta(p) => (-p.x, -p.y, ScrollUnit::Surface),
                };
                let (dx, dy) = scroll_to_video(dx, dy, unit, &rect, self.video_size.1 as f64);
                let _ = self.input.send(UserInput::Scroll { dx, dy });
            }
            _ => {}
        }
    }
}
