# OpenDisplay for Windows

Windows port of [peetzweg/opendisplay](https://github.com/peetzweg/opendisplay) —
use spare devices as extra monitors, with the same wire protocol
([PROTOCOL.md](PROTOCOL.md)) so existing iPhone / iPad / Android / Mac / Linux
apps keep working.

> **Status: alpha, one app with a control panel.** `opendisplay.exe` opens a window with two cards: use this PC as a display, or extend this PC onto another device (the extended screen at the other device's own resolution needs the free Virtual Display Driver; not yet verified with a real iPad). See [PORTING.md](PORTING.md).

## Quick start (Windows) / Hướng dẫn nhanh

1. Download `OpenDisplay-Setup-*.exe` from [Releases](https://github.com/ngiangbao/opendisplay-windows/releases) and run it (Windows 10 or later, 64-bit). Windows SmartScreen will warn because the installer is not code-signed: choose *More info → Run anyway*. Tick the firewall option, and the *Virtual Display Driver* option if you want an extended screen.
   Tải file cài đặt ở mục Releases và chạy (Windows 10 trở lên, 64-bit). SmartScreen sẽ cảnh báo vì chưa ký số: chọn *More info → Run anyway*.
2. Open **OpenDisplay** from the Start menu. One window, two cards:
   - **Use this PC as a display** (*Dùng PC này làm màn hình phụ*): press **Start**; on the Mac/Linux sender pick this computer by name.
   - **Extend this PC onto another screen** (*Mở rộng màn hình PC sang thiết bị khác*): press **Find** to list receivers (iPhone/iPad/Android/Mac/Linux/another Windows PC running OpenDisplay) or type `host:port`, then **Start**. "New extended display" creates a screen with the *exact resolution of the other device* (needs the free Virtual Display Driver, one-time install button inside the app); "Mirror main screen" just copies your screen.
3. Command line (same exe): `opendisplay receive --help`, `opendisplay send run --to <name|host:port>`, `opendisplay send doctor`.

Status: alpha. The Windows ↔ Windows path is tested; extended display with the real driver and an iPad is **not yet verified**. Contributors and other agents: read [PORTING.md](PORTING.md).

## Goals

| Role | What it means | Plan |
|---|---|---|
| **Windows receiver** | A Windows PC becomes the display for a Mac/Linux sender | Port `od-receiver`: keep `opendisplay-proto` / `opendisplay-session` / mDNS (already portable), replace the Wayland/GTK4/logind parts with a Windows presenter (GStreamer or Media Foundation decode, Win32/winit window, input) |
| **Windows sender** | Extend the Windows desktop onto an iPhone / iPad / Android / Mac | New `od-sender` backend: virtual monitor via an Indirect Display Driver (IddCx), capture via Desktop Duplication / Windows.Graphics.Capture, H.264 encode via Media Foundation/NVENC/x264, input via `SendInput` |

## Layout

- `linux/crates/opendisplay-proto` — wire protocol (sans-I/O, portable)
- `linux/crates/opendisplay-session` — session state machines (portable)
- `linux/crates/od-receiver` — receiver (Linux today; Windows target in progress)
- `linux/crates/od-sender` — sender (Wayland-specific today; Windows backend planned)
- `Mac/`, `iOS/`, `Android/`, `MacReceiver/` — untouched upstream apps
- [`PORTING.md`](PORTING.md) — progress notes and task board for contributors/agents
- `README.upstream.md` — original upstream README

## Building (Windows)

Requires Rust ≥ 1.85 (`winget install Rustlang.Rustup`).

```powershell
cd linux
cargo test -p opendisplay-proto -p opendisplay-session
```

## License & credits

GPL-3.0-only, same as upstream. All credit for the protocol and original apps
goes to [peetzweg](https://github.com/peetzweg) and contributors; see
[LICENSE](LICENSE). This is an unofficial fork, not affiliated with upstream.
