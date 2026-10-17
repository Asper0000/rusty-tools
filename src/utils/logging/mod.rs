//! Multi-file logger. Ported from `Utils/Logging/`.
//!
//! The `LOG_*` macros are defined by `build.rs` and included at the
//! crate root in `lib.rs`. This module provides the runtime they call.

mod level;
mod module;
mod logger;
mod state;

pub use level::Level;
pub use module::LogModule;
pub use state::{Source, apply_config_level, current_level, init_main, init_modules,
                install_platform_log_sink, log_at, set_level};

use crate::platform::dynamic_library::ModuleHandle;

pub fn init(self_module: ModuleHandle) { state::init_main(self_module) }
pub fn init_modules_public() { state::init_modules() }
pub fn apply_config_level_public() { state::apply_config_level() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_ordering() {
        assert!(Level::Trace < Level::Debug);
        assert!(Level::Debug < Level::Info);
        assert!(Level::Info  < Level::Warn);
        assert!(Level::Warn  < Level::Error);
    }

    #[test]
    fn module_count_matches_all() {
        assert_eq!(LogModule::COUNT, LogModule::ALL.len());
        assert_eq!(LogModule::ALL[0], LogModule::Main);
    }

    #[test]
    fn module_index_round_trips() {
        for (i, m) in LogModule::ALL.iter().enumerate() {
            assert_eq!(m.as_index(), i);
        }
    }
}
