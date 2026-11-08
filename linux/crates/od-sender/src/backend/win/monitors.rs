//! Monitor discovery through DXGI (same adapter/output numbering Desktop
//! Duplication needs) plus GDI for the refresh rate and primary flag.

use anyhow::{Context, Result};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
use windows::Win32::Graphics::Gdi::{
    DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplaySettingsW, GetMonitorInfoW, MONITORINFO,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    SetProcessDpiAwarenessContext,
};
use windows::core::PCWSTR;

use crate::backend::DisplayInfo;

#[derive(Debug, Clone)]
pub struct Monitor {
    /// GDI device name, e.g. `\\.\DISPLAY2`.
    pub name: String,
    /// Top-left in the virtual desktop, physical pixels.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub hz: f64,
    pub primary: bool,
    pub scale: f64,
    /// DXGI adapter and output index, for Desktop Duplication.
    pub adapter: u32,
    pub output: u32,
}

impl Monitor {
    pub fn display_info(&self) -> DisplayInfo {
        DisplayInfo {
            name: self.name.clone(),
            width: self.width,
            height: self.height,
            refresh_hz: self.hz,
            scale: self.scale,
            x: self.x,
            y: self.y,
        }
    }
}

/// Per-monitor DPI awareness, so coordinates are physical pixels. Harmless if
/// the process already chose (the call then fails and is ignored).
pub fn make_dpi_aware() {
    // SAFETY: plain process-wide setting call.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

fn wide(v: &[u16]) -> String {
    let n = v.iter().position(|&c| c == 0).unwrap_or(v.len());
    String::from_utf16_lossy(&v[..n])
}

/// Monitors attached to the desktop, primary first.
pub fn list() -> Result<Vec<Monitor>> {
    make_dpi_aware();
    // SAFETY: COM calls on interfaces we create and own; every out-pointer
    // is a local the call fills in.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().context("CreateDXGIFactory1")?;
        let mut out = Vec::new();
        let mut ai = 0;
        while let Ok(adapter) = factory.EnumAdapters1(ai) {
            let mut oi = 0;
            while let Ok(output) = adapter.EnumOutputs(oi) {
                let desc = output.GetDesc().context("IDXGIOutput::GetDesc")?;
                if desc.AttachedToDesktop.as_bool() {
                    let r = desc.DesktopCoordinates;
                    let mut mi = MONITORINFO {
                        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    let primary = GetMonitorInfoW(desc.Monitor, &mut mi).as_bool()
                        && mi.dwFlags & 1 != 0; // MONITORINFOF_PRIMARY
                    let mut dm = DEVMODEW {
                        dmSize: std::mem::size_of::<DEVMODEW>() as u16,
                        ..Default::default()
                    };
                    let hz = if EnumDisplaySettingsW(
                        PCWSTR(desc.DeviceName.as_ptr()),
                        ENUM_CURRENT_SETTINGS,
                        &mut dm,
                    )
                    .as_bool()
                        && dm.dmDisplayFrequency > 1
                    {
                        dm.dmDisplayFrequency as f64
                    } else {
                        60.0
                    };
                    let (mut dx, mut dy) = (96u32, 96u32);
                    if GetDpiForMonitor(desc.Monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_err()
                    {
                        dx = 96;
                    }
                    out.push(Monitor {
                        name: wide(&desc.DeviceName),
                        x: r.left,
                        y: r.top,
                        width: (r.right - r.left).max(0) as u32,
                        height: (r.bottom - r.top).max(0) as u32,
                        hz,
                        primary,
                        scale: dx as f64 / 96.0,
                        adapter: ai,
                        output: oi,
                    });
                }
                oi += 1;
            }
            ai += 1;
        }
        out.sort_by_key(|m| !m.primary);
        Ok(out)
    }
}
