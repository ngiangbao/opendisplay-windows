//! Power events the receiver tells the sender about (§6.1 `sleeping`).
//! The receiver loop only sees [`PowerEvent`]s on a channel; the logind
//! watcher below feeds it in the binary, tests feed it directly, so no test
//! touches the system bus.

/// The machine is about to sleep: say `sleeping` and close, the sender
/// redials after wake.
#[derive(Debug)]
pub enum PowerEvent {
    Sleeping {
        /// Dropped (or sent) once `sleeping` went out, which lets the
        /// watcher release its delay lock and the machine suspend.
        done: Option<std::sync::mpsc::Sender<()>>,
    },
}

/// Watch logind's `PrepareForSleep` on the system bus. Holds a `delay`
/// sleep inhibitor so `sleeping` reaches the sender before the suspend, and
/// re-takes it after every wake. Runs on its own thread; logs and stops if
/// there is no system bus or logind.
#[cfg(feature = "logind")]
pub fn watch_logind(tx: tokio::sync::mpsc::UnboundedSender<PowerEvent>) {
    use std::time::Duration;
    use tracing::{debug, info, warn};

    let run = move || -> zbus::Result<()> {
        let conn = zbus::blocking::Connection::system()?;
        let manager = zbus::blocking::Proxy::new(
            &conn,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )?;
        let inhibit = || -> zbus::Result<zbus::zvariant::OwnedFd> {
            manager.call(
                "Inhibit",
                &(
                    "sleep",
                    "OpenDisplay Receiver",
                    "Tell the sender the display is going to sleep",
                    "delay",
                ),
            )
        };
        let mut lock = Some(inhibit()?);
        info!("watching logind for sleep");
        for msg in manager.receive_signal("PrepareForSleep")? {
            let going: bool = msg.body().deserialize()?;
            if going {
                let (done_tx, done_rx) = std::sync::mpsc::channel();
                if tx
                    .send(PowerEvent::Sleeping {
                        done: Some(done_tx),
                    })
                    .is_err()
                {
                    break;
                }
                // Wait for `sleeping` to go out, but never hold the suspend
                // up for long (logind's InhibitDelayMaxSec is 5 s).
                let _ = done_rx.recv_timeout(Duration::from_millis(1500));
                debug!("releasing the sleep delay lock");
                lock = None;
            } else if lock.is_none() {
                lock = Some(inhibit()?);
            }
        }
        drop(lock);
        Ok(())
    };
    let spawned = std::thread::Builder::new()
        .name("logind".into())
        .spawn(move || {
            if let Err(e) = run() {
                warn!("no sleep notifications for the sender (logind: {e})");
            }
        });
    if let Err(e) = spawned {
        warn!("logind watcher: {e}");
    }
}

/// Windows: tell the sender `sleeping` before the PC suspends.
/// `RegisterSuspendResumeNotification` with a callback needs no window, so it
/// works from any thread. The callback holds the suspend for up to 1.5 s while
/// `sleeping` goes out, like the logind delay lock.
#[cfg(windows)]
pub fn watch_windows(tx: tokio::sync::mpsc::UnboundedSender<PowerEvent>) {
    use std::ffi::c_void;
    use tracing::{info, warn};
    use windows_sys::Win32::System::Power::{
        DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, RegisterSuspendResumeNotification,
    };

    if WIN_TX.set(tx).is_err() {
        return; // already watching
    }
    const DEVICE_NOTIFY_CALLBACK: u32 = 2;
    // The parameters must outlive the registration: leak them (one per process).
    let params = Box::leak(Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
        Callback: Some(suspend_callback),
        Context: std::ptr::null_mut::<c_void>(),
    }));
    // SAFETY: `params` is valid for the life of the process; the callback is
    // a plain extern "system" fn.
    let handle = unsafe {
        RegisterSuspendResumeNotification(
            params as *mut DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS as *mut c_void,
            DEVICE_NOTIFY_CALLBACK,
        )
    };
    if handle == 0 {
        warn!("no sleep notifications for the sender (RegisterSuspendResumeNotification failed)");
    } else {
        info!("watching Windows power events for sleep");
    }
}

/// PBT_APMSUSPEND: the system is about to suspend.
#[cfg(windows)]
const PBT_APMSUSPEND: u32 = 4;

#[cfg(windows)]
unsafe extern "system" fn suspend_callback(
    _context: *const std::ffi::c_void,
    event: u32,
    _setting: *const std::ffi::c_void,
) -> u32 {
    handle_power_event(event);
    0 // ERROR_SUCCESS
}

#[cfg(windows)]
fn handle_power_event(event: u32) {
    use std::time::Duration;
    if event != PBT_APMSUSPEND {
        return;
    }
    if let Some(tx) = WIN_TX.get() {
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        if tx
            .send(PowerEvent::Sleeping {
                done: Some(done_tx),
            })
            .is_ok()
        {
            let _ = done_rx.recv_timeout(Duration::from_millis(1500));
        }
    }
}

#[cfg(windows)]
static WIN_TX: std::sync::OnceLock<tokio::sync::mpsc::UnboundedSender<PowerEvent>> =
    std::sync::OnceLock::new();

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn a_suspend_notification_becomes_a_sleeping_event_and_other_events_are_ignored() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        WIN_TX.set(tx).unwrap();
        handle_power_event(18); // PBT_APMRESUMEAUTOMATIC: ignored
        assert!(rx.try_recv().is_err());
        // The handler waits for `done`; drop the sender side from this thread
        // as the receiver loop would, after seeing the event.
        let t = std::thread::spawn(|| handle_power_event(PBT_APMSUSPEND));
        let ev = loop {
            if let Ok(ev) = rx.try_recv() {
                break ev;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        match ev {
            PowerEvent::Sleeping { done } => drop(done),
        }
        t.join().unwrap();
    }
}
