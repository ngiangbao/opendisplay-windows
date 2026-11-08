# Windows sender — design (T2.1)

Status: proposal, 2026-11-08. Nothing here is implemented yet. Goal: `od-sender`
on Windows extends the desktop onto an OpenDisplay receiver (iPhone, iPad,
Android, Mac, Linux, or the Windows receiver from this repo) using the existing
wire protocol ([PROTOCOL.md](../PROTOCOL.md)). The protocol does not change.

## 1. What the sender needs (and what already exists)

The Linux sender splits cleanly (see `linux/crates/od-sender/src/backend/mod.rs`):

| Piece | Trait / module | Windows answer |
|---|---|---|
| Create a monitor sized for the receiver | `Backend::open(&DisplaySpec) -> VirtualDisplay`, `reconfigure`, `close` | §2 |
| Capture its frames (+ cursor) | `VirtualDisplay::start_capture(FrameSink)`, `start_cursor(CursorSink)` | §3 |
| Encode H.264 | `encoder/` (`Encoder`, `convert.rs` BGRA→I420) | §4 |
| Inject touch/scroll/pencil | `InputSink::{touch, scroll, pencil}` | §5 |
| Session loop, liveness, keyframes, cursor seq | `session.rs` + `opendisplay-session` | reuse unchanged |
| Discovery / config / lock | `discover.rs` (mdns-sd), `config.rs`, `backend/lock.rs` | reuse; fix paths |

So the Windows work is **one new backend** (`backend/windows/`) plus making the
crate compile on Windows. We do **not** fork the session loop.

### Compile blocker (do this first, T2.2)
`od-sender` depends unconditionally on `wayland-*`, `pipewire` (optional) and
`ffmpeg-next` (needs FFmpeg dev libs). On Windows:
- gate `wayland-*`, `zbus`, `pipewire` as `cfg(target_os = "linux")` target deps (same technique as T1.1);
- make `ffmpeg-next` Linux-only; Windows uses `openh264` first (already proven to build with MSVC in the receiver) and Media Foundation later;
- gate `mod wayland`, `pipewire`, the Linux backends, `tailscale`/`desktop` bits that shell out to Linux tools.
Acceptance: `cargo build -p od-sender` and its pure unit tests pass on `windows-latest` with no backend available (`doctor` says so).

## 2. The virtual monitor — options

| | Option | Pros | Cons |
|---|---|---|---|
| **D** | **Adopt an existing monitor** (`--backend adopt --output \\.\DISPLAY2`, or a monitor the user made with a driver/dummy plug) | No driver, no signing, no admin. Fastest path to a working end-to-end. Mirrors the Linux `adopt` backend. | The user must already have a spare monitor; resolution is not the receiver's. |
| **A** | **Use an installed, signed IddCx virtual display driver**, e.g. [VirtualDrivers/Virtual-Display-Driver](https://github.com/VirtualDrivers/Virtual-Display-Driver) (based on Microsoft's Indirect Display Driver sample) | Real second monitor on demand; no need to build or sign a driver ourselves. | Third-party dependency. Modes come from the driver's config, so a receiver-specific size means writing its settings and restarting the device (admin). Signing/licence statements below are unverified. |
| **B** | Parsec VDD ([nomi-san/parsec-vdd](https://github.com/nomi-san/parsec-vdd) wrapper) | Add/remove display at runtime via IOCTL from user mode. | Driver binary belongs to Parsec; redistribution rights unclear, so only "use if the user installed it". Limited custom modes. |
| **C** | Write our own IddCx driver from Microsoft's MIT sample | Full control: any mode per receiver, runtime add/remove via our own IOCTL. | Needs WDK, and **production signing** (EV certificate + Microsoft attestation). Biggest cost and legal/ops risk. |

**Recommendation**
1. **MVP = D.** Capture + encode + input end to end with an existing monitor. Zero driver risk, and everything else (T2.3–T2.5) is needed regardless.
2. **Then A as an optional integration**: `doctor` detects an installed IddCx driver; the backend asks it for a mode matching the receiver, then places it with `ChangeDisplaySettingsEx`/`SetDisplayConfig`. If a mode is missing we write the driver's settings file and restart the device, and tell the user it needs admin.
3. **C only if A's limits hurt** (arbitrary panel sizes, no restart). Do not start it without a signing plan.

What I could verify by search (low confidence, from forks and mirrors, not the official release page): Virtual-Display-Driver is described as MIT-licensed, built on Microsoft's sample, and some forks credit SignPath for signing while others still tell users to trust a root certificate; ARM64 on Windows 11 24H2 may need test signing. **Before relying on A, read the official repo's LICENSE and release notes.** For B, an MIT wrapper does not license Parsec's driver binary, so never bundle it.

Sources: [Virtual-Display-Driver mirrors](https://github.com/itsmikethetech/Virtual-Display-Driver), [driver team page](https://github.com/VirtualDrivers/Virtual-Display-Driver/discussions/515), [parsec-vdd](https://github.com/nomi-san/parsec-vdd), [ParsecVDisplay on SignPath](https://signpath.org/projects/parsecvdisplay).

## 3. Capture

- **DXGI Desktop Duplication** (`IDXGIOutput1::DuplicateOutput`) — recommended primary.
  It targets one output, reports **pointer position and shape separately**, which maps directly onto the protocol's cursor side channel (§6.2) so the cursor need not be burned into the video; handles dirty rects.
  Constraints: the D3D11 device must be created on the **adapter that owns the output** (laptops with iGPU+dGPU fail otherwise); cannot capture the secure desktop/UAC prompt; `DXGI_ERROR_ACCESS_LOST` on mode change or lock must trigger re-create; the process should be per-monitor-v2 DPI aware.
- **Windows.Graphics.Capture** — fallback (`IGraphicsCaptureItemInterop::CreateForMonitor`). Easier on hybrid GPUs and Windows 11 can hide the yellow border, but no separate cursor shape.
- Frames arrive as BGRA D3D11 textures. First version copies to a staging texture and maps to CPU (`Pixels::Owned`); a later zero-copy path hands the texture to a Media Foundation encoder.
- Crate: `windows` (windows-rs) with `Win32_Graphics_Dxgi`, `Win32_Graphics_Direct3D11`.

## 4. Encode

1. **OpenH264 (CPU)** via the existing `encoder/openh264.rs` + `convert.rs` — works today on MSVC, good enough for MVP up to roughly 1080p30.
2. **Media Foundation H.264 encoder MFT** (hardware through NVENC/Quick Sync/AMF, async MFT with D3D11 input) — the performance step. Low-latency settings: CBR, no B-frames, `CODECAPI_AVLowLatencyMode`, GOP on demand for keyframe requests.
3. HEVC later (receiver side must also decode it).
No FFmpeg on Windows: avoids shipping DLLs and a GPL/LGPL build matrix.

## 5. Input

`InputSink` → `SendInput`:
- `touch` (normalised to the virtual monitor) → move to absolute coordinate (`MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK`, converted with the monitor rectangle from `GetMonitorInfo`), `Began`→left down, `Ended`→left up, `Moved`→move.
- `scroll` → `MOUSEEVENTF_WHEEL` / `HWHEEL` (sign per `input.rs` conventions already used by the receiver).
- `pencil` → optional later via the Touch/Pen injection APIs (`InitializePointerDeviceInjection`).
- Limits: UIPI blocks injection into elevated windows unless the sender runs elevated; document it.

## 6. Other platform details

- Config/state dirs under `%APPDATA%\OpenDisplay`; lock file in `%LOCALAPPDATA%` (use a named mutex instead of pid file).
- Ctrl+C / console close → remove the virtual monitor (Windows has no `SIGHUP`; use `SetConsoleCtrlHandler`).
- Firewall: sender dials out, so no inbound rule; mDNS browse needs UDP 5353 inbound on private networks.
- Packaging: second binary in the existing Inno Setup installer; later a tray app.

## 7. Tasks (replaces T2.2–T2.6; claim by issue/PR)

- [ ] **T2.2** Make `od-sender` compile on Windows (gating, §1). Acceptance: CI job builds it.
- [ ] **T2.3** `backend/windows`: `Adopt`-style backend on a chosen `\\.\DISPLAYn`, enumerate monitors (`EnumDisplayMonitors`), `doctor` output.
- [ ] **T2.4** Desktop Duplication capture + cursor events into `FrameSink`/`CursorSink`; recover from `ACCESS_LOST`; pick the right adapter.
- [ ] **T2.5** OpenH264 path end to end on Windows: `od-sender run --to <receiver>` streams a real monitor to the Windows receiver on the same PC pair (2 machines) or loopback.
- [ ] **T2.6** `InputSink` via `SendInput`.
- [ ] **T2.7** Media Foundation hardware encoder.
- [ ] **T2.8** Optional IddCx driver integration (option A), after reading its licence/signing.
- [ ] **T2.9** Ctrl handler + named-mutex lock + `%APPDATA%` config, installer entry, release.

## 8. Open questions for the maintainer

1. Is requiring a pre-existing second monitor (or a driver the user installs) acceptable for the first release? (The recommendation says yes.)
2. Is there budget for an EV certificate? Without it, option C is off the table and the installer stays unsigned.
3. Minimum Windows version: Desktop Duplication works on Windows 8+, WGC needs 10 1903+. Proposal: Windows 10 21H2+.
