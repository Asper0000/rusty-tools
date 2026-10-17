use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;

use crate::platform::dynamic_library::{self, ModuleHandle};
use super::level::Level;
use super::logger::Logger;
use super::module::LogModule;

#[derive(Copy, Clone, Debug)]
pub struct Source {
    pub file: &'static str,
    pub line: u32,
    pub function: &'static str,
}

struct State {
    loggers: Vec<Logger>,
    #[allow(dead_code)]
    log_dir: PathBuf,
}

static STATE: OnceLock<State> = OnceLock::new();
static LEVEL: RwLock<Level> = RwLock::new(Level::Trace);

pub fn init_main(self_module: ModuleHandle) {
    let dir = {
        let d = dynamic_library::get_module_directory(self_module);
        if d.as_os_str().is_empty() {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else { d }
    };
    let log_dir = dir.join("rustytools");
    let _ = std::fs::create_dir_all(&log_dir);

    let mut loggers = Vec::with_capacity(LogModule::COUNT);
    for module in LogModule::ALL {
        let path = log_dir.join(format!("{}.log", module.file_name()));
        let l = match Logger::open(&path, Level::Trace) {
            Ok(l) => l,
            Err(_) => {
                // Fall back to a temp path so init doesn't fail.
                let tmp = std::env::temp_dir().join(format!("ost-{}.log", module.file_name()));
                Logger::open(&tmp, Level::Trace).expect("cannot open log")
            }
        };
        loggers.push(l);
    }

    let _ = STATE.set(State { loggers, log_dir });
}

/// Called from `Config::load` consumers after config is available.
pub fn init_modules() {
    let state = match STATE.get() { Some(s) => s, None => return };
    let lvl = *LEVEL.read();
    for l in &state.loggers { l.set_level(lvl); }
}

pub fn apply_config_level() { init_modules(); }

/// Update the level used by `init_modules` / `apply_config_level`.
pub fn set_level(lvl: Level) { *LEVEL.write() = lvl; }

pub fn current_level() -> Level { *LEVEL.read() }

pub fn install_platform_log_sink() {
    crate::platform::logging::set_sink(Arc::new(|level, src, msg| {
        let our = match level {
            crate::platform::logging::Level::Trace => Level::Trace,
            crate::platform::logging::Level::Debug => Level::Debug,
            crate::platform::logging::Level::Info  => Level::Info,
            crate::platform::logging::Level::Warn  => Level::Warn,
            crate::platform::logging::Level::Error => Level::Error,
        };
        log_at(
            LogModule::Platform,
            our,
            format_args!("{}", msg),
            Source { file: src.file, line: src.line, function: src.function },
        );
    }));
}

pub fn log_at(module: LogModule, level: Level, args: std::fmt::Arguments<'_>, src: Source) {
    if let Some(state) = STATE.get() {
        if let Some(logger) = state.loggers.get(module.as_index()) {
            logger.log(level, args, &src);
        }
    }
}
