//! Enumerate platform outputs to size `hello` from the real panel: physical
//! pixels, refresh, scale, and the platform display name.

#[cfg(target_os = "linux")]
use std::collections::BTreeMap;

#[cfg(target_os = "linux")]
use anyhow::bail;
use anyhow::{Context, Result};
#[cfg(target_os = "linux")]
use wayland_client::protocol::{wl_output, wl_registry};
#[cfg(target_os = "linux")]
use wayland_client::{Connection, Dispatch, QueueHandle};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutputInfo {
    pub name: String,
    pub description: String,
    /// Current mode, physical pixels.
    pub width: i32,
    pub height: i32,
    pub refresh_mhz: i32,
    /// Effective DPI scale relative to 96 DPI.
    pub scale: f64,
    /// The system's primary output (Windows); false where unknown.
    pub primary: bool,
}

impl OutputInfo {
    /// Whole Hz, or `None` when the output does not know (headless and
    /// virtual outputs report 0): announcing that as 1 Hz would cap the
    /// sender at one frame per second.
    pub fn refresh_hz(&self) -> Option<u32> {
        (self.refresh_mhz >= 1000).then(|| ((self.refresh_mhz as f64) / 1000.0).round() as u32)
    }
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct State {
    outputs: BTreeMap<u32, OutputInfo>,
    too_old: bool,
}

#[cfg(target_os = "linux")]
impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            if interface == "wl_output" {
                if version < 4 {
                    // Pre-v4 outputs have no `name` event; we cannot address them.
                    state.too_old = true;
                    return;
                }
                registry.bind::<wl_output::WlOutput, _, _>(name, 4, qh, name);
                state.outputs.insert(name, OutputInfo::default());
            }
        }
    }
}

#[cfg(target_os = "linux")]
impl Dispatch<wl_output::WlOutput, u32> for State {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(info) = state.outputs.get_mut(global) else {
            return;
        };
        match event {
            wl_output::Event::Mode {
                flags,
                width,
                height,
                refresh,
            } => {
                if flags
                    .into_result()
                    .is_ok_and(|f| f.contains(wl_output::Mode::Current))
                {
                    info.width = width;
                    info.height = height;
                    info.refresh_mhz = refresh;
                }
            }
            wl_output::Event::Scale { factor } => info.scale = factor as f64,
            wl_output::Event::Name { name } => info.name = name,
            wl_output::Event::Description { description } => info.description = description,
            _ => {}
        }
    }
}

/// All outputs of the compositor named by `WAYLAND_DISPLAY`.
#[cfg(target_os = "linux")]
pub fn enumerate() -> Result<Vec<OutputInfo>> {
    let conn = Connection::connect_to_env().context("connecting to the Wayland display")?;
    let mut queue = conn.new_event_queue::<State>();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut state = State::default();
    queue.roundtrip(&mut state).context("registry roundtrip")?;
    queue.roundtrip(&mut state).context("output roundtrip")?;
    if state.too_old && state.outputs.is_empty() {
        bail!("compositor exposes wl_output < v4; pass --width/--height/--output explicitly");
    }
    Ok(state
        .outputs
        .into_values()
        .filter(|o| o.width > 0 && o.height > 0)
        .collect())
}

#[cfg(target_os = "windows")]
mod windows {
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsW,
        GetMonitorInfoW, HDC, MONITORINFO, MONITORINFOEXW,
    };
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

    use super::OutputInfo;

    const DEFAULT_DPI: u32 = 96;

    pub fn enumerate() -> anyhow::Result<Vec<OutputInfo>> {
        let mut outputs = Vec::new();
        // SAFETY: the callback only uses `data` as the live `outputs` vector
        // passed here. Windows invokes it synchronously before this call returns.
        let ok = unsafe {
            EnumDisplayMonitors(
                null_mut(),
                null(),
                Some(callback),
                (&mut outputs as *mut Vec<OutputInfo>) as LPARAM,
            )
        };
        if ok == 0 {
            anyhow::bail!("EnumDisplayMonitors failed")
        }
        // Primary first: `pick(None)` takes the first output, and a borderless
        // fullscreen window opens on the primary monitor.
        outputs.sort_by_key(|o: &OutputInfo| !o.primary);
        Ok(outputs)
    }

    unsafe extern "system" fn callback(
        monitor: *mut c_void,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> i32 {
        // SAFETY: `data` is the vector pointer supplied by `enumerate`, and the
        // callback is synchronous. A failed monitor query is skipped while
        // enumeration continues for the remaining monitors.
        let outputs = unsafe { &mut *(data as *mut Vec<OutputInfo>) };
        if let Some(info) = unsafe { monitor_info(monitor) } {
            outputs.push(info);
        }
        1
    }

    unsafe fn monitor_info(monitor: *mut c_void) -> Option<OutputInfo> {
        // SAFETY: both structures are plain Win32 data structures. Their size
        // fields are initialized as required before passing writable pointers.
        let mut info: MONITORINFOEXW = unsafe { zeroed() };
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo as *mut MONITORINFO) } == 0 {
            return None;
        }

        let name = wide_string(&info.szDevice);
        // SAFETY: `szDevice` is NUL-terminated storage owned by `info`; mode has
        // the required dmSize and remains writable for the duration of the call.
        let mut mode: DEVMODEW = unsafe { zeroed() };
        mode.dmSize = size_of::<DEVMODEW>() as u16;
        let has_mode = unsafe {
            EnumDisplaySettingsW(info.szDevice.as_ptr(), ENUM_CURRENT_SETTINGS, &mut mode)
        } != 0;

        let rect = info.monitorInfo.rcMonitor;
        let width = if has_mode {
            mode.dmPelsWidth as i32
        } else {
            rect.right - rect.left
        };
        let height = if has_mode {
            mode.dmPelsHeight as i32
        } else {
            rect.bottom - rect.top
        };
        let refresh_mhz = if has_mode && mode.dmDisplayFrequency > 1 {
            mode.dmDisplayFrequency.saturating_mul(1000) as i32
        } else {
            0
        };

        let mut dpi_x = DEFAULT_DPI;
        let mut dpi_y = DEFAULT_DPI;
        // SAFETY: the monitor handle comes from EnumDisplayMonitors and both DPI
        // pointers are valid. A failed query leaves the 96-DPI fallback in place.
        if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) } < 0 {
            dpi_x = DEFAULT_DPI;
        }

        Some(OutputInfo {
            description: name.clone(),
            name,
            width,
            height,
            refresh_mhz,
            scale: dpi_x as f64 / DEFAULT_DPI as f64,
            primary: info.monitorInfo.dwFlags & 1 != 0, // MONITORINFOF_PRIMARY
        })
    }

    fn wide_string(value: &[u16]) -> String {
        let len = value.iter().position(|&c| c == 0).unwrap_or(value.len());
        String::from_utf16_lossy(&value[..len])
    }
}

/// All active Windows monitors, named like `\\.\DISPLAY1`.
#[cfg(target_os = "windows")]
pub fn enumerate() -> Result<Vec<OutputInfo>> {
    windows::enumerate()
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn enumerate() -> Result<Vec<OutputInfo>> {
    anyhow::bail!("output enumeration is not implemented on this platform; pass --width/--height")
}

/// Pick the output named `name`, or the first one.
pub fn pick(outputs: &[OutputInfo], name: Option<&str>) -> Result<OutputInfo> {
    match name {
        Some(n) => outputs
            .iter()
            .find(|o| o.name == n)
            .cloned()
            .with_context(|| {
                format!(
                    "no output named {n}; have: {}",
                    outputs
                        .iter()
                        .map(|o| o.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }),
        None => outputs.first().cloned().context("no outputs found"),
    }
}

#[cfg(test)]
mod tests {
    use super::OutputInfo;

    #[test]
    fn unknown_refresh_is_absent_not_one_hertz() {
        let o = |refresh_mhz| OutputInfo {
            name: "HEADLESS-1".into(),
            description: String::new(),
            width: 1280,
            height: 720,
            refresh_mhz,
            scale: 1.0,
            primary: false,
        };
        assert_eq!(o(0).refresh_hz(), None);
        assert_eq!(o(59_940).refresh_hz(), Some(60));
        assert_eq!(o(119_800).refresh_hz(), Some(120));
    }
}
