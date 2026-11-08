//! Extended display through the signed, MIT-licensed
//! [Virtual Display Driver](https://github.com/VirtualDrivers/Virtual-Display-Driver)
//! (an IddCx indirect display driver; `winget install VirtualDrivers.Virtual-Display-Driver`).
//!
//! The driver always presents one virtual monitor once installed. Its modes
//! come from `C:\VirtualDisplayDriver\vdd_settings.xml`; writing a new
//! `<resolution>` there and sending `RELOAD_DRIVER` over its named pipe makes
//! the receiver's exact panel size available, and `ChangeDisplaySettingsEx`
//! then selects it. This is how the iPad's own resolution is adopted.
//!
//! Facts taken from the driver's source (Driver.cpp): pipe
//! `\\.\pipe\MTTVirtualDisplayPipe`, command `RELOAD_DRIVER`, a monitor count of 0
//! is treated as 1, global refresh rates apply to every resolution.

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

pub const PIPE: &str = r"\\.\pipe\MTTVirtualDisplayPipe";
const DEFAULT_DIR: &str = r"C:\VirtualDisplayDriver";

/// Folder holding `vdd_settings.xml` (override for tests: `OPENDISPLAY_VDD_DIR`).
pub fn settings_dir() -> PathBuf {
    std::env::var_os("OPENDISPLAY_VDD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DIR))
}

pub fn settings_path() -> PathBuf {
    settings_dir().join("vdd_settings.xml")
}

/// The driver is installed when its settings file exists.
pub fn installed() -> bool {
    settings_path().exists()
}

/// Refresh rates the driver offers for every resolution (its `g_refresh_rate`
/// list); the nearest one not above `want` is used.
pub fn pick_refresh(want: u32) -> u32 {
    const RATES: [u32; 6] = [60, 90, 120, 144, 165, 240];
    RATES
        .iter()
        .rev()
        .copied()
        .find(|r| *r <= want.max(60))
        .unwrap_or(60)
}

/// Make sure `<width>x<height>` is in the settings XML. Returns whether the
/// file changed (the driver must then be reloaded).
pub fn ensure_resolution(width: u32, height: u32) -> Result<bool> {
    let path = settings_path();
    let xml = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {} (is the Virtual Display Driver installed?)", path.display()))?;
    let Some(new_xml) = with_resolution(&xml, width, height) else {
        return Ok(false);
    };
    std::fs::write(&path, new_xml).with_context(|| {
        format!(
            "writing {}: run OpenDisplay once as administrator, or give your user write access to that folder",
            path.display()
        )
    })?;
    Ok(true)
}

/// `Some(new_xml)` with the resolution added, `None` if it is already there.
pub fn with_resolution(xml: &str, width: u32, height: u32) -> Option<String> {
    let w = format!("<width>{width}</width>");
    let h = format!("<height>{height}</height>");
    let mut rest = xml;
    while let Some(start) = rest.find("<resolution>") {
        let tail = &rest[start..];
        let end = tail.find("</resolution>").unwrap_or(tail.len());
        let block = &tail[..end];
        if block.contains(&w) && block.contains(&h) {
            return None;
        }
        rest = &tail[end.min(tail.len())..];
        if end == tail.len() {
            break;
        }
        rest = &rest["</resolution>".len()..];
    }
    let block = format!(
        "        <resolution>\n            <width>{width}</width>\n            <height>{height}</height>\n            <refresh_rate>60</refresh_rate>\n        </resolution>\n"
    );
    let close = xml.rfind("</resolutions>")?;
    // Insert at the start of the closing tag's line.
    let line_start = xml[..close].rfind('\n').map_or(0, |i| i + 1);
    let mut out = String::with_capacity(xml.len() + block.len());
    out.push_str(&xml[..line_start]);
    out.push_str(&block);
    out.push_str(&xml[line_start..]);
    Some(out)
}

/// Ask the driver to re-read its settings (it re-creates its monitor).
pub fn reload() -> Result<()> {
    let mut pipe = std::fs::OpenOptions::new()
        .write(true)
        .open(PIPE)
        .context("the Virtual Display Driver is not running (pipe missing)")?;
    let msg: Vec<u8> = "RELOAD_DRIVER".encode_utf16().flat_map(u16::to_le_bytes).collect();
    pipe.write_all(&msg).context("sending RELOAD_DRIVER")?;
    Ok(())
}

pub fn not_installed_message() -> String {
    "No second screen to extend onto. Install the free virtual display driver once \
     (winget install --id VirtualDrivers.Virtual-Display-Driver -e, or the Install button in \
     OpenDisplay), then try again. To just mirror your main screen instead, use --output DISPLAY1."
        .to_string()
}

#[allow(dead_code)]
fn _assert_bail_used() -> Result<()> {
    bail!("unused")
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = "<vdd_settings>\n    <resolutions>\n        <resolution>\n            <width>1920</width>\n            <height>1080</height>\n            <refresh_rate>60</refresh_rate>\n        </resolution>\n    </resolutions>\n</vdd_settings>\n";

    #[test]
    fn existing_resolution_is_left_alone() {
        assert!(with_resolution(XML, 1920, 1080).is_none());
    }

    #[test]
    fn new_resolution_is_added_before_the_closing_tag() {
        let out = with_resolution(XML, 2360, 1640).unwrap();
        assert!(out.contains("<width>2360</width>"));
        assert!(out.contains("<height>1640</height>"));
        assert!(out.find("2360").unwrap() < out.find("</resolutions>").unwrap());
        // Adding it again is a no-op.
        assert!(with_resolution(&out, 2360, 1640).is_none());
        // The original entry survives.
        assert!(out.contains("<width>1920</width>"));
    }

    #[test]
    fn width_and_height_must_match_in_the_same_block() {
        // 1080x1920 is not 1920x1080.
        assert!(with_resolution(XML, 1080, 1920).is_some());
    }

    #[test]
    fn refresh_rate_picks_the_nearest_offered_one() {
        assert_eq!(pick_refresh(60), 60);
        assert_eq!(pick_refresh(100), 90);
        assert_eq!(pick_refresh(120), 120);
        assert_eq!(pick_refresh(30), 60);
        assert_eq!(pick_refresh(500), 240);
    }
}
