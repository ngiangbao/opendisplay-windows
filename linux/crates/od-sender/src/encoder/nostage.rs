//! Platforms without FFmpeg (Windows): there is no convert stage, so frames
//! reach the encoder unconverted and OpenH264 converts them itself. The
//! types exist so the session code is the same everywhere.

use super::Encoder;
use crate::backend::FrameSink;

pub const MIN_STAGE_WIDTH: u32 = 2560;

/// Never constructed: no stage means no pre-converted frames.
pub struct YuvFrame(());

impl YuvFrame {
    pub fn dimensions(&self) -> (u32, u32) {
        (0, 0)
    }
}

#[derive(Clone, Default)]
pub struct ConvertStage;

impl ConvertStage {
    pub fn new() -> Self {
        Self
    }

    pub fn bind(&self, _encoder: &Encoder) {}

    pub fn sink(&self, next: FrameSink) -> FrameSink {
        next
    }
}
