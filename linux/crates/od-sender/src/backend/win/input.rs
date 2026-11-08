//! Receiver touch and scroll become mouse input on the adopted monitor
//! through `SendInput`. Windows blocks injection into elevated windows
//! (UIPI) unless this process is elevated too.

use opendisplay_proto::control::{Scroll, Touch, TouchPhase};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK,
    MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN,
};

use super::monitors::Monitor;
use crate::backend::InputSink;

pub struct WinInput {
    monitor: Monitor,
    down: bool,
}

impl WinInput {
    pub fn new(monitor: Monitor) -> Self {
        Self {
            monitor,
            down: false,
        }
    }

    fn send(&self, flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: i32) {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: data as u32,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        // SAFETY: one fully initialised INPUT and its true size.
        unsafe {
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        }
    }

    /// Normalised position on the monitor to SendInput's 0..65535 range
    /// across the whole virtual desktop.
    fn absolute(&self, nx: f64, ny: f64) -> (i32, i32) {
        // SAFETY: GetSystemMetrics has no preconditions.
        let (vx, vy, vw, vh) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2),
                GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2),
            )
        };
        let px = self.monitor.x as f64 + nx.clamp(0.0, 1.0) * (self.monitor.width as f64 - 1.0);
        let py = self.monitor.y as f64 + ny.clamp(0.0, 1.0) * (self.monitor.height as f64 - 1.0);
        let ax = ((px - vx as f64) * 65535.0 / (vw as f64 - 1.0)).round() as i32;
        let ay = ((py - vy as f64) * 65535.0 / (vh as f64 - 1.0)).round() as i32;
        (ax, ay)
    }

    fn move_to(&self, nx: f64, ny: f64) {
        let (ax, ay) = self.absolute(nx, ny);
        self.send(
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            ax,
            ay,
            0,
        );
    }
}

impl InputSink for WinInput {
    fn touch(&mut self, t: &Touch) {
        self.move_to(t.x, t.y);
        match t.phase {
            TouchPhase::Began => {
                self.down = true;
                self.send(MOUSEEVENTF_LEFTDOWN, 0, 0, 0);
            }
            TouchPhase::Moved => {}
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if self.down {
                    self.down = false;
                    self.send(MOUSEEVENTF_LEFTUP, 0, 0, 0);
                }
            }
        }
    }

    /// `scroll` deltas are video pixels with natural sign: positive `dy` =
    /// content moves down = wheel up (positive WHEEL_DELTA, 120 per notch);
    /// positive `dx` = content moves right = HWHEEL negative.
    fn scroll(&mut self, s: &Scroll) {
        let dy = s.dy.round() as i32;
        let dx = (-s.dx).round() as i32;
        if dy != 0 {
            self.send(MOUSEEVENTF_WHEEL, 0, 0, dy);
        }
        if dx != 0 {
            self.send(MOUSEEVENTF_HWHEEL, 0, 0, dx);
        }
    }
}

impl Drop for WinInput {
    fn drop(&mut self) {
        // Never leave the button held if the session ends mid-touch.
        if self.down {
            self.send(MOUSEEVENTF_LEFTUP, 0, 0, 0);
        }
    }
}
