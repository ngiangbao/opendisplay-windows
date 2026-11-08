//! Per-monitor Windows display scaling (the "Scale" setting in Settings >
//! Display) through the DisplayConfig calls Settings itself uses.
//!
//! The scale API is undocumented but stable since Windows 10: header types
//! -3 (get: min/current/max, all relative to the "recommended" step) and -4
//! (set). We use it so an iPad-sized virtual monitor gets 200% instead of
//! Windows' guess, making its desktop the size the iPad would show natively.

use anyhow::{Result, bail};
use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_DEVICE_INFO_TYPE, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DisplayConfigGetDeviceInfo,
    DisplayConfigSetDeviceInfo, GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS,
    QueryDisplayConfig,
};
use windows::Win32::Foundation::LUID;

/// Scale percentages Windows offers, in order; indexes are what the API uses.
const STEPS: [u32; 12] = [100, 125, 150, 175, 200, 225, 250, 300, 350, 400, 450, 500];

const GET_DPI: i32 = -3;
const SET_DPI: i32 = -4;

#[repr(C)]
struct GetDpi {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    min_rel: i32,
    cur_rel: i32,
    max_rel: i32,
}

#[repr(C)]
struct SetDpi {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    rel: i32,
}

/// Current, recommended and range of the scale of `device_name`, as percents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScaleInfo {
    pub current: u32,
    pub recommended: u32,
    pub min: u32,
    pub max: u32,
}

fn find_source(device_name: &str) -> Result<(LUID, u32)> {
    // SAFETY: buffers are sized by GetDisplayConfigBufferSizes and the
    // header sizes are set before each query.
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        let r = GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm);
        if r.0 != 0 {
            bail!("GetDisplayConfigBufferSizes failed ({})", r.0);
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        let r = QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut np,
            paths.as_mut_ptr(),
            &mut nm,
            modes.as_mut_ptr(),
            None,
        );
        if r.0 != 0 {
            bail!("QueryDisplayConfig failed ({})", r.0);
        }
        for p in paths.iter().take(np as usize) {
            let mut sn = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            sn.header.r#type = DISPLAYCONFIG_DEVICE_INFO_TYPE(1); // SOURCE_NAME
            sn.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            sn.header.adapterId = p.sourceInfo.adapterId;
            sn.header.id = p.sourceInfo.id;
            if DisplayConfigGetDeviceInfo(&mut sn.header) != 0 {
                continue;
            }
            let n = sn.viewGdiDeviceName.iter().position(|&c| c == 0).unwrap_or(sn.viewGdiDeviceName.len());
            if String::from_utf16_lossy(&sn.viewGdiDeviceName[..n]).eq_ignore_ascii_case(device_name) {
                return Ok((p.sourceInfo.adapterId, p.sourceInfo.id));
            }
        }
    }
    bail!("{device_name} is not an active display path")
}

/// Read the scale state of a monitor.
pub fn get(device_name: &str) -> Result<ScaleInfo> {
    let (adapter, id) = find_source(device_name)?;
    let mut g = GetDpi {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_TYPE(GET_DPI),
            size: std::mem::size_of::<GetDpi>() as u32,
            adapterId: adapter,
            id,
        },
        min_rel: 0,
        cur_rel: 0,
        max_rel: 0,
    };
    // SAFETY: GetDpi is the documented layout (header + three i32) with its size set.
    let r = unsafe { DisplayConfigGetDeviceInfo(&mut g.header) };
    if r != 0 {
        bail!("reading the display scale failed ({r})");
    }
    let rec = (-g.min_rel).clamp(0, STEPS.len() as i32 - 1);
    let at = |rel: i32| STEPS[(rec + rel).clamp(0, STEPS.len() as i32 - 1) as usize];
    Ok(ScaleInfo {
        current: at(g.cur_rel),
        recommended: STEPS[rec as usize],
        min: at(g.min_rel),
        max: at(g.max_rel),
    })
}

/// Set the scale to the supported step nearest to `percent`.
pub fn set(device_name: &str, percent: u32) -> Result<u32> {
    let (adapter, id) = find_source(device_name)?;
    let mut g = GetDpi {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_TYPE(GET_DPI),
            size: std::mem::size_of::<GetDpi>() as u32,
            adapterId: adapter,
            id,
        },
        min_rel: 0,
        cur_rel: 0,
        max_rel: 0,
    };
    // SAFETY: see `get`.
    let r = unsafe { DisplayConfigGetDeviceInfo(&mut g.header) };
    if r != 0 {
        bail!("reading the display scale failed ({r})");
    }
    let rec = (-g.min_rel).clamp(0, STEPS.len() as i32 - 1);
    let target_idx = STEPS
        .iter()
        .enumerate()
        .min_by_key(|(_, s)| s.abs_diff(percent))
        .map(|(i, _)| i as i32)
        .unwrap_or(rec);
    let rel = (target_idx - rec).clamp(g.min_rel, g.max_rel);
    if rel == g.cur_rel {
        return Ok(STEPS[(rec + rel) as usize]);
    }
    let mut s = SetDpi {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_TYPE(SET_DPI),
            size: std::mem::size_of::<SetDpi>() as u32,
            adapterId: adapter,
            id,
        },
        rel,
    };
    // SAFETY: SetDpi is the layout the API expects (header + one i32).
    let r = unsafe { DisplayConfigSetDeviceInfo(&mut s.header) };
    if r != 0 {
        bail!("setting the display scale failed ({r})");
    }
    Ok(STEPS[(rec + rel) as usize])
}

#[cfg(test)]
mod tests {
    /// Read-only check on the first active monitor (needs a desktop):
    /// `cargo test -p od-sender --no-default-features dpi -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn reads_the_scale_of_the_primary_monitor() {
        let list = crate::backend::win::monitors::list().unwrap();
        let m = list.iter().find(|m| m.primary).unwrap();
        let info = super::get(&m.name).unwrap();
        println!("{}: {info:?}", m.name);
        assert!(info.min <= info.current && info.current <= info.max);
    }
}
