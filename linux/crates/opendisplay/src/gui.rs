//! The OpenDisplay control panel (what you get when you start `opendisplay`
//! without arguments). Design notes: windows/DESIGN-gui.md.
//!
//! Two independent cards that can run at the same time:
//!   1. "Use this PC as a display"  -> runs `opendisplay receive` as a child process
//!   2. "Extend this PC onto another screen" -> runs `opendisplay send run ...`
//! plus a live log. Each mode is a child process of this same exe, so the
//! receiver's window keeps the main thread of its own process, a crash in one
//! mode never takes the panel down, and "Stop" is just ending the child.
//!
//! Plain Win32 controls (native-windows-gui): no GPU, no OpenGL, so it also
//! works on virtual machines and machines without graphics drivers.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use native_windows_derive as nwd;
use native_windows_gui as nwg;
use nwd::NwgUi;
use nwg::NativeUi;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_LOG_LINES: usize = 300;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Display,
    Extend,
}

impl Mode {
    fn tag(self) -> &'static str {
        match self {
            Mode::Display => "display",
            Mode::Extend => "extend",
        }
    }
}

#[derive(Default)]
struct Shared {
    log: VecDeque<String>,
    display_status: String,
    extend_status: String,
    display_running: bool,
    extend_running: bool,
    /// Receivers found by the last "Find": (label, address).
    found: Vec<(String, String)>,
    finding: bool,
}

#[derive(Default, NwgUi)]
pub struct App {
    #[nwg_resource(family: "Segoe UI", size: 26, weight: 700)]
    font_title: nwg::Font,
    #[nwg_resource(family: "Segoe UI", size: 19, weight: 700)]
    font_head: nwg::Font,
    #[nwg_resource(family: "Segoe UI", size: 17)]
    font_body: nwg::Font,
    #[nwg_resource(family: "Consolas", size: 15)]
    font_mono: nwg::Font,

    #[nwg_control(size: (620, 690), position: (200, 30), title: "OpenDisplay", flags: "WINDOW|VISIBLE")]
    #[nwg_events(OnWindowClose: [App::on_close])]
    window: nwg::Window,

    #[nwg_control]
    #[nwg_events(OnNotice: [App::on_notice])]
    notice: nwg::Notice,

    #[nwg_layout(parent: window, spacing: 4, margin: [14, 16, 14, 16])]
    layout: nwg::GridLayout,

    // ---- header ----
    #[nwg_control(text: "OpenDisplay", font: Some(&data.font_title))]
    #[nwg_layout_item(layout: layout, row: 0, col: 0, row_span: 2)]
    title: nwg::Label,

    #[nwg_control(text: "Use another screen as a monitor, or let another device use this PC's screen.", font: Some(&data.font_body))]
    #[nwg_layout_item(layout: layout, row: 2, col: 0)]
    subtitle: nwg::Label,

    // ---- card 1: this PC is the display ----
    #[nwg_control(flags: "VISIBLE|BORDER")]
    #[nwg_layout_item(layout: layout, row: 3, col: 0, row_span: 5)]
    card_display: nwg::Frame,

    #[nwg_layout(parent: card_display, spacing: 4, margin: [8, 10, 8, 10])]
    card_display_layout: nwg::GridLayout,

    #[nwg_control(parent: card_display, text: "1  Use this PC as a display", font: Some(&data.font_head))]
    #[nwg_layout_item(layout: card_display_layout, row: 0, col: 0, col_span: 3)]
    d_head: nwg::Label,

    #[nwg_control(parent: card_display, text: "A Mac, Linux PC or another Windows PC sends its screen here.\r\nOn the sender, pick this computer by its name.", font: Some(&data.font_body))]
    #[nwg_layout_item(layout: card_display_layout, row: 1, col: 0, col_span: 3, row_span: 2)]
    d_desc: nwg::Label,

    #[nwg_control(parent: card_display, text: "Open in a window (untick for fullscreen)", font: Some(&data.font_body), check_state: nwg::CheckBoxState::Checked)]
    #[nwg_layout_item(layout: card_display_layout, row: 3, col: 0, col_span: 3)]
    d_windowed: nwg::CheckBox,

    #[nwg_control(parent: card_display, text: "Status: stopped", font: Some(&data.font_body))]
    #[nwg_layout_item(layout: card_display_layout, row: 4, col: 0, col_span: 2)]
    d_status: nwg::Label,

    #[nwg_control(parent: card_display, text: "Start", font: Some(&data.font_head))]
    #[nwg_layout_item(layout: card_display_layout, row: 4, col: 2)]
    #[nwg_events(OnButtonClick: [App::on_display_toggle])]
    d_button: nwg::Button,

    // ---- card 2: extend this PC ----
    #[nwg_control(flags: "VISIBLE|BORDER")]
    #[nwg_layout_item(layout: layout, row: 8, col: 0, row_span: 7)]
    card_extend: nwg::Frame,

    #[nwg_layout(parent: card_extend, spacing: 4, margin: [8, 10, 8, 10])]
    card_extend_layout: nwg::GridLayout,

    #[nwg_control(parent: card_extend, text: "2  Extend this PC onto another screen", font: Some(&data.font_head))]
    #[nwg_layout_item(layout: card_extend_layout, row: 0, col: 0, col_span: 4)]
    e_head: nwg::Label,

    #[nwg_control(parent: card_extend, text: "Screen to share", font: Some(&data.font_body))]
    #[nwg_layout_item(layout: card_extend_layout, row: 1, col: 0)]
    e_screen_label: nwg::Label,

    #[nwg_control(parent: card_extend, font: Some(&data.font_body))]
    #[nwg_layout_item(layout: card_extend_layout, row: 1, col: 1, col_span: 3)]
    e_screen: nwg::ComboBox<String>,

    #[nwg_control(parent: card_extend, text: "Send to", font: Some(&data.font_body))]
    #[nwg_layout_item(layout: card_extend_layout, row: 2, col: 0)]
    e_target_label: nwg::Label,

    #[nwg_control(parent: card_extend, font: Some(&data.font_body))]
    #[nwg_layout_item(layout: card_extend_layout, row: 2, col: 1, col_span: 2)]
    e_found: nwg::ComboBox<String>,

    #[nwg_control(parent: card_extend, text: "Find", font: Some(&data.font_body))]
    #[nwg_layout_item(layout: card_extend_layout, row: 2, col: 3)]
    #[nwg_events(OnButtonClick: [App::on_find])]
    e_find: nwg::Button,

    #[nwg_control(parent: card_extend, text: "or type host:port", font: Some(&data.font_body))]
    #[nwg_layout_item(layout: card_extend_layout, row: 3, col: 0)]
    e_manual_label: nwg::Label,

    #[nwg_control(parent: card_extend, font: Some(&data.font_body))]
    #[nwg_layout_item(layout: card_extend_layout, row: 3, col: 1, col_span: 3)]
    e_manual: nwg::TextInput,

    #[nwg_control(parent: card_extend, text: "Extended display makes a new screen with the exact resolution of the other device.", font: Some(&data.font_body))]
    #[nwg_layout_item(layout: card_extend_layout, row: 4, col: 0, col_span: 4)]
    e_hint: nwg::Label,

    #[nwg_control(parent: card_extend, text: "Install the free extended-display driver", font: Some(&data.font_body))]
    #[nwg_layout_item(layout: card_extend_layout, row: 5, col: 0, col_span: 4)]
    #[nwg_events(OnButtonClick: [App::on_install])]
    e_install: nwg::Button,

    #[nwg_control(parent: card_extend, text: "Status: stopped", font: Some(&data.font_body))]
    #[nwg_layout_item(layout: card_extend_layout, row: 6, col: 0, col_span: 3)]
    e_status: nwg::Label,

    #[nwg_control(parent: card_extend, text: "Start", font: Some(&data.font_head))]
    #[nwg_layout_item(layout: card_extend_layout, row: 6, col: 3)]
    #[nwg_events(OnButtonClick: [App::on_extend_toggle])]
    e_button: nwg::Button,

    // ---- log ----
    #[nwg_control(text: "Activity", font: Some(&data.font_head))]
    #[nwg_layout_item(layout: layout, row: 15, col: 0)]
    log_head: nwg::Label,

    #[nwg_control(flags: "VISIBLE|VSCROLL|AUTOVSCROLL", readonly: true, font: Some(&data.font_mono))]
    #[nwg_layout_item(layout: layout, row: 16, col: 0, row_span: 5)]
    log_box: nwg::TextBox,

    #[nwg_control(text: "Unofficial Windows port of peetzweg/opendisplay (GPL-3.0)  -  github.com/ngiangbao/opendisplay-windows", font: Some(&data.font_body))]
    #[nwg_layout_item(layout: layout, row: 21, col: 0)]
    footer: nwg::Label,

    shared: Arc<Mutex<Shared>>,
    display_child: RefCell<Option<Child>>,
    extend_child: RefCell<Option<Child>>,
    monitors: RefCell<Vec<Option<String>>>,
}

impl App {
    fn exe() -> std::path::PathBuf {
        std::env::current_exe().unwrap_or_else(|_| "opendisplay.exe".into())
    }

    fn log(&self, line: &str) {
        let mut s = self.shared.lock().unwrap();
        push_log(&mut s, line);
    }

    // ---------------- display (receiver) ----------------
    fn on_display_toggle(&self) {
        if self.display_child.borrow().is_some() {
            self.stop(Mode::Display);
            return;
        }
        let mut args = vec!["receive".to_string()];
        if self.d_windowed.check_state() == nwg::CheckBoxState::Checked {
            args.push("--windowed".into());
        }
        self.spawn(Mode::Display, args);
    }

    // ---------------- extend (sender) ----------------
    fn on_extend_toggle(&self) {
        if self.extend_child.borrow().is_some() {
            self.stop(Mode::Extend);
            return;
        }
        let manual = self.e_manual.text().trim().to_string();
        let target = if !manual.is_empty() {
            manual
        } else {
            let s = self.shared.lock().unwrap();
            self.e_found
                .selection()
                .and_then(|i| s.found.get(i).map(|f| f.1.clone()))
                .unwrap_or_default()
        };
        let mut args = vec!["send".to_string(), "run".into()];
        if !target.is_empty() {
            args.push("--to".into());
            args.push(target);
        } else {
            args.push("--wait".into());
        }
        if let Some(i) = self.e_screen.selection() {
            if let Some(Some(name)) = self.monitors.borrow().get(i) {
                args.push("--output".into());
                args.push(name.clone());
            }
        }
        self.spawn(Mode::Extend, args);
    }

    fn on_install(&self) {
        let params = nwg::MessageParams {
            title: "Install extended-display driver",
            content: "OpenDisplay will install the free, open-source Virtual Display Driver (MIT, signed) with winget. Windows will ask for permission. Continue?",
            buttons: nwg::MessageButtons::YesNo,
            icons: nwg::MessageIcons::Question,
        };
        if nwg::modal_message(&self.window, &params) != nwg::MessageChoice::Yes {
            return;
        }
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        let r = Command::new("winget")
            .args([
                "install",
                "--id",
                "VirtualDrivers.Virtual-Display-Driver",
                "-e",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ])
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn();
        match r {
            Ok(_) => self.log("[extend] installing the virtual display driver (see the console window); press Start when it finishes"),
            Err(e) => self.log(&format!("[extend] could not run winget: {e}. Install it from github.com/VirtualDrivers/Virtual-Display-Driver/releases")),
        }
        self.notice.sender().notice();
    }

    fn on_find(&self) {
        {
            let mut s = self.shared.lock().unwrap();
            if s.finding {
                return;
            }
            s.finding = true;
            push_log(&mut s, "[extend] looking for receivers on the network...");
        }
        self.e_find.set_enabled(false);
        let shared = self.shared.clone();
        let sender = self.notice.sender();
        std::thread::spawn(move || {
            let out = Command::new(App::exe())
                .args(["send", "list", "--json", "--timeout", "3"])
                .env("NO_COLOR", "1")
                .creation_flags(CREATE_NO_WINDOW)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output();
            let mut found = Vec::new();
            if let Ok(out) = out {
                if let Ok(serde_json::Value::Array(items)) =
                    serde_json::from_slice::<serde_json::Value>(&out.stdout)
                {
                    for it in items {
                        let name = it["name"].as_str().unwrap_or("receiver");
                        let addr = it["addr"].as_str().unwrap_or("");
                        if !addr.is_empty() {
                            found.push((format!("{name}  ({addr})"), addr.to_string()));
                        }
                    }
                }
            }
            let mut s = shared.lock().unwrap();
            let n = found.len();
            s.found = found;
            s.finding = false;
            push_log(
                &mut s,
                &if n == 0 {
                    "[extend] no receivers found (is OpenDisplay open on the other device?)".to_string()
                } else {
                    format!("[extend] found {n} receiver(s)")
                },
            );
            drop(s);
            sender.notice();
        });
    }

    // ---------------- child processes ----------------
    fn spawn(&self, mode: Mode, args: Vec<String>) {
        let mut cmd = Command::new(Self::exe());
        cmd.args(&args)
            .env("NO_COLOR", "1")
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.log(&format!("[{}] could not start: {e}", mode.tag()));
                self.notice.sender().notice();
                return;
            }
        };
        self.log(&format!("[{}] started: {}", mode.tag(), args.join(" ")));
        {
            let mut s = self.shared.lock().unwrap();
            match mode {
                Mode::Display => {
                    s.display_running = true;
                    s.display_status = "starting...".into();
                }
                Mode::Extend => {
                    s.extend_running = true;
                    s.extend_status = "starting...".into();
                }
            }
        }
        let pid = child.id();
        let out = child.stdout.take();
        let err = child.stderr.take();
        match mode {
            Mode::Display => *self.display_child.borrow_mut() = Some(child),
            Mode::Extend => *self.extend_child.borrow_mut() = Some(child),
        }
        let mut readers = Vec::new();
        for stream in [out.map(Box::new).map(|b| b as Box<dyn std::io::Read + Send>), err.map(Box::new).map(|b| b as Box<dyn std::io::Read + Send>)]
            .into_iter()
            .flatten()
        {
            let shared = self.shared.clone();
            let sender = self.notice.sender();
            readers.push(std::thread::spawn(move || {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    let line = strip_ansi(&line);
                    if line.trim().is_empty() {
                        continue;
                    }
                    let mut s = shared.lock().unwrap();
                    apply_status(mode, &line, &mut s);
                    push_log(&mut s, &format!("[{}] {}", mode.tag(), line));
                    drop(s);
                    sender.notice();
                }
            }));
        }
        // When both pipes close the child is gone: mark it stopped.
        let shared = self.shared.clone();
        let sender = self.notice.sender();
        std::thread::spawn(move || {
            for r in readers {
                let _ = r.join();
            }
            let mut s = shared.lock().unwrap();
            match mode {
                Mode::Display => {
                    s.display_running = false;
                    s.display_status = "stopped".into();
                }
                Mode::Extend => {
                    s.extend_running = false;
                    s.extend_status = "stopped".into();
                }
            }
            push_log(&mut s, &format!("[{}] stopped (pid {pid})", mode.tag()));
            drop(s);
            sender.notice();
        });
        self.refresh();
    }

    fn stop(&self, mode: Mode) {
        let cell = match mode {
            Mode::Display => &self.display_child,
            Mode::Extend => &self.extend_child,
        };
        if let Some(mut c) = cell.borrow_mut().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        let mut s = self.shared.lock().unwrap();
        match mode {
            Mode::Display => {
                s.display_running = false;
                s.display_status = "stopped".into();
            }
            Mode::Extend => {
                s.extend_running = false;
                s.extend_status = "stopped".into();
            }
        }
        drop(s);
        self.refresh();
    }

    // ---------------- UI refresh ----------------
    fn on_notice(&self) {
        // A child that ended on its own (pipes closed) must be forgotten.
        {
            let s = self.shared.lock().unwrap();
            if !s.display_running {
                self.display_child.borrow_mut().take();
            }
            if !s.extend_running {
                self.extend_child.borrow_mut().take();
            }
        }
        self.refresh();
    }

    fn refresh(&self) {
        let s = self.shared.lock().unwrap();
        let dot = |running: bool| if running { "\u{25CF}" } else { "\u{25CB}" };
        self.d_status
            .set_text(&format!("{} Status: {}", dot(s.display_running), or_stopped(&s.display_status)));
        self.e_status
            .set_text(&format!("{} Status: {}", dot(s.extend_running), or_stopped(&s.extend_status)));
        self.d_button
            .set_text(if s.display_running { "Stop" } else { "Start" });
        self.e_button
            .set_text(if s.extend_running { "Stop" } else { "Start" });
        let have_driver = od_sender::vdd_installed();
        self.e_install.set_visible(!have_driver);
        self.e_hint.set_text(if have_driver {
            "Extended display: a new screen with the exact resolution of the other device."
        } else {
            "Extended display needs a free driver (one time). Mirroring works without it."
        });
        self.d_windowed.set_enabled(!s.display_running);
        self.e_screen.set_enabled(!s.extend_running);
        self.e_found.set_enabled(!s.extend_running);
        self.e_manual.set_enabled(!s.extend_running);
        self.e_find.set_enabled(!s.finding && !s.extend_running);

        // Found receivers (keep the user's selection if the list is unchanged).
        let labels: Vec<String> = s.found.iter().map(|f| f.0.clone()).collect();
        if labels != self.e_found.collection().clone() {
            self.e_found.set_collection(labels);
            if !s.found.is_empty() {
                self.e_found.set_selection(Some(0));
            }
        }
        let text = s.log.iter().cloned().collect::<Vec<_>>().join("\r\n");
        drop(s);
        if text != self.log_box.text() {
            self.log_box.set_text(&text);
            let len = self.log_box.len();
            self.log_box.set_selection(len..len);
            self.log_box.scroll_lastline();
        }
    }

    fn init(&self) {
        let list = od_sender::list_monitors();
        let mut names: Vec<Option<String>> = vec![None];
        let mut labels = vec!["New extended display (matches the other device)".to_string()];
        for m in &list {
            names.push(Some(m.name.clone()));
            labels.push(format!(
                "{} {}  {}x{} @ {:.0} Hz",
                if m.primary { "Mirror main screen" } else { "Use screen" },
                m.name,
                m.width,
                m.height,
                m.hz,
            ));
        }
        self.e_screen.set_collection(labels);
        self.e_screen.set_selection(Some(0));
        *self.monitors.borrow_mut() = names;
        let computer = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "this PC".into());
        self.log(&format!("OpenDisplay ready. This computer is \"{computer}\"."));
        self.refresh();
    }

    fn on_close(&self) {
        self.stop(Mode::Display);
        self.stop(Mode::Extend);
        nwg::stop_thread_dispatch();
    }
}

fn or_stopped(s: &str) -> &str {
    if s.is_empty() { "stopped" } else { s }
}

fn push_log(s: &mut Shared, line: &str) {
    s.log.push_back(line.to_string());
    while s.log.len() > MAX_LOG_LINES {
        s.log.pop_front();
    }
}

/// Map a child's log line to a human status.
fn apply_status(mode: Mode, line: &str, s: &mut Shared) {
    match mode {
        Mode::Display => {
            if line.contains("listening on") {
                s.display_status = "waiting for a sender".into();
            } else if line.contains("sender connected") {
                s.display_status = "connected".into();
            } else if line.contains("disconnected") {
                s.display_status = "waiting for a sender".into();
            }
        }
        Mode::Extend => {
            if line.contains("connected to receiver") {
                s.extend_status = "connecting...".into();
            } else if line.contains("Extending onto") {
                s.extend_status = "streaming".into();
            } else if line.contains("connection ended") {
                s.extend_status = "connection ended".into();
            } else if line.contains("ERROR") {
                s.extend_status = "error (see activity)".into();
            }
        }
    }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Open the control panel and run until it is closed.
pub fn run() {
    nwg::init().expect("failed to initialise the Windows UI");
    let _ = nwg::Font::set_global_family("Segoe UI");
    let app = App::build_ui(Default::default()).expect("failed to build the window");
    app.init();
    nwg::dispatch_thread_events();
}
