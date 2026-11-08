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
| `od-receiver` (2.6k LOC) | Linux-only deps: `wayland-client` (`outputs.rs`), `gtk4`+`gst-plugin-gtk4` (`gtk_ui.rs`), `zbus`/logind (`power.rs`). `lib.rs`, `discovery.rs` (mdns-sd), `stats.rs`, `id.rs` are portable. `--sink none` already exists for protocol testing. |
| `od-sender` (18k LOC) | Wayland/Hyprland/KDE/GNOME/PipeWire specific. Needs a new Windows backend; reusable: `session.rs` logic, `encoder/*` (openh264/x264), `discover.rs`, `config.rs` (check paths). |
| `Mac/ iOS/ Android/ MacReceiver/` | Untouched; not part of the port. |

## Plan / task board
Claim a task by opening an issue or PR titled with the task id. Keep PRs small.

### Phase 0 — toolchain & baseline
- [x] **T0.1** (done 2026-11-08: 54 tests pass on Windows MSVC) Finish MSVC Build Tools install; `cd linux && cargo test -p opendisplay-proto -p opendisplay-session -p od-fake-sender` passes on Windows.
- [x] **T0.2** Add a GitHub Actions `windows-latest` job running the above.

### Phase 1 — Windows receiver (PC as display)
- [x] **T1.1** (done 2026-11-08) `cargo build/test -p od-receiver --no-default-features` works on Windows; `wayland-client`/`zbus` are Linux-only target deps, `outputs::enumerate` has a non-Linux stub. `od-receiver --sink none --width 1920 --height 1080` listens on :9000 and advertises mDNS. Next: T1.2/T1.3.
- [ ] **T1.2** (claimed by another agent, see "In progress" below) native Windows monitor enumeration for `outputs::enumerate` and `--output`. FYI the `winui` path already reads the primary monitor through winit (`win_ui::primary_monitor`), which is enough for the single-monitor case.
- [x] **T1.3** (done 2026-11-08) `--sink soft` (feature `winui`, default on Windows): OpenH264 software decode + winit/softbuffer window, `crates/od-receiver/src/win_ui.rs`. Verified end to end with `od-fake-sender` (180 frames, 0 decode errors, ~2.5 ms decode). H.264 only (no HEVC), CPU decode, no GPU path yet. Follow-ups: GPU decode (Media Foundation/D3D11) and HEVC.
- [x] **T1.4** (done, mouse only) left-button drag -> touch, wheel -> scroll via `input.rs`. TODO: real touchscreen/pen events, cursor overlay from the sender cursor channel.
- [ ] **T1.5** Power events (`WM_POWERBROADCAST`) in place of logind (`power.rs`).
- [~] **T1.6** installer done: `windows/build-installer.ps1` builds a static-CRT release and an Inno Setup installer (optional firewall rule). Not yet: code signing, auto-start, tray icon, no console window, CI release workflow.

### Phase 2 — Windows sender (extend PC to iPhone/iPad/Android/Mac)
- [ ] **T2.1** Design doc: virtual display options (IddCx indirect display driver, or integrate an existing signed driver such as Virtual Display Driver / parsec-vdd). Driver signing is the main risk — decide here.
- [ ] **T2.2** `od-sender` backend trait for Windows (`backend/windows/`): create/remove virtual monitor sized to receiver; position next to primary.
- [ ] **T2.3** Capture: Windows.Graphics.Capture or DXGI Desktop Duplication of that monitor, with cursor.
- [ ] **T2.4** Encode: reuse openh264/x264 first; later Media Foundation / NVENC hardware.
- [ ] **T2.5** Input injection from receiver touch/scroll via `SendInput`.
- [ ] **T2.6** `od-sender doctor` for Windows; config path under `%APPDATA%`.

## Release
Prerelease **v0.1.0-receiver** ships `OpenDisplay-Receiver-Setup-0.1.0.exe` (receiver only; unsigned, so SmartScreen will warn). Rebuild with `windowsuild-installer.ps1` (needs Rust, MSVC Build Tools, Inno Setup 6).

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
- **In progress: T1.2** — add native Windows monitor enumeration (device name,
  physical pixels, refresh rate, and effective DPI scale) to `outputs.rs`.
  T0.2 is complete: `.github/workflows/windows.yml` continuously tests the
  portable crates on `windows-latest`.

## Log
- 2026-11-08: added `.github/workflows/windows.yml` to run the portable-crate
  baseline continuously on `windows-latest` (T0.2).
- 2026-11-08: T1.1 done (receiver builds/tests on Windows with --no-default-features; sink none runs). Files are CRLF in Windows checkouts: use sed tolerant of \r. No python on the dev machine.
- 2026-11-08: MSVC Build Tools installed; T0.1 done. Tip: deep checkout paths cause LNK1104, set CARGO_TARGET_DIR to a short path. Repo made public.
- 2026-11-08: repo created; Rust installed; MSVC install pending; survey written.
- 2026-11-08: T1.3 + T1.4 (mouse) done, installer script added, v0.1.0-receiver prerelease published. Open: T1.2 monitor picking (claimed by another agent), T1.5 power events, all of Phase 2.
