//! H.264 encoding behind one [`Encoder`] shape, with the backend picked at
//! run time (#360):
//!
//! 1. `v4l2m2m` — any V4L2 stateful mem2mem encoder through FFmpeg
//!    (`h264_v4l2m2m`): Raspberry Pi, Rockchip, and Asahi's AVE once a
//!    driver ships. Never auto-selected unless a device accepts the session;
//!    skipped without asking FFmpeg when no `/dev/video*` node is one.
//! 2. `x264` — libx264 through FFmpeg, `tune=zerolatency`. The software
//!    default: several times faster than OpenH264 on the same cores.
//! 3. `openh264` — compiled in, so it works even where the system FFmpeg was
//!    built without libx264 (Fedora's `ffmpeg-free`).
//!
//! VA-API (Intel/AMD) slots in as another FFmpeg backend ahead of x264.

#[cfg(target_os = "linux")]
mod convert;
#[cfg(target_os = "linux")]
mod ffmpeg;
#[cfg(not(target_os = "linux"))]
mod nostage;
mod openh264;
#[cfg(target_os = "linux")]
mod stage;
#[cfg(target_os = "linux")]
mod v4l2;

#[cfg(not(target_os = "linux"))]
pub use nostage::{ConvertStage, YuvFrame};
#[cfg(target_os = "linux")]
pub use stage::{ConvertStage, MIN_STAGE_WIDTH, YuvFrame};

use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use clap::ValueEnum;
#[cfg(target_os = "linux")]
use ffmpeg_next as ff;
use opendisplay_proto::video::AccessUnit;
use tracing::{debug, info};

use crate::backend::{Frame, FrameBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Backend {
    /// First that opens: v4l2m2m, x264, openh264.
    Auto,
    V4l2m2m,
    X264,
    Openh264,
}

/// `false` when no `/dev/video*` node is a V4L2 H.264 encoder, so
/// [`Backend::V4l2m2m`] cannot open; `true` when one is, or when a node
/// could not be asked.
#[cfg(target_os = "linux")]
pub fn v4l2m2m_present() -> bool {
    v4l2::h264_encoder() != v4l2::Presence::Absent
}

#[cfg(not(target_os = "linux"))]
pub fn v4l2m2m_present() -> bool {
    false
}

impl Backend {
    fn label(self) -> &'static str {
        match self {
            Backend::Auto => "auto",
            Backend::V4l2m2m => "V4L2 M2M (hardware)",
            Backend::X264 => "x264",
            Backend::Openh264 => "OpenH264",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EncoderSettings {
    pub backend: Backend,
    pub bitrate_bps: u32,
    pub max_fps: f32,
    /// 0 = let the backend decide.
    pub threads: u16,
}

/// One encoded picture ready for the wire.
pub struct Encoded {
    pub unit: AccessUnit,
    pub captured_at: Instant,
    pub encode_ms: f64,
}

enum Imp {
    #[cfg(target_os = "linux")]
    Ffmpeg(ffmpeg::FfmpegEncoder),
    OpenH264(Box<openh264::OpenH264Encoder>),
}

pub struct Encoder {
    imp: Imp,
    backend: Backend,
    width: u32,
    height: u32,
}

impl Encoder {
    /// Open the requested backend, or the first working one for
    /// [`Backend::Auto`]. I420 needs even dimensions; an odd panel loses a
    /// row/column.
    pub fn new(width: u32, height: u32, s: EncoderSettings) -> Result<Self> {
        let (width, height) = (width & !1, height & !1);
        let order: &[Backend] = match s.backend {
            #[cfg(target_os = "linux")]
            Backend::Auto => &[Backend::V4l2m2m, Backend::X264, Backend::Openh264],
            #[cfg(not(target_os = "linux"))]
            Backend::Auto => &[Backend::Openh264],
            Backend::V4l2m2m => &[Backend::V4l2m2m],
            Backend::X264 => &[Backend::X264],
            Backend::Openh264 => &[Backend::Openh264],
        };
        let mut errors = Vec::new();
        for &b in order {
            let imp = match b {
                Backend::V4l2m2m if !v4l2m2m_present() => {
                    debug!("encoder {}: no V4L2 H.264 encoder device", b.label());
                    if s.backend == Backend::Auto {
                        continue;
                    }
                    Err(anyhow!("no V4L2 H.264 encoder device"))
                }
                #[cfg(target_os = "linux")]
                Backend::V4l2m2m => {
                    ffmpeg::FfmpegEncoder::new(ffmpeg::Kind::V4l2m2m, width, height, s)
                        .map(Imp::Ffmpeg)
                }
                #[cfg(target_os = "linux")]
                Backend::X264 => ffmpeg::FfmpegEncoder::new(ffmpeg::Kind::X264, width, height, s)
                    .map(Imp::Ffmpeg),
                #[cfg(not(target_os = "linux"))]
                Backend::V4l2m2m | Backend::X264 => {
                    Err(anyhow!("{} needs FFmpeg, which this platform build does not include", b.label()))
                }
                Backend::Openh264 => openh264::OpenH264Encoder::new(width, height, s)
                    .map(|e| Imp::OpenH264(Box::new(e))),
                Backend::Auto => unreachable!(),
            };
            match imp {
                Ok(imp) => {
                    if !errors.is_empty() {
                        info!("encoder: using {} ({})", b.label(), errors.join("; "));
                    }
                    return Ok(Self {
                        imp,
                        backend: b,
                        width,
                        height,
                    });
                }
                Err(e) => {
                    debug!("encoder {} unavailable: {e:#}", b.label());
                    errors.push(format!("{} unavailable: {e:#}", b.label()));
                }
            }
        }
        bail!("no H.264 encoder could be opened: {}", errors.join("; "))
    }

    /// The backend that actually opened; pass it back in [`EncoderSettings`]
    /// so a resize does not probe again.
    pub fn backend(&self) -> Backend {
        self.backend
    }

    pub fn label(&self) -> &'static str {
        self.backend.label()
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The 4:2:0 format this encoder takes pre-converted frames in
    /// ([`FrameBuf::Yuv`] from a [`ConvertStage`]), if any.
    #[cfg(target_os = "linux")]
    pub fn yuv_format(&self) -> Option<ff::format::Pixel> {
        match &self.imp {
            Imp::Ffmpeg(e) => Some(e.format()),
            // OpenH264 converts with its own coefficients and announces no
            // BT.709 colour; its frames stay unconverted.
            Imp::OpenH264(_) => None,
        }
    }

    /// Encode one frame: 4-byte RGB pixels in the capture's byte order
    /// ([`crate::backend::PixelFormat`]), or already converted by a
    /// [`ConvertStage`]; a dmabuf frame is an error until a VA-API backend
    /// exists. Returns the pictures the encoder produced, in order: usually
    /// one, none when it skipped the frame or holds it, two when a lagging
    /// encoder catches up.
    pub fn encode(&mut self, frame: &Frame, force_idr: bool) -> Result<Vec<Encoded>> {
        match (&frame.buf, &mut self.imp) {
            #[cfg(target_os = "linux")]
            (
                FrameBuf::Cpu {
                    pixels,
                    stride,
                    format,
                    ..
                },
                Imp::Ffmpeg(e),
            ) => e.encode(pixels, *stride, *format, force_idr, frame.captured_at),
            (
                FrameBuf::Cpu {
                    pixels,
                    stride,
                    format,
                    ..
                },
                Imp::OpenH264(e),
            ) => e.encode(pixels, *stride, *format, force_idr, frame.captured_at),
            #[cfg(target_os = "linux")]
            (FrameBuf::Yuv(y), Imp::Ffmpeg(e)) => e.encode_yuv(y, force_idr, frame.captured_at),
            (FrameBuf::Yuv(_), _) => bail!("{} takes RGB frames only", self.label()),
            (FrameBuf::Dmabuf { .. }, _) => bail!("{} cannot encode dmabuf frames", self.label()),
        }
    }
}
