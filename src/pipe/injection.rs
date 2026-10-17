//! Per-pipe DLL injection. Source:
//! `Pipe/Features/Injection/Injection.cpp`.

use std::collections::HashSet;
use std::path::PathBuf;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::pipe::pipe_types::{PipeContext, ProcessKey};
use crate::platform::process;
use crate::platform::remote_process::{self, InjectStatus};
use crate::utils::config::config::{self, InjectDll};

/// Keyed on `(process, path)` so each DLL injects at most once per
/// process while several `[[inject]]` entries can still target the
/// same game.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct InjectedKey {
    process: ProcessKey,
    path: String,
}

static G_INJECTED: Lazy<Mutex<HashSet<InjectedKey>>> =
    Lazy::new(|| Mutex::new(HashSet::new()));

fn claim_injection(key: &InjectedKey) -> bool {
    G_INJECTED.lock().insert(key.clone())
}

fn matches(dll: &InjectDll, ctx: &PipeContext, cmd_line: Option<&str>) -> bool {
    if !dll.all_games && !ctx.tracked_app { return false; }
    if !dll.when_appids.is_empty() && !dll.when_appids.contains(&ctx.app_id) {
        return false;
    }
    if !dll.when_cmdline.is_empty() {
        match cmd_line {
            Some(c) if c.contains(&dll.when_cmdline) => {}
            _ => return false,
        }
    }
    true
}

pub fn apply(ctx: &PipeContext) {
    let dlls = config::get_inject_dlls();
    if dlls.is_empty() { return; }
    if !ctx.game_process { return; }

    // Read the command line lazily: only if an injection entry uses it.
    let mut cmd_line: Option<String> = None;
    let mut cmd_line_resolved = false;

    for dll in &dlls {
        let cmd = if dll.when_cmdline.is_empty() {
            cmd_line.as_deref()
        } else {
            if !cmd_line_resolved {
                cmd_line = process::get_process_command_line(ctx.process.pid);
                cmd_line_resolved = true;
            }
            cmd_line.as_deref()
        };

        if !matches(dll, ctx, cmd) { continue; }
        if !claim_injection(&InjectedKey {
            process: ctx.process,
            path: dll.path.clone(),
        }) {
            continue;
        }

        let path = PathBuf::from(&dll.path);
        let status = remote_process::inject_library(ctx.process.pid, &path);
        if status == InjectStatus::Ok {
            LOG_INJECT_INFO!(
                "injected pid={} appid={} dll=\"{}\"",
                ctx.process.pid, ctx.app_id,
                path.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
            );
        } else {
            LOG_INJECT_WARN!(
                "inject failed pid={} appid={} status={} dll=\"{}\"",
                ctx.process.pid, ctx.app_id, status.as_str(),
                path.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_dll(all_games: bool, when_appids: &[u32], when_cmdline: &str) -> InjectDll {
        InjectDll {
            path: String::new(),
            when_cmdline: when_cmdline.to_string(),
            when_appids: when_appids.iter().copied().collect(),
            all_games,
        }
    }

    fn mk_ctx(tracked: bool, app_id: u32) -> PipeContext {
        PipeContext {
            pipe: std::ptr::null_mut(),
            process: ProcessKey { pid: 1, creation_time: 1 },
            app_id,
            game_process: true,
            tracked_app: tracked,
            owned: false,
        }
    }

    #[test]
    fn matches_requires_tracked_or_all_games() {
        let ctx = mk_ctx(false, 100);
        assert!(!matches(&mk_dll(false, &[], ""), &ctx, None));
        assert!(matches(&mk_dll(true, &[], ""), &ctx, None));
    }

    #[test]
    fn matches_requires_appid_when_appids_given() {
        let ctx = mk_ctx(true, 100);
        assert!(matches(&mk_dll(true, &[100], ""), &ctx, None));
        assert!(!matches(&mk_dll(true, &[200], ""), &ctx, None));
    }

    #[test]
    fn matches_requires_substring_in_cmdline() {
        let ctx = mk_ctx(true, 100);
        assert!(matches(&mk_dll(true, &[], "launch"), &ctx, Some("game.exe launch")));
        assert!(!matches(&mk_dll(true, &[], "launch"), &ctx, Some("game.exe")));
        assert!(!matches(&mk_dll(true, &[], "launch"), &ctx, None));
    }
}
