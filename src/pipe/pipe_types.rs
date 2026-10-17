//! Shared identities passed between Pipe features. Source:
//! `Pipe/PipeTypes.h`.

use crate::steam::structs::CPipeClient;
use crate::steam::types::{AppId_t, HSteamPipe, PID_t};

/// PID plus creation time. Lets caches survive PID reuse safely.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProcessKey {
    pub pid: PID_t,
    pub creation_time: u64,
}

impl ProcessKey {
    pub fn is_valid(&self) -> bool {
        self.pid != 0 && self.creation_time != 0
    }

    pub fn debug_string(&self) -> String {
        format!(
            "pid={} creation={} filetime={}",
            self.pid,
            crate::platform::process::format_creation_time(self.creation_time),
            self.creation_time,
        )
    }
}

/// A process may open several Steam IPC pipes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PipeKey {
    pub pid: PID_t,
    pub pipe: HSteamPipe,
}

impl PipeKey {
    pub fn is_valid(&self) -> bool {
        self.pid != 0 && self.pipe != 0
    }

    pub fn debug_string(&self) -> String {
        format!("pid={} pipe=0x{:08X}", self.pid, self.pipe as u32)
    }
}

/// # Safety
/// `pipe`, if non-null, must point at a live `CPipeClient`.
pub unsafe fn make_pipe_key(pipe: *const CPipeClient) -> PipeKey {
    if pipe.is_null() {
        return PipeKey { pid: 0, pipe: 0 };
    }
    PipeKey {
        pid: (*pipe).m_clientPID,
        pipe: (*pipe).m_hSteamPipe,
    }
}

pub struct PipeContext {
    pub pipe: *mut CPipeClient,
    pub process: ProcessKey,
    pub app_id: AppId_t,
    pub game_process: bool,
    pub tracked_app: bool,
    pub owned: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_key_valid_requires_both_fields() {
        assert!(!ProcessKey { pid: 0, creation_time: 1 }.is_valid());
        assert!(!ProcessKey { pid: 1, creation_time: 0 }.is_valid());
        assert!(ProcessKey { pid: 1, creation_time: 1 }.is_valid());
    }

    #[test]
    fn pipe_key_valid_requires_both_fields() {
        assert!(!PipeKey { pid: 0, pipe: 1 }.is_valid());
        assert!(!PipeKey { pid: 1, pipe: 0 }.is_valid());
        assert!(PipeKey { pid: 1, pipe: 1 }.is_valid());
    }

    #[test]
    fn make_pipe_key_null_is_invalid() {
        let k = unsafe { make_pipe_key(std::ptr::null()) };
        assert_eq!(k.pid, 0);
        assert_eq!(k.pipe, 0);
        assert!(!k.is_valid());
    }
}
