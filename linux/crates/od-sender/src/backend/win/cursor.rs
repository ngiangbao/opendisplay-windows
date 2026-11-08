//! Paints the mouse cursor into captured frames. Desktop Duplication frames
//! do not contain the cursor, and on the extended screen the person drives it
//! by touch, so without this the receiver would show no pointer at all.
//!
//! The current cursor is read with `GetCursorInfo` / `GetIconInfo` and cached
//! per cursor handle; colour cursors are alpha-blended, monochrome ones use
//! the AND/XOR masks (including the "invert" I-beam).

use std::ffi::c_void;

use windows::Win32::Graphics::Gdi::{
    BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HGDIOBJ, ReleaseDC,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CURSORINFO, CURSORINFO_FLAGS, GetCursorInfo, GetIconInfo, HICON, ICONINFO,
};

const CURSOR_SHOWING: u32 = 1;

struct Sprite {
    w: usize,
    h: usize,
    hot_x: i32,
    hot_y: i32,
    /// BGRA, top-down, straight alpha. For monochrome cursors `invert`
    /// marks pixels that invert the screen instead.
    bgra: Vec<u8>,
    invert: Vec<bool>,
}

#[derive(Default)]
pub struct CursorPainter {
    cached: Option<(isize, Option<Sprite>)>,
}

impl CursorPainter {
    /// Paint the cursor onto `frame` (BGRX, `fw` x `fh`, tightly packed) for a
    /// monitor whose top-left is `(ox, oy)` in the virtual desktop.
    pub fn paint(&mut self, frame: &mut [u8], fw: usize, fh: usize, ox: i32, oy: i32) {
        let mut ci = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `ci` has its size set.
        if unsafe { GetCursorInfo(&mut ci) }.is_err() || ci.flags != CURSORINFO_FLAGS(CURSOR_SHOWING) {
            return;
        }
        let key = ci.hCursor.0 as isize;
        if self.cached.as_ref().map(|c| c.0) != Some(key) {
            self.cached = Some((key, load_sprite(HICON(ci.hCursor.0))));
        }
        let Some((_, Some(sp))) = &self.cached else {
            return;
        };
        let x0 = ci.ptScreenPos.x - ox - sp.hot_x;
        let y0 = ci.ptScreenPos.y - oy - sp.hot_y;
        for sy in 0..sp.h {
            let fy = y0 + sy as i32;
            if fy < 0 || fy >= fh as i32 {
                continue;
            }
            for sx in 0..sp.w {
                let fx = x0 + sx as i32;
                if fx < 0 || fx >= fw as i32 {
                    continue;
                }
                let si = (sy * sp.w + sx) * 4;
                let di = (fy as usize * fw + fx as usize) * 4;
                if sp.invert[sy * sp.w + sx] {
                    for c in 0..3 {
                        frame[di + c] = !frame[di + c];
                    }
                    continue;
                }
                let a = sp.bgra[si + 3] as u32;
                if a == 0 {
                    continue;
                }
                for c in 0..3 {
                    let s = sp.bgra[si + c] as u32;
                    let d = frame[di + c] as u32;
                    frame[di + c] = ((s * a + d * (255 - a) + 127) / 255) as u8;
                }
            }
        }
    }
}

fn load_sprite(icon: HICON) -> Option<Sprite> {
    // SAFETY: GDI calls on bitmaps returned by GetIconInfo, which we delete.
    unsafe {
        let mut ii = ICONINFO::default();
        GetIconInfo(icon, &mut ii).ok()?;
        let result = build(&ii);
        if !ii.hbmMask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
        }
        if !ii.hbmColor.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
        }
        result
    }
}

/// # Safety
/// `ii` must come from a successful `GetIconInfo`.
unsafe fn build(ii: &ICONINFO) -> Option<Sprite> {
    unsafe {
        let color = !ii.hbmColor.is_invalid();
        let src = if color { ii.hbmColor } else { ii.hbmMask };
        let mut bm = BITMAP::default();
        if GetObjectW(
            HGDIOBJ(src.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut c_void),
        ) == 0
        {
            return None;
        }
        let w = bm.bmWidth as usize;
        let total_h = bm.bmHeight as usize;
        // A monochrome cursor stores AND mask over XOR mask in one bitmap.
        let h = if color { total_h } else { total_h / 2 };
        if w == 0 || h == 0 || w > 512 || h > 512 {
            return None;
        }
        let dc = GetDC(None);
        let read = |bmp: windows::Win32::Graphics::Gdi::HBITMAP, lines: usize| -> Option<Vec<u8>> {
            let mut bi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(lines as i32), // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut buf = vec![0u8; w * lines * 4];
            let got = GetDIBits(
                dc,
                bmp,
                0,
                lines as u32,
                Some(buf.as_mut_ptr() as *mut c_void),
                &mut bi,
                DIB_RGB_COLORS,
            );
            (got != 0).then_some(buf)
        };
        let out = if color {
            let mut bgra = read(ii.hbmColor, h);
            let mask = read(ii.hbmMask, h);
            if let (Some(b), Some(m)) = (bgra.as_mut(), mask.as_ref()) {
                // Cursors without an alpha channel use the AND mask for
                // transparency (white mask pixel = transparent).
                let has_alpha = b.chunks_exact(4).any(|p| p[3] != 0);
                for (i, p) in b.chunks_exact_mut(4).enumerate() {
                    if !has_alpha {
                        p[3] = if m[i * 4] > 127 { 0 } else { 255 };
                    }
                }
            }
            bgra.map(|bgra| Sprite {
                w,
                h,
                hot_x: ii.xHotspot as i32,
                hot_y: ii.yHotspot as i32,
                bgra,
                invert: vec![false; w * h],
            })
        } else {
            read(ii.hbmMask, h * 2).map(|m| {
                let mut bgra = vec![0u8; w * h * 4];
                let mut invert = vec![false; w * h];
                for i in 0..w * h {
                    let and = m[i * 4] > 127;
                    let xor = m[(w * h + i) * 4] > 127;
                    match (and, xor) {
                        (true, false) => {} // transparent
                        (false, false) => bgra[i * 4 + 3] = 255, // black
                        (false, true) => {
                            bgra[i * 4..i * 4 + 4].copy_from_slice(&[255, 255, 255, 255])
                        }
                        (true, true) => invert[i] = true,
                    }
                }
                Sprite {
                    w,
                    h,
                    hot_x: ii.xHotspot as i32,
                    hot_y: ii.yHotspot as i32,
                    bgra,
                    invert,
                }
            })
        };
        ReleaseDC(None, dc);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs an interactive desktop with a visible cursor:
    /// `cargo test -p od-sender --no-default-features cursor -- --ignored`
    #[test]
    #[ignore]
    fn the_current_cursor_changes_some_pixels() {
        let mut p = CursorPainter::default();
        let (w, h) = (4000usize, 3000usize);
        let mut frame = vec![128u8; w * h * 4];
        p.paint(&mut frame, w, h, 0, 0);
        let changed = frame.iter().filter(|b| **b != 128).count();
        println!("cursor pixels changed: {changed}");
        assert!(changed > 0);
    }
}
