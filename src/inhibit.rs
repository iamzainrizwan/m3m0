//! Hold a GNOME logout inhibitor while anything is uncommitted, so logging out
//! or shutting down from GNOME stops and lists m3m0 as having unsaved work
//! (closing the window is guarded in main.rs; this covers the session ending).
//! The helper watches our pid, so a crash never leaves a stale inhibitor.

use std::process::{Child, Command, Stdio};

#[derive(Default)]
pub struct Inhibit {
    child: Option<Child>,
}

impl Inhibit {
    pub fn set(&mut self, on: bool) {
        if let Some(c) = &mut self.child {
            // the helper went away (gnome-session restarted?): start a new one
            if matches!(c.try_wait(), Ok(Some(_))) {
                self.child = None;
            }
        }
        match (on, self.child.is_some()) {
            (true, false) => {
                let watch = format!("while kill -0 {} 2>/dev/null; do sleep 2; done", std::process::id());
                // absolute paths: launched from the app grid there's no shell PATH
                self.child = Command::new("/usr/bin/gnome-session-inhibit")
                    .args(["--app-id", "m3m0", "--reason", "uncommitted notes in m3m0 (:w to commit)", "--inhibit", "logout", "/bin/sh", "-c", &watch])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .ok();
            }
            (false, true) => self.release(),
            _ => {}
        }
    }

    fn release(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for Inhibit {
    fn drop(&mut self) {
        self.release();
    }
}
