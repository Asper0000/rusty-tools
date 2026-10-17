//! Stub for CloudRedirect host.
//! The real implementation was moved to `src/unused_denuvo/cloud_redirect/host.rs`.
//! This stub keeps the API surface so callers compile without changes.
//! All functions are inert.

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct AchievementBlock {
    pub stat_id: u32,
    pub bits: u32,
    pub unlock_times: [u32; 32],
}

pub fn initialize(_steam_install_path: &str) {
    LOG_INFO!("CloudRedirect: backend moved to unused/; cloud save redirection disabled");
}

pub fn shutdown() {}

pub fn is_active() -> bool { false }

pub fn is_app(_app_id: u32) -> bool { false }

pub fn handle_cloud_rpc(
    _method: &str,
    _app_id: u32,
    _account_id: u32,
    _req_body: &[u8],
    _resp_buf: &mut [u8],
) -> Option<(usize, i32)> {
    None
}

pub fn sync_app_set() {}

pub fn set_account_id(_account_id: u32) {}

pub fn notify_app_running(_app_id: u32, _running: bool) {}

pub fn notify_stats_stored(_app_id: u32) {}

pub fn get_achievements(_app_id: u32, _out: &mut [AchievementBlock]) -> usize { 0 }