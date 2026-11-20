use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::protocol::frames::OutboundFrame;
use crate::transport::multiplexer::SeamTransportMultiplexer;
use crate::transport::SendReliability;

pub struct ClipboardSyncEngine {
    multiplexer: Arc<SeamTransportMultiplexer>,
    last_synced_text: Arc<Mutex<String>>,
    running: Arc<AtomicBool>,
    worker_handle: Option<thread::JoinHandle<()>>,
}

impl ClipboardSyncEngine {
    pub fn new(multiplexer: Arc<SeamTransportMultiplexer>) -> Self {
        Self {
            multiplexer,
            last_synced_text: Arc::new(Mutex::new(String::new())),
            running: Arc::new(AtomicBool::new(false)),
            worker_handle: None,
        }
    }

    pub fn start(&mut self) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }

        let running = Arc::clone(&self.running);
        let last_text = Arc::clone(&self.last_synced_text);
        let mux = Arc::clone(&self.multiplexer);

        let handle = thread::Builder::new()
            .name("seam-clipboard".to_string())
            .spawn(move || {
                while running.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(250));
                    if !running.load(Ordering::Relaxed) {
                        break;
                    }

                    if let Some(current_text) = read_linux_clipboard() {
                        if !current_text.is_empty() {
                            let mut lock = last_text.lock().unwrap();
                            if *lock != current_text {
                                *lock = current_text.clone();
                                drop(lock);

                                let frame = OutboundFrame::ClipboardSet { text: current_text };
                                let _ = mux.send(&frame, SendReliability::ReliableOrdered);
                            }
                        }
                    }
                }
            })
            .ok();

        self.worker_handle = handle;
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn apply_remote_text(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Ok(mut lock) = self.last_synced_text.lock() {
            *lock = text.to_string();
        }
        write_linux_clipboard(text);
    }
}

impl Drop for ClipboardSyncEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

fn read_linux_clipboard() -> Option<String> {
    // Try xclip first
    if let Ok(output) = Command::new("xclip")
        .args(["-selection", "clipboard", "-o"])
        .output()
    {
        if output.status.success() {
            return String::from_utf8(output.stdout).ok();
        }
    }
    // Fallback to xsel
    if let Ok(output) = Command::new("xsel")
        .args(["-b", "-o"])
        .output()
    {
        if output.status.success() {
            return String::from_utf8(output.stdout).ok();
        }
    }
    None
}

fn write_linux_clipboard(text: &str) {
    // Try xclip first
    if let Ok(mut child) = Command::new("xclip")
        .args(["-selection", "clipboard", "-i"])
        .stdin(Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
        return;
    }
    // Fallback to xsel
    if let Ok(mut child) = Command::new("xsel")
        .args(["-b", "-i"])
        .stdin(Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    }
}
