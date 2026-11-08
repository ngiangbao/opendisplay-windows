# OpenDisplay control panel (GUI)

Start `opendisplay.exe` without arguments. Plain Win32 controls via native-windows-gui (no GPU/OpenGL, so it also runs
on VMs and machines without graphics drivers). Source: `linux/crates/opendisplay/src/gui.rs`.

```
+--------------------------------------------------------+
| OpenDisplay                                            |
| Use another screen as a monitor, or let another ...    |
| +----------------------------------------------------+ |
| | 1  Use this PC as a display                        | |
| | A Mac, Linux PC or another Windows PC sends ...    | |
| | [x] Open in a window (untick for fullscreen)       | |
| | o Status: stopped                         [ Start ]| |
| +----------------------------------------------------+ |
| +----------------------------------------------------+ |
| | 2  Extend this PC onto another screen              | |
| | Screen to share  [ New extended display (...)    v]| |
| | Send to          [ found receivers             v][Find]|
| | or type host:port[                                ]| |
| | Extended display: a new screen with the exact ...  | |
| | [ Install the free extended-display driver ]       | |
| | o Status: stopped                         [ Start ]| |
| +----------------------------------------------------+ |
| Activity                                               |
| +----------------------------------------------------+ |
| | [display] listening on 0.0.0.0:9000 ...            | |
| +----------------------------------------------------+ |
+--------------------------------------------------------+
```

- Two independent cards; both can run at the same time.
- Each mode is a child process of the same exe (`opendisplay receive ...`, `opendisplay send run ...`): the receiver keeps the
  main thread of its own process, a crash in one mode cannot take the panel down, Stop ends the child. Logs (stdout/stderr
  pipes) feed the Activity box; status text is derived from known log lines.
- "Screen to share": the first entry is the extended display (a virtual monitor sized to the receiver, needs the Virtual
  Display Driver); the others mirror or use an existing screen (`--output`).
- "Find" runs `opendisplay send list --json` (mDNS) and fills the list; a typed `host:port` wins over the list.
- The exe is a GUI-subsystem program; when started from a terminal with arguments it re-attaches to that console, so the CLI
  still prints (`attach_console` in main.rs). `opendisplay.manifest` enables Common Controls v6 and per-monitor DPI.

Not done: tray icon / minimise to tray, autostart, receiver status for several senders, translations (strings are English).
