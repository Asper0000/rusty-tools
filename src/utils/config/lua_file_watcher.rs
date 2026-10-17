//! `Utils/Config/LuaFileWatcher.cpp`. Watches the merged Lua
//! directories and reloads the Lua config on any change.
//!
//! One worker thread, one `DirectoryWatch::Watch` per directory. On
//! any change the worker drains, debounces briefly (so a burst of
//! file-save events coalesces into one reload), drains again, and
//! calls `lua_config::reload_directories`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::platform::directory_watch::{self, WaitStatus, Watch};

const K_WAIT_TIMEOUT_MS: u32 = 500;
const K_DEBOUNCE_MS: u64 = 200;
const K_WATCH_BUFFER_BYTES: u32 = 4096;

static RUNNING: AtomicBool = AtomicBool::new(false);
static SHUTDOWN: AtomicBool = AtomicBool::new(false);
static HANDLE: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

pub fn start(dirs: Vec<String>) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    SHUTDOWN.store(false, Ordering::SeqCst);

    let h = std::thread::Builder::new()
        .name("ost-lua-watch".into())
        .spawn(move || worker(dirs))
        .ok();
    *HANDLE.lock().expect("lua watcher handle poisoned") = h;
}

pub fn stop() {
    SHUTDOWN.store(true, Ordering::SeqCst);
    let h = HANDLE.lock().expect("lua watcher handle poisoned").take();
    if let Some(h) = h {
        let _ = h.join();
    }
    RUNNING.store(false, Ordering::SeqCst);
}

fn worker(dirs: Vec<String>) {
    let mut watches: Vec<Watch> = Vec::new();
    for d in &dirs {
        let mut w = Watch::new();
        if w.open(d, K_WATCH_BUFFER_BYTES) {
            let _ = w.issue_read();
            watches.push(w);
        } else {
            LOG_WARN!("LuaFileWatcher: could not open watch on {}", d);
        }
    }
    if watches.is_empty() {
        LOG_WARN!("LuaFileWatcher: no directories could be watched");
        return;
    }

    while !SHUTDOWN.load(Ordering::SeqCst) {
        let mut refs: Vec<&mut Watch> = watches.iter_mut().collect();
        let r = directory_watch::wait_any(&mut refs, K_WAIT_TIMEOUT_MS);
        match r.status {
            WaitStatus::Signaled => {
                // Drain every watch, then debounce, then drain again
                // to catch files that landed during the sleep.
                for w in watches.iter_mut() {
                    let _ = w.drain();
                }
                std::thread::sleep(Duration::from_millis(K_DEBOUNCE_MS));
                for w in watches.iter_mut() {
                    let _ = w.drain();
                }

                if SHUTDOWN.load(Ordering::SeqCst) { break; }
                LOG_INFO!("LuaFileWatcher: change detected, reloading directories");
                crate::utils::config::lua_config::reload_directories(&dirs, false);
                crate::bridge::hooks_notify_license_changed();

                for w in watches.iter_mut() {
                    let _ = w.issue_read();
                }
            }
            WaitStatus::Timeout => { /* loop */ }
            WaitStatus::Failed => {
                LOG_WARN!("LuaFileWatcher: wait_any failed, stopping");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_before_start_is_noop() {
        // SHUTDOWN store + HANDLE.take on an empty handle is safe.
        SHUTDOWN.store(true, Ordering::SeqCst);
        stop();
    }
}
