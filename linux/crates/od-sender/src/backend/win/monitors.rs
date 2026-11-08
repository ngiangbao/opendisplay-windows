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


/// Whether `device_name` (`\\.\DISPLAYn`) is a Virtual Display Driver monitor
/// (its EDID manufacturer is "MTT").
pub fn is_virtual(device_name: &str) -> bool {
    use windows::Win32::Graphics::Gdi::{DISPLAY_DEVICEW, EnumDisplayDevicesW};
    let name: Vec<u16> = device_name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut dd = DISPLAY_DEVICEW {
        cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
        ..Default::default()
    };
    // SAFETY: `name` is NUL-terminated and `dd` has its size set.
    let ok = unsafe { EnumDisplayDevicesW(PCWSTR(name.as_ptr()), 0, &mut dd, 0) }.as_bool();
    ok && {
        let id = wide(&dd.DeviceID).to_uppercase();
        let s = wide(&dd.DeviceString).to_lowercase();
        id.contains("MTT") || s.contains("virtual display")
    }
}

/// A Virtual Display Driver output that exists but is not part of the desktop
/// (we detach it when a session ends). `None` if there is none.
pub fn detached_virtual() -> Option<String> {
    use windows::Win32::Graphics::Gdi::{DISPLAY_DEVICEW, EnumDisplayDevicesW};
    const ATTACHED: u32 = 1; // DISPLAY_DEVICE_ATTACHED_TO_DESKTOP
    let mut i = 0;
    loop {
        let mut dd = DISPLAY_DEVICEW {
            cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        // SAFETY: `dd` has its size set; a null device enumerates adapters.
        if !unsafe { EnumDisplayDevicesW(PCWSTR::null(), i, &mut dd, 0) }.as_bool() {
            return None;
        }
        i += 1;
        let adapter = wide(&dd.DeviceString).to_lowercase();
        let id = wide(&dd.DeviceID).to_uppercase();
        if (adapter.contains("virtual display") || id.contains("MTT")) && dd.StateFlags.0 & ATTACHED == 0 {
            return Some(wide(&dd.DeviceName));
        }
    }
}

/// Remove a monitor from the desktop (zero size), so a virtual screen does not
/// stay behind after a session. Windows windows move back to the other screens.
pub fn detach(device_name: &str) -> Result<()> {
    use windows::Win32::Graphics::Gdi::{
        CDS_NORESET, CDS_TYPE, CDS_UPDATEREGISTRY, ChangeDisplaySettingsExW, DISP_CHANGE_SUCCESSFUL,
        DM_PELSHEIGHT, DM_PELSWIDTH, DM_POSITION,
    };
    let name: Vec<u16> = device_name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `name` is NUL-terminated; the DEVMODE has its size set.
    unsafe {
        let mut dm = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            dmFields: DM_PELSWIDTH | DM_PELSHEIGHT | DM_POSITION,
            ..Default::default()
        };
        dm.Anonymous1.Anonymous2.dmPosition.x = 0;
        dm.Anonymous1.Anonymous2.dmPosition.y = 0;
        let r = ChangeDisplaySettingsExW(
            PCWSTR(name.as_ptr()),
            Some(&dm),
            None,
            CDS_UPDATEREGISTRY | CDS_NORESET,
            None,
        );
        if r != DISP_CHANGE_SUCCESSFUL {
            anyhow::bail!("Windows refused to detach {device_name} (code {})", r.0);
        }
        let r = ChangeDisplaySettingsExW(PCWSTR::null(), None, None, CDS_TYPE(0), None);
        if r != DISP_CHANGE_SUCCESSFUL {
            anyhow::bail!("Windows could not apply the detach (code {})", r.0);
        }
    }
    Ok(())
}

/// Set a monitor's mode and (optionally) its top-left position in the virtual
/// desktop, and apply it now. Windows keeps it for the next session too.
pub fn set_mode(
    device_name: &str,
    width: u32,
    height: u32,
    hz: u32,
    pos: Option<(i32, i32)>,
) -> Result<()> {
    use windows::Win32::Graphics::Gdi::{
        CDS_NORESET, CDS_TYPE, CDS_UPDATEREGISTRY, ChangeDisplaySettingsExW, DISP_CHANGE_SUCCESSFUL,
        DM_BITSPERPEL, DM_DISPLAYFREQUENCY, DM_PELSHEIGHT, DM_PELSWIDTH, DM_POSITION,
        ENUM_REGISTRY_SETTINGS,
    };
    let name: Vec<u16> = device_name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `name` is NUL-terminated; `dm` is initialised by
    // EnumDisplaySettingsW before it is changed.
    unsafe {
        let mut dm = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        // A detached monitor has no current mode: start from the registry one,
        // else from a fresh 32-bit mode.
        let have = EnumDisplaySettingsW(PCWSTR(name.as_ptr()), ENUM_CURRENT_SETTINGS, &mut dm).as_bool()
            || EnumDisplaySettingsW(PCWSTR(name.as_ptr()), ENUM_REGISTRY_SETTINGS, &mut dm).as_bool();
        if !have {
            dm = DEVMODEW {
                dmSize: std::mem::size_of::<DEVMODEW>() as u16,
                dmBitsPerPel: 32,
                ..Default::default()
            };
        }
        dm.dmPelsWidth = width;
        dm.dmPelsHeight = height;
        dm.dmDisplayFrequency = hz;
        dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY | DM_BITSPERPEL;
        if dm.dmBitsPerPel == 0 {
            dm.dmBitsPerPel = 32;
        }
        if let Some((x, y)) = pos {
            dm.Anonymous1.Anonymous2.dmPosition.x = x;
            dm.Anonymous1.Anonymous2.dmPosition.y = y;
            dm.dmFields |= DM_POSITION;
        }
        let r = ChangeDisplaySettingsExW(
            PCWSTR(name.as_ptr()),
            Some(&dm),
            None,
            CDS_UPDATEREGISTRY | CDS_NORESET,
            None,
        );
        if r != DISP_CHANGE_SUCCESSFUL {
            anyhow::bail!("Windows refused {width}x{height}@{hz} on {device_name} (code {})", r.0);
        }
        // Apply the batched change.
        let r = ChangeDisplaySettingsExW(PCWSTR::null(), None, None, CDS_TYPE(0), None);
        if r != DISP_CHANGE_SUCCESSFUL {
            anyhow::bail!("Windows could not apply the new mode (code {})", r.0);
        }
    }
    Ok(())
}
