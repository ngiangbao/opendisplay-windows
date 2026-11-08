# Windows port — progress notes (for contributors and other agents)

Last updated: 2026-11-08. Upstream base: peetzweg/opendisplay v1.30.2 (GPL-3.0).
Read [AGENTS.md](AGENTS.md) (upstream conventions) and [PROTOCOL.md](PROTOCOL.md) first.

## Done
- Repo created from upstream snapshot (no upstream history), README with goals.
- Surveyed `linux/` Rust workspace (see "Findings").
- Rust 1.99.1 and MSVC Build Tools installed; the portable Rust baseline passes
  all 54 tests on Windows MSVC.

## Findings
| Crate | Windows readiness |
|---|---|
| `opendisplay-proto` (1.8k LOC) | Sans-I/O, deps: serde/thiserror only. Expected to build as is. |
| `opendisplay-session` (1.0k LOC) | Sans-I/O state machines. Expected to build as is. |
| `od-fake-sender` | Test helper (tokio TCP). Likely portable. |
| `od-receiver` (2.6k LOC) | Builds on Windows with `--no-default-features`; native monitor enumeration supplies physical pixels, refresh, and DPI scale. Linux-only deps remain `wayland-client`, `gtk4`+`gst-plugin-gtk4`, and `zbus`/logind. `--sink none` supports protocol testing. |
| `od-sender` (18k LOC) | Wayland/Hyprland/KDE/GNOME/PipeWire specific. Needs a new Windows backend; reusable: `session.rs` logic, `encoder/*` (openh264/x264), `discover.rs`, `config.rs` (check paths). |
| `Mac/ iOS/ Android/ MacReceiver/` | Untouched; not part of the port. |

## Plan / task board
Claim a task by opening an issue or PR titled with the task id. Keep PRs small.

### Phase 0 — toolchain & baseline
- [x] **T0.1** (done 2026-11-08: 54 tests pass on Windows MSVC) Finish MSVC Build Tools install; `cd linux && cargo test -p opendisplay-proto -p opendisplay-session -p od-fake-sender` passes on Windows.
- [x] **T0.2** Add a GitHub Actions `windows-latest` job running the above.

### Phase 1 — Windows receiver (PC as display)
- [x] **T1.1** (done 2026-11-08) `cargo build/test -p od-receiver --no-default-features` works on Windows; `wayland-client`/`zbus` are Linux-only target deps, `outputs::enumerate` has a non-Linux stub. `od-receiver --sink none --width 1920 --height 1080` listens on :9000 and advertises mDNS. Next: T1.2/T1.3.
- [x] **T1.2** (done 2026-11-08) Windows `outputs.rs` enumerates active monitors with Win32, including device name, physical pixels, refresh rate, and effective DPI scale; `--output` selects the panel values announced to the sender.
- [x] **T1.3** (done 2026-11-08) `--sink soft` (feature `winui`, default on Windows): OpenH264 software decode + winit/softbuffer window, `crates/od-receiver/src/win_ui.rs`. Verified end to end with `od-fake-sender` (180 frames, 0 decode errors, ~2.5 ms decode). H.264 only (no HEVC), CPU decode, no GPU path yet. Follow-ups: GPU decode (Media Foundation/D3D11) and HEVC.
- [x] **T1.4** (done, mouse only) left-button drag -> touch, wheel -> scroll via `input.rs`. TODO: real touchscreen/pen events, cursor overlay from the sender cursor channel.
- [ ] **T1.5** Power events (`WM_POWERBROADCAST`) in place of logind (`power.rs`).
- [~] **T1.6** installer done: `windows/build-installer.ps1` builds the single-exe static-CRT release and an Inno Setup installer (`windows/opendisplay.iss`) (optional firewall rule). Not yet: code signing, auto-start, tray icon, no console window, CI release workflow.

### Phase 3 — one app
- [x] **T3.1** (done 2026-11-09) single exe: new crate `linux/crates/opendisplay` (Windows-only, empty stub elsewhere) calls `od_receiver::cli::run_cli` and `od_sender::run_cli` in-process. How: `od-sender` is now lib + thin bin (`src/lib.rs` holds the old main.rs); `od-receiver` compiles its `main.rs` twice, as the bin and as `pub mod cli` in the lib (`#[path = "main.rs"]` + `extern crate self as od_receiver;`), so keep `run_cli` and avoid `std::process::exit` inside it.
- [~] **T3.2** (done 2026-11-09, first version) control panel GUI: native-windows-gui window with two cards (use this PC as a display / extend this PC) that can run together, live activity log, found-receivers list, driver install button. Design: [windows/DESIGN-gui.md](windows/DESIGN-gui.md). Verified: window opens, Start launches the child `opendisplay receive`, status reads "waiting for a sender". Not done: tray icon, autostart, translations, app icon.
- [ ] **T3.3** Installer/runtime smoke test on a clean Windows VM or Windows Sandbox (not done yet; only checked with a scrubbed environment on the dev machine).

### Phase 2 — Windows sender (extend PC to iPhone/iPad/Android/Mac)
- [x] **T2.1** (done 2026-11-08) design written: [windows/DESIGN-sender.md](windows/DESIGN-sender.md). Recommendation: MVP adopts an existing monitor (no driver), DXGI Desktop Duplication + OpenH264 + SendInput; IddCx virtual display driver is an optional later step. Revised task list T2.2-T2.9 is in that file (it replaces the T2.2-T2.6 lines below). Open questions for the maintainer are at its end.
- [x] **T2.2** (done 2026-11-09) `od-sender` builds and its 78 unit tests pass on Windows (`cargo test -p od-sender --no-default-features`). How: Linux-only deps moved to `[target.'cfg(target_os = "linux")'.dependencies]`; Wayland/PipeWire/D-Bus modules and the FFmpeg encoders are `cfg(target_os = "linux")`; Windows stand-ins: `backend/detect_win.rs` (always "unsupported" until T2.3), `doctor_win.rs`, `encoder/nostage.rs` (no convert stage, OpenH264 only), Windows branches in `backend/lock.rs` (share-mode lock in `%LOCALAPPDATA%OpenDisplay`), `disconnect.rs` (taskkill), `session.rs` (Ctrl+C/Break/close), `config.rs` (`%APPDATA%OpenDisplaysender.toml`). **Not verified on Linux** (no Linux machine here): the Linux paths are unchanged code behind `cfg(linux)`, but please run `cargo test -p od-sender` on Linux if you can. NEXT: T2.3 (`backend/windows`, replace `detect_win.rs`).
- [x] **T2.3** (done 2026-11-09) `linux/crates/od-sender/src/backend/win/`: adopts a monitor (`--output \.DISPLAY2`, default first non-primary, else the primary with a warning). `detect_win.rs` builds it; `doctor` lists monitors. No virtual monitor yet (option A/C in the design).
- [x] **T2.4** (done) DXGI Desktop Duplication capture (`win/capture.rs`): CPU copy, re-sends the last frame each second when idle, re-opens on ACCESS_LOST. TODO: cursor (not painted yet; start_cursor errs so the session falls back to painting, which the backend ignores), rotated monitors, zero-copy GPU path.
- [x] **T2.5** (done, verified) sender -> receiver on one PC: `opendisplay receive --windowed` + `opendisplay send run --to 127.0.0.1:9000` streamed 1080p with 0 decode errors (debug build: encode p50 ~77 ms, so only ~10 fps; measure a release build, then T2.7 hardware encode).
- [x] **T2.6** (done, unverified on a real touch device) `win/input.rs`: touch -> absolute mouse move + left button, scroll -> wheel/hwheel via SendInput; UIPI limits apply.

## Release
**v0.2.0** ships one installer, `OpenDisplay-Setup-0.2.0.exe`, containing ONE executable `opendisplay.exe` (receiver + sender). `opendisplay` = receiver window; `opendisplay receive [opts]`; `opendisplay send [opts]` (sender: no display backend yet, see T2.3). v0.1.0-receiver (two-part, receiver only) is superseded. Unsigned, so SmartScreen warns. Rebuild: `windowsuild-installer.ps1` (Rust, MSVC Build Tools, Inno Setup 6). The exe imports only system DLLs (static CRT), requires Windows 10+ x64 (ARM64 runs it under emulation).

## Review log (2026-11-09, review of the other agent's T0.2/T1.2 work + portability audit)
- T1.2 monitor enumeration (`outputs.rs`, windows-sys): reviewed, unsafe blocks are sound; GetDpiForMonitor needs Windows 8.1+. Fixed: monitors are now sorted primary-first (previously `pick(None)` could choose a secondary monitor while the fullscreen window opens on the primary).
- CI (`.github/workflows/windows.yml`): fine; action versions exist.
- Portability bug found and fixed (only showed on the dev machine because Git Bash sets HOME): `id.rs` fell back to `/tmp` when HOME/XDG were unset, which fails for normal users on Windows. Now uses `%LOCALAPPDATA%OpenDisplay`. Hostname now falls back to `COMPUTERNAME`, `hello.device` defaults to "Windows".
- Rule for everyone: test with `env -u HOME -u XDG_STATE_HOME` (or from cmd.exe) before claiming Windows support; never rely on HOME, /tmp, /etc, /proc.

## Reported by the maintainer (2026-11-09)
- "Extend does not work, and iPad aspect ratio is wrong": cause was adopt-only mode (mirrors the one monitor at its own 16:9 size). Fixed by T2.8 (virtual monitor sized to the receiver's panel, so the iPad's ratio is native). Needs confirmation on a real iPad.

## Known issues
- `tests/e2e` `silent_newcomers_leave_a_streaming_sender_alone` is timing-flaky (off by one frame when the machine is busy); it passed on reruns.
- Windows sender (Phase 2) is **not started**; it needs a virtual display driver and is the main open work.
- Never run the installer/firewall steps on someone else machine without telling them.

## How to build (Windows)
```powershell
winget install Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
cd linux
$env:CARGO_TARGET_DIR="C:odt"   # short path avoids LNK1104 (MAX_PATH) in deep checkouts
cargo test -p opendisplay-proto -p opendisplay-session -p od-fake-sender
```

## Conventions
- Keep GPL-3.0 headers/attribution; do not remove upstream credits.
- Do not change the wire protocol; receivers must interoperate with upstream apps.
- Prefer `cfg` gating over forking files, so upstream merges stay easy.
- Record decisions and blockers in this file (append to "Log").

## Current handoff
- **In progress: T1.5** — route Windows `WM_POWERBROADCAST` suspend events from
  the winit window into the receiver's existing `PowerEvent::Sleeping` channel.
  T2.2 is complete; T2.3 remains unclaimed.

## Log
- 2026-11-08: T1.2 done. Native smoke detected `\\.\DISPLAY1` at 1920x1080,
  144 Hz, scale 1; receiver advertised and listened with those panel values.
  Windows CI now also tests `od-receiver --no-default-features`.
- 2026-11-08: added `.github/workflows/windows.yml` to run the portable-crate
  baseline continuously on `windows-latest` (T0.2).
- 2026-11-08: T1.1 done (receiver builds/tests on Windows with --no-default-features; sink none runs). Files are CRLF in Windows checkouts: use sed tolerant of \r. No python on the dev machine.
- 2026-11-08: MSVC Build Tools installed; T0.1 done. Tip: deep checkout paths cause LNK1104, set CARGO_TARGET_DIR to a short path. Repo made public.
- 2026-11-08: repo created; Rust installed; MSVC install pending; survey written.
- 2026-11-08: T1.3 + T1.4 (mouse) done, installer script added, v0.1.0-receiver prerelease published. Open: T1.5 power events, T1.4 touch/pen follow-up, and remaining T1.6 work.
- 2026-11-08: T2.1 design doc added (`windows/DESIGN-sender.md`). Next unclaimed: T2.2 (make `od-sender` compile on Windows).
- 2026-11-09: T2.2 done (od-sender compiles + tests on Windows, CI step added). `od-sender doctor` runs and reports no backend; `run` exits 2 with a pointer to T2.3. Next unclaimed: T2.3.
- 2026-11-09: reviewed the other agent's commits (see Review log), fixed state-dir bug; T3.1 single exe done; v0.2.0 installer built. Heads-up for whoever edits `od-receiver/src/main.rs` (T1.5): it is also compiled into the lib as `cli`; keep it free of `process::exit`.
- 2026-11-09: T2.3-T2.6 done: Windows sender MVP streams a monitor to a receiver (verified loopback). Open: cursor, release-build perf, T2.7 hardware encode, T2.8 virtual display driver, T2.9 polish, T3.2 GUI.
- 2026-11-09: GUI (T3.2) and extended display via Virtual Display Driver (T2.8, unverified with the driver) added; installer v0.3.0 has an optional driver install task (winget). The other agent's T1.x work is still in progress (a Copilot session was seen working in the same repo): pull before editing `outputs.rs` / `win_ui.rs`.
