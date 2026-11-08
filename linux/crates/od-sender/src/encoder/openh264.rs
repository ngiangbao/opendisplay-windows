//! OpenH264, compiled in: the last-resort tier that works on every machine,
//! including ones whose FFmpeg lacks libx264.

use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use opendisplay_proto::video::access_units;
use openh264::OpenH264API;
use openh264::encoder::{
    BitRate, EncoderConfig, FrameRate, FrameType, IntraFramePeriod, RateControlMode,
    SpsPpsStrategy, UsageType,
};
use openh264::formats::{BgraSliceU8, RgbaSliceU8, YUVBuffer};

use super::{Encoded, EncoderSettings};
use crate::backend::PixelFormat;

/// OpenH264 refuses larger frames, at the first `encode` rather than at
/// creation; checking up front lets `auto` skip it cleanly.
const MAX_LONG: u32 = 3840;
const MAX_SHORT: u32 = 2160;

fn supports(width: u32, height: u32) -> bool {
    width.max(height) <= MAX_LONG && width.min(height) <= MAX_SHORT
}

pub struct OpenH264Encoder {
    inner: openh264::encoder::Encoder,
    width: u32,
    height: u32,
    packed: Vec<u8>,
}

impl OpenH264Encoder {
    /// `width`/`height` are already even.
    pub fn new(width: u32, height: u32, s: EncoderSettings) -> Result<Self> {
        if !supports(width, height) {
            bail!(
                "OpenH264 encodes at most {MAX_LONG}x{MAX_SHORT} \
                 ({MAX_SHORT}x{MAX_LONG} portrait), not {width}x{height}"
            );
        }
        let cfg = EncoderConfig::new()
            .usage_type(UsageType::ScreenContentRealTime)
            .rate_control_mode(RateControlMode::Bitrate)
            .bitrate(BitRate::from_bps(s.bitrate_bps))
            .max_frame_rate(FrameRate::from_hz(s.max_fps))
            // §5.1/5.3: no periodic IDRs; keyframes only on demand.
            .intra_frame_period(IntraFramePeriod::from_num_frames(0))
            .sps_pps_strategy(SpsPpsStrategy::ConstantId)
            // OpenH264 insists on scene-change detection for screen content and
            // cannot hold a bitrate without being allowed to skip frames; a
            // skipped frame is fine for a latest-wins display stream.
            .skip_frames(true)
            .scene_change_detect(true)
            .num_threads(s.threads);
        let inner = openh264::encoder::Encoder::with_api_config(OpenH264API::from_source(), cfg)
            .map_err(|e| anyhow!("creating OpenH264 encoder: {e}"))?;
        Ok(Self {
            inner,
            width,
            height,
            packed: Vec::new(),
        })
    }

    pub fn encode(
        &mut self,
        pixels: &[u8],
        stride: u32,
        format: PixelFormat,
        force_idr: bool,
        captured_at: Instant,
    ) -> Result<Vec<Encoded>> {
        let t0 = Instant::now();
        let row = (self.width * 4) as usize;
        let src: &[u8] = if stride as usize == row {
            &pixels[..row * self.height as usize]
        } else {
            self.packed.clear();
            for y in 0..self.height as usize {
                let start = y * stride as usize;
                self.packed.extend_from_slice(&pixels[start..start + row]);
            }
            &self.packed
        };
        let dims = (self.width as usize, self.height as usize);
        let yuv = match format {
            PixelFormat::Xrgb8888 | PixelFormat::Argb8888 => {
                YUVBuffer::from_bgra8_source(BgraSliceU8::new(src, dims))
            }
            PixelFormat::Xbgr8888 | PixelFormat::Abgr8888 => {
                YUVBuffer::from_rgba8_source(RgbaSliceU8::new(src, dims))
            }
        };
        let t_conv = t0.elapsed();
        if force_idr {
            self.inner.force_intra_frame();
        }
        let t1 = Instant::now();
        let bs = self
            .inner
            .encode(&yuv)
            .map_err(|e| anyhow!("encode: {e}"))?;
        if std::env::var_os("OD_PROFILE").is_some() {
            eprintln!("openh264: convert {:.1} ms, encode {:.1} ms, type {:?}", t_conv.as_secs_f64() * 1000.0, t1.elapsed().as_secs_f64() * 1000.0, bs.frame_type());
        }
        if matches!(bs.frame_type(), FrameType::Skip | FrameType::Invalid) {
            return Ok(Vec::new());
        }
        let raw = bs.to_vec();
        let mut units = access_units(&raw);
        let unit = units.pop().context("encoder produced no access unit")?;
        Ok(vec![Encoded {
            unit,
            captured_at,
            encode_ms: t0.elapsed().as_secs_f64() * 1000.0,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::Backend;

    fn settings() -> EncoderSettings {
        EncoderSettings {
            backend: Backend::Openh264,
            bitrate_bps: 8_000_000,
            max_fps: 60.0,
            threads: 1,
        }
    }

    fn solid(w: u32, h: u32, px: [u8; 4]) -> (Vec<u8>, u32) {
        // Stride with padding, like a real capture buffer.
        let stride = w * 4 + 32;
        let mut v = vec![0u8; (stride * h) as usize];
        for row in v.chunks_exact_mut(stride as usize) {
            for p in row[..(w * 4) as usize].chunks_exact_mut(4) {
                p.copy_from_slice(&px);
            }
        }
        (v, stride)
    }

    fn idr(px: &[u8], stride: u32, format: PixelFormat) -> Vec<u8> {
        let mut e = OpenH264Encoder::new(64, 64, settings()).unwrap();
        e.encode(px, stride, format, true, Instant::now())
            .unwrap()
            .into_iter()
            .flat_map(|e| e.unit.annexb)
            .collect()
    }

    #[test]
    fn refuses_frames_above_its_limit_at_creation() {
        assert!(supports(3840, 2160) && supports(2160, 3840) && supports(2556, 1178));
        assert!(!supports(4096, 2304) && !supports(2304, 4096) && !supports(3840, 2162));
        let e = OpenH264Encoder::new(4096, 2304, settings()).err().unwrap();
        assert!(e.to_string().contains("at most 3840x2160"), "{e}");
    }

    #[test]
    fn rgba_order_encodes_like_its_bgra_twin() {
        let (bgrx, stride) = solid(64, 64, [0, 0, 255, 255]);
        let (rgbx, _) = solid(64, 64, [255, 0, 0, 255]);
        let red_from_bgrx = idr(&bgrx, stride, PixelFormat::Xrgb8888);
        let red_from_rgbx = idr(&rgbx, stride, PixelFormat::Xbgr8888);
        assert!(!red_from_bgrx.is_empty());
        assert_eq!(red_from_bgrx, red_from_rgbx);
        // The same bytes read in the other order are pure blue.
        let blue = idr(&rgbx, stride, PixelFormat::Xrgb8888);
        assert_ne!(red_from_bgrx, blue);
    }
}

#[cfg(test)]
mod bench {
    use super::*;
    use crate::encoder::Backend;

    /// `cargo test --release -p od-sender --no-default-features bench -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn convert_vs_encode_1080p() {
        let (w, h) = (1920usize, 1080usize);
        let mut px = vec![0u8; w * h * 4];
        for (i, p) in px.chunks_exact_mut(4).enumerate() {
            let x = (i % w) as u32;
            let y = (i / w) as u32;
            p.copy_from_slice(&[(x ^ y) as u8, (x * 3 + y) as u8, (y * 2) as u8, 255]);
        }
        let t = Instant::now();
        let mut yuv = None;
        for _ in 0..5 {
            yuv = Some(YUVBuffer::from_bgra8_source(BgraSliceU8::new(&px, (w, h))));
        }
        println!("convert BGRA->I420: {:.1} ms/frame", t.elapsed().as_secs_f64() * 200.0);
        let mut e = OpenH264Encoder::new(
            w as u32,
            h as u32,
            EncoderSettings {
                backend: Backend::Openh264,
                bitrate_bps: 20_000_000,
                max_fps: 60.0,
                threads: 0,
            },
        )
        .unwrap();
        let y = yuv.unwrap();
        let t = Instant::now();
        for _ in 0..5 {
            let _ = e.inner.encode(&y).unwrap();
        }
        println!("openh264 encode only: {:.1} ms/frame", t.elapsed().as_secs_f64() * 200.0);
    }
}

#[cfg(test)]
mod bench2 {
    use super::*;
    use openh264::encoder::Complexity;

    #[test]
    #[ignore]
    fn complexity_and_threads_1080p() {
        let (w, h) = (1920usize, 1080usize);
        // Mostly flat "desktop-like" content with some noise: closer to real use.
        let mut px = vec![240u8; w * h * 4];
        for (i, p) in px.chunks_exact_mut(4).enumerate() {
            let x = i % w;
            let y = i / w;
            if (x / 64 + y / 64) % 3 == 0 {
                p.copy_from_slice(&[(x as u8).wrapping_mul(7), (y as u8).wrapping_mul(5), 90, 255]);
            }
        }
        let yuv = YUVBuffer::from_bgra8_source(BgraSliceU8::new(&px, (w, h)));
        for complexity in [Complexity::Low, Complexity::Medium] {
            for threads in [0u16, 1, 4, 12] {
                let cfg = EncoderConfig::new()
                    .usage_type(UsageType::ScreenContentRealTime)
                    .rate_control_mode(RateControlMode::Bitrate)
                    .bitrate(BitRate::from_bps(20_000_000))
                    .max_frame_rate(FrameRate::from_hz(60.0))
                    .intra_frame_period(IntraFramePeriod::from_num_frames(0))
                    .skip_frames(true)
                    .scene_change_detect(true)
                    .complexity(complexity)
                    .num_threads(threads);
                let mut e = openh264::encoder::Encoder::with_api_config(OpenH264API::from_source(), cfg).unwrap();
                let _ = e.encode(&yuv).unwrap(); // first IDR
                let t = Instant::now();
                for _ in 0..10 {
                    let _ = e.encode(&yuv).unwrap();
                }
                println!("{complexity:?} threads={threads}: {:.1} ms/frame (P frames)", t.elapsed().as_secs_f64() * 100.0);
            }
        }
    }
}

#[cfg(test)]
mod bench3 {
    use super::*;
    use openh264::encoder::Complexity;

    /// Moving content (every frame differs), like video or a busy desktop.
    #[test]
    #[ignore]
    fn motion_usage_types_1080p() {
        let (w, h) = (2360usize, 1640usize); // iPad Air/Pro 11" panel
        let frames: Vec<YUVBuffer> = (0..8)
            .map(|k| {
                let mut px = vec![200u8; w * h * 4];
                for (i, p) in px.chunks_exact_mut(4).enumerate() {
                    let x = i % w + k * 13;
                    let y = i / w + k * 7;
                    p.copy_from_slice(&[(x * 5 ^ y * 3) as u8, ((x / 3) + y) as u8, (y / 2 + x / 5) as u8, 255]);
                }
                YUVBuffer::from_bgra8_source(BgraSliceU8::new(&px, (w, h)))
            })
            .collect();
        for (name, usage) in [("ScreenContent", UsageType::ScreenContentRealTime), ("CameraVideo", UsageType::CameraVideoRealTime)] {
            for complexity in [Complexity::Low, Complexity::Medium] {
                let cfg = EncoderConfig::new()
                    .usage_type(usage)
                    .rate_control_mode(RateControlMode::Bitrate)
                    .bitrate(BitRate::from_bps(20_000_000))
                    .max_frame_rate(FrameRate::from_hz(60.0))
                    .intra_frame_period(IntraFramePeriod::from_num_frames(0))
                    .skip_frames(true)
                    .scene_change_detect(true)
                    .complexity(complexity)
                    .num_threads(0);
                let mut e = openh264::encoder::Encoder::with_api_config(OpenH264API::from_source(), cfg).unwrap();
                let _ = e.encode(&frames[0]).unwrap();
                let t = Instant::now();
                let mut bytes = 0usize;
                for i in 0..16 {
                    bytes += e.encode(&frames[(i % 7) + 1]).unwrap().to_vec().len();
                }
                println!("{name} {complexity:?}: {:.1} ms/frame, {} kB/frame", t.elapsed().as_secs_f64() * 1000.0 / 16.0, bytes / 16 / 1024);
            }
        }
    }
}
