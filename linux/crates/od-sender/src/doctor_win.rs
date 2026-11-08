//! `od-sender doctor` on Windows: reports what exists. No backend yet (T2.3).

use std::path::Path;

use crate::backend::detect::Request;
use crate::{config, encoder};

/// Returns whether `run` could work here (false until a backend exists).
pub fn doctor(_req: &Request, config_path: Option<&Path>) -> bool {
    println!("platform: windows");
    println!("backend:  none yet (task T2.3, see windows/DESIGN-sender.md)");
    let path = config_path.map(Path::to_path_buf).or_else(config::default_path);
    match path {
        Some(p) => println!("config:   {} ({})", p.display(), if p.exists() { "found" } else { "absent" }),
        None => println!("config:   no location"),
    }
    let s = encoder::EncoderSettings {
        backend: encoder::Backend::Openh264,
        bitrate_bps: 1_000_000,
        max_fps: 30.0,
        threads: 0,
    };
    match encoder::Encoder::new(320, 240, s) {
        Ok(_) => println!("encoder:  OpenH264 opens"),
        Err(e) => println!("encoder:  OpenH264 failed: {e:#}"),
    }
    false
}
