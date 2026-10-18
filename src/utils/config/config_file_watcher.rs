//! `Utils/Config/ConfigFileWatcher.cpp`. Watches the TOML config file
//! and the Lua directory. Reloads the config on TOML change; rescans
//! the Lua directories on new/removed `.lua` files.
//!
//! One worker thread, one `DirectoryWatch::Watch` for the config's
//! parent directory and one for the Lua directory. The two are tagged
//! so a change event routes to the correct handler.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::platform::directory_watch::{self, WaitStatus, Watch};

const K_WAIT_TIMEOUT_MS: u32 = 500;
const K_DEBOUNCE_MS: u64 = 200;
const K_WATCH_BUFFER_BYTES: u32 = 4096;

#[derive(Copy, Clone)]
enum Tag {
    Config,
    LuaDir,
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static SHUTDOWN: AtomicBool = AtomicBool::new(false);
static HANDLE: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

pub fn start(config_path: &str, steam_root: &str) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    SHUTDOWN.store(false, Ordering::SeqCst);

    let config_path = config_path.to_string();
    let steam_root = steam_root.to_string();
    let h = std::thread::Builder::new()
        .name("ost-cfg-watch".into())
        .spawn(move || worker(config_path, steam_root))
        .ok();
    *HANDLE.lock().expect("config watcher handle poisoned") = h;
}

pub fn stop() {
    SHUTDOWN.store(true, Ordering::SeqCst);
    let h = HANDLE.lock().expect("config watcher handle poisoned").take();
    if let Some(h) = h {
        let _ = h.join();
    }
    RUNNING.store(false, Ordering::SeqCst);
}

fn worker(config_path: String, steam_root: String) {
    let config_parent = PathBuf::from(&config_path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let config_name = PathBuf::from(&config_path)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut lua_dir = crate::utils::config::config::get_lua_dir(&steam_root);
    let mut watches: Vec<Watch> = Vec::new();
    let mut tags: Vec<Tag> = Vec::new();

    if !config_parent.is_empty() {
        let mut w = Watch::new();
        if w.open(&config_parent, K_WATCH_BUFFER_BYTES) {
            let _ = w.issue_read();
            watches.push(w);
            tags.push(Tag::Config);
        } else {
            LOG_WARN!("ConfigFileWatcher: could not open watch on {}", config_parent);
        }
    }
    if !lua_dir.is_empty() {
        let mut w = Watch::new();
        if w.open(&lua_dir, K_WATCH_BUFFER_BYTES) {
            let _ = w.issue_read();
            watches.push(w);
            tags.push(Tag::LuaDir);
        } else {
            LOG_WARN!("ConfigFileWatcher: could not open watch on {}", lua_dir);
        }
    }
    if watches.is_empty() {
        LOG_WARN!("ConfigFileWatcher: no directories could be watched");
        return;
    }

    while !SHUTDOWN.load(Ordering::SeqCst) {
        let mut refs: Vec<&mut Watch> = watches.iter_mut().collect();
        let r = directory_watch::wait_any(&mut refs, K_WAIT_TIMEOUT_MS);
        match r.status {
            WaitStatus::Signaled => {
                let mut reload_config = false;
                let mut reload_lua = false;
                for (i, w) in watches.iter_mut().enumerate() {
                    for c in w.drain() {
                        match tags[i] {
                            Tag::Config => {
                                if c.relative_path.eq_ignore_ascii_case(&config_name) {
                                    reload_config = true;
                                }
                            }
                            Tag::LuaDir => {
                                if c.relative_path.to_ascii_lowercase().ends_with(".lua") {
                                    reload_lua = true;
                                }
                            }
                        }
                    }
                }

                // Debounce: a save often produces several events in a
                // few ms. Sleep, then discard whatever accumulated.
                std::thread::sleep(Duration::from_millis(K_DEBOUNCE_MS));
                for w in watches.iter_mut() {
                    let _ = w.drain();
                }
                if SHUTDOWN.load(Ordering::SeqCst) { break; }

                if reload_config {
                    LOG_INFO!("ConfigFileWatcher: {} changed, reloading", config_path);
                    let result = crate::utils::config::config::load(&config_path);
                    crate::utils::logging::init_modules_public();
                    
                    let new_lua_dir = crate::utils::config::config::get_lua_dir(&steam_root);
                    let lua_dir_changed = new_lua_dir != lua_dir;
                    lua_dir = new_lua_dir;

                    if result.lua_paths_changed || lua_dir_changed {
                        let new_dirs = crate::utils::config::lua_config::merge_watch_dirs(
                            &crate::utils::config::config::get_lua_paths(),
                            &lua_dir,
                        );
                        crate::utils::config::lua_config::reload_directories(&new_dirs, false);
                        crate::bridge::hooks_notify_license_changed();

                        if lua_dir_changed {
                            crate::utils::config::lua_file_watcher::stop();
                            crate::utils::config::lua_file_watcher::start(new_dirs);
                        }
                    }
                }
                if reload_lua {
                    LOG_INFO!("ConfigFileWatcher: lua dir changed, rescanning");
                    let dirs = crate::utils::config::lua_config::merge_watch_dirs(
                        &crate::utils::config::config::get_lua_paths(),
                        &lua_dir,
                    );
                    crate::utils::config::lua_config::reload_directories(&dirs, false);
                    crate::bridge::hooks_notify_license_changed();
                }

                for w in watches.iter_mut() {
                    let _ = w.issue_read();
                }
            }
            WaitStatus::Timeout => { /* loop */ }
            WaitStatus::Failed => {
                LOG_WARN!("ConfigFileWatcher: wait_any failed, stopping");
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
        SHUTDOWN.store(true, Ordering::SeqCst);
        stop();
    }
}