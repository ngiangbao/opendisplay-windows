//! `od-sender doctor` on Windows: reports what exists. No backend yet (T2.3).

use std::path::Path;

use crate::backend::detect::Request;
use crate::{config, encoder};

/// Returns whether `run` could work here (false until a backend exists).
pub fn doctor(_req: &Request, config_path: Option<&Path>) -> bool {
    println!("platform: windows");
    #[cfg(windows)]
    {
        println!("backend:  windows monitor adoption (Desktop Duplication + SendInput)");
        match crate::backend::win::monitors::list() {
            Ok(list) => {
                for m in list {
                    println!(
                        "monitor:  {}  {}x{} at {},{}  {:.0} Hz  scale {}{}",
                        m.name, m.width, m.height, m.x, m.y, m.hz, m.scale,
                        if m.primary { "  (primary)" } else { "" }
                    );
                }
            }
            Err(e) => println!("monitor:  could not list monitors: {e:#}"),
        }
        for (name, adapter, flags) in crate::backend::win::monitors::devices() {
            println!(
                "device:   {name}  {adapter}  {}",
                if flags & 1 != 0 { "attached" } else { "NOT attached to the desktop" }
            );
        }
        println!(
            "driver:   Virtual Display Driver {}",
            if crate::backend::win::vdd::installed() {
                "installed (settings file found)"
            } else {
                "not installed"
            }
        );
    }
    #[cfg(not(windows))]
    println!("backend:  none for this platform");
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
    cfg!(windows)
}
