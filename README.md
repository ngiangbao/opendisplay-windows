# OpenDisplay for Windows

Windows port of [peetzweg/opendisplay](https://github.com/peetzweg/opendisplay) —
use spare devices as extra monitors, with the same wire protocol
([PROTOCOL.md](PROTOCOL.md)) so existing iPhone / iPad / Android / Mac / Linux
apps keep working.

> **Status: work in progress.** This repository starts as a copy of upstream
> (v1.30.2, GPL-3.0). The Windows work lives in the Rust workspace under
> [`linux/`](linux/), whose crates are mostly platform-neutral.

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
