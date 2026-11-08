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
- [ ] **T1.2** Replace `outputs.rs` (Wayland output size/scale) with a Windows implementation (monitor size, DPI via `windows` crate).
- [ ] **T1.3** Windows presenter: window + H.264/HEVC decode. Candidates: GStreamer (`d3d11h264dec` + `d3d11videosink`) via the existing `video.rs`, or Media Foundation. Fullscreen on chosen monitor.
- [ ] **T1.4** Pointer/touch input → protocol touch/scroll messages (`input.rs`).
- [ ] **T1.5** Power events (`WM_POWERBROADCAST`) in place of logind (`power.rs`).
- [ ] **T1.6** Bonjour/mDNS advertise check on Windows firewall; document firewall prompt; installer or zip release.

### Phase 2 — Windows sender (extend PC to iPhone/iPad/Android/Mac)
- [ ] **T2.1** Design doc: virtual display options (IddCx indirect display driver, or integrate an existing signed driver such as Virtual Display Driver / parsec-vdd). Driver signing is the main risk — decide here.
- [ ] **T2.2** `od-sender` backend trait for Windows (`backend/windows/`): create/remove virtual monitor sized to receiver; position next to primary.
- [ ] **T2.3** Capture: Windows.Graphics.Capture or DXGI Desktop Duplication of that monitor, with cursor.
- [ ] **T2.4** Encode: reuse openh264/x264 first; later Media Foundation / NVENC hardware.
- [ ] **T2.5** Input injection from receiver touch/scroll via `SendInput`.
- [ ] **T2.6** `od-sender doctor` for Windows; config path under `%APPDATA%`.

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
