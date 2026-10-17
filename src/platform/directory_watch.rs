//! Directory change notifications. Source: `OSTPlatform/Windows/DirectoryWatch.cpp`
//!
//! `Watch` is an opaque handle wrapping:
//!   * a directory handle opened with `FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED`
//!   * an event for the overlapped read
//!   * a `Vec<u8>` for `FILE_NOTIFY_INFORMATION` records
//!   * a pending-read flag
//!
//! `wait_any` blocks on the event handles via `WaitForMultipleObjects`.

use std::path::PathBuf;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChangeAction {
    Added,
    Removed,
    Modified,
    RenamedOldName,
    RenamedNewName,
}

#[derive(Clone, Debug)]
pub struct Change {
    pub relative_path: String,
    pub action: ChangeAction,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WaitStatus { Signaled, Timeout, Failed }

#[derive(Copy, Clone, Debug)]
pub struct WaitResult {
    pub status: WaitStatus,
    pub index: usize,
}

// ─── Watch handle ───────────────────────────────────────────────────

pub struct Watch {
    directory: Option<PathBuf>,
    imp: imp::WatchImpl,
}

impl Watch {
    pub fn new() -> Self {
        Self { directory: None, imp: imp::WatchImpl::new() }
    }

    pub fn open(&mut self, directory: &str, buffer_size: u32) -> bool {
        self.directory = Some(PathBuf::from(directory));
        self.imp.open(directory, buffer_size)
    }

    pub fn issue_read(&mut self) -> bool {
        self.imp.issue_read()
    }

    pub fn drain(&mut self) -> Vec<Change> {
        self.imp.drain()
    }

    pub fn cancel(&mut self) {
        self.imp.cancel();
        self.directory = None;
    }

    pub fn is_open(&self) -> bool { self.imp.is_open() }
    pub fn directory(&self) -> &str {
        self.directory.as_ref().and_then(|p| p.to_str()).unwrap_or("")
    }

    pub(super) fn native_event(&self) -> imp::NativeEvent { self.imp.native_event() }
}

impl Default for Watch {
    fn default() -> Self { Self::new() }
}

// ─── Windows implementation ─────────────────────────────────────────

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, ReadDirectoryChangesW, FILE_ACTION_ADDED, FILE_ACTION_MODIFIED,
        FILE_ACTION_REMOVED, FILE_ACTION_RENAMED_NEW_NAME, FILE_ACTION_RENAMED_OLD_NAME,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_FILE_NAME,
        FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_INFORMATION, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows_sys::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForMultipleObjects};

    pub(super) type NativeEvent = HANDLE;

    fn to_wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    fn from_wide(units: &[u16]) -> String {
        String::from_utf16_lossy(units)
    }

    fn from_windows_action(a: u32) -> ChangeAction {
        match a {
            x if x == FILE_ACTION_ADDED => ChangeAction::Added,
            x if x == FILE_ACTION_REMOVED => ChangeAction::Removed,
            x if x == FILE_ACTION_MODIFIED => ChangeAction::Modified,
            x if x == FILE_ACTION_RENAMED_OLD_NAME => ChangeAction::RenamedOldName,
            x if x == FILE_ACTION_RENAMED_NEW_NAME => ChangeAction::RenamedNewName,
            _ => ChangeAction::Modified,
        }
    }

    pub(super) struct WatchImpl {
        dir: HANDLE,
        event: HANDLE,
        overlapped: OVERLAPPED,
        buffer: Vec<u8>,
        read_pending: bool,
    }

    unsafe impl Send for WatchImpl {}

    impl WatchImpl {
        pub(super) fn new() -> Self {
            Self {
                dir: std::ptr::null_mut(),
                event: std::ptr::null_mut(),
                overlapped: unsafe { std::mem::zeroed() },
                buffer: Vec::new(),
                read_pending: false,
            }
        }

        pub(super) fn open(&mut self, directory: &str, buffer_size: u32) -> bool {
            self.close();

            let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
            if event.is_null() {
                crate::ostp_warn!(
                    "DirectoryWatch: CreateEvent failed for '{}' (error={})",
                    directory,
                    unsafe { GetLastError() }
                );
                return false;
            }
            self.event = event;

            let native = to_wide(directory);
            let dir = unsafe {
                CreateFileW(
                    native.as_ptr(),
                    FILE_LIST_DIRECTORY,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                    std::ptr::null_mut(),
                )
            };
            if dir == INVALID_HANDLE_VALUE {
                crate::ostp_warn!(
                    "DirectoryWatch: CreateFile failed for '{}' (error={})",
                    directory,
                    unsafe { GetLastError() }
                );
                self.close();
                return false;
            }
            self.dir = dir;

            self.buffer = vec![0u8; buffer_size as usize];
            self.overlapped = unsafe { std::mem::zeroed() };
            self.overlapped.hEvent = event;
            true
        }

        pub(super) fn issue_read(&mut self) -> bool {
            if !self.is_open() { return false; }
            if self.read_pending { return true; }

            let event = self.event;
            self.overlapped = unsafe { std::mem::zeroed() };
            self.overlapped.hEvent = event;

            let mut dummy: u32 = 0;
            let ok = unsafe {
                ReadDirectoryChangesW(
                    self.dir,
                    self.buffer.as_mut_ptr() as *mut c_void,
                    self.buffer.len() as u32,
                    0,
                    FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE,
                    &mut dummy,
                    &mut self.overlapped,
                    None,
                )
            };
            if ok == 0 {
                let err = unsafe { GetLastError() };
                // ERROR_IO_PENDING (997) means the async op is queued
                // successfully; anything else is a real failure.
                const ERROR_IO_PENDING: u32 = 997;
                if err != ERROR_IO_PENDING {
                    crate::ostp_warn!("DirectoryWatch: ReadDirectoryChangesW failed (error={})", err);
                    self.read_pending = false;
                    return false;
                }
            }
            self.read_pending = true;
            true
        }

        pub(super) fn drain(&mut self) -> Vec<Change> {
            let mut changes = Vec::new();
            if !self.is_open() || !self.read_pending { return changes; }

            let mut bytes_returned: u32 = 0;
            let ok = unsafe {
                GetOverlappedResult(self.dir, &self.overlapped, &mut bytes_returned, 0)
            };
            if ok == 0 || bytes_returned == 0 {
                self.read_pending = false;
                return changes;
            }
            self.read_pending = false;

            // Walk the FILE_NOTIFY_INFORMATION chain.
            let mut offset = 0usize;
            let base = self.buffer.as_ptr();
            loop {
                if offset + std::mem::size_of::<FILE_NOTIFY_INFORMATION>() > self.buffer.len() {
                    break;
                }
                let info = unsafe { &*(base.add(offset) as *const FILE_NOTIFY_INFORMATION) };
                let chars = (info.FileNameLength / 2) as usize;
                let name_slice = unsafe {
                    std::slice::from_raw_parts(info.FileName.as_ptr() as *const u16, chars)
                };
                let relative_path = from_wide(name_slice);
                if !relative_path.is_empty() {
                    changes.push(Change {
                        relative_path,
                        action: from_windows_action(info.Action),
                    });
                }
                if info.NextEntryOffset == 0 { break; }
                offset += info.NextEntryOffset as usize;
            }
            changes
        }

        pub(super) fn cancel(&mut self) {
            self.close();
        }

        pub(super) fn is_open(&self) -> bool {
            !self.dir.is_null() && !self.event.is_null()
        }

        pub(super) fn native_event(&self) -> HANDLE { self.event }

        fn close(&mut self) {
            if !self.dir.is_null() {
                unsafe { CancelIoEx(self.dir, std::ptr::null()); }
            }
            if !self.dir.is_null() && self.dir != INVALID_HANDLE_VALUE {
                unsafe { CloseHandle(self.dir); }
            }
            if !self.event.is_null() {
                unsafe { CloseHandle(self.event); }
            }
            self.dir = std::ptr::null_mut();
            self.event = std::ptr::null_mut();
            self.overlapped = unsafe { std::mem::zeroed() };
            self.buffer.clear();
            self.read_pending = false;
        }
    }

    impl Drop for WatchImpl {
        fn drop(&mut self) { self.close(); }
    }

    pub(super) fn wait_any(watches: &mut [&mut Watch], timeout_ms: u32) -> WaitResult {
        let mut events: Vec<HANDLE> = Vec::with_capacity(watches.len());
        let mut indexes: Vec<usize> = Vec::with_capacity(watches.len());
        for (i, w) in watches.iter().enumerate() {
            if !w.is_open() { continue; }
            events.push(w.native_event());
            indexes.push(i);
        }
        if events.is_empty() {
            return WaitResult { status: WaitStatus::Failed, index: 0 };
        }

        let n = events.len() as u32;
        let result = unsafe {
            WaitForMultipleObjects(n, events.as_ptr(), 0, timeout_ms)
        };
        if result == WAIT_TIMEOUT {
            return WaitResult { status: WaitStatus::Timeout, index: 0 };
        }
        if result >= WAIT_OBJECT_0 && result < WAIT_OBJECT_0 + n {
            let idx = (result - WAIT_OBJECT_0) as usize;
            return WaitResult { status: WaitStatus::Signaled, index: indexes[idx] };
        }
        crate::ostp_warn!("DirectoryWatch: WaitForMultipleObjects failed (result={})", result);
        let _ = ResetEvent; // silence unused
        WaitResult { status: WaitStatus::Failed, index: 0 }
    }
}

// ─── Non-Windows fallback ───────────────────────────────────────────

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub(super) type NativeEvent = *mut c_void;

    pub(super) struct WatchImpl;
    impl WatchImpl {
        pub(super) fn new() -> Self { Self }
        pub(super) fn open(&mut self, _d: &str, _b: u32) -> bool { false }
        pub(super) fn issue_read(&mut self) -> bool { false }
        pub(super) fn drain(&mut self) -> Vec<Change> { Vec::new() }
        pub(super) fn cancel(&mut self) {}
        pub(super) fn is_open(&self) -> bool { false }
        pub(super) fn native_event(&self) -> NativeEvent { std::ptr::null_mut() }
    }

    pub(super) fn wait_any(_w: &mut [&mut Watch], _t: u32) -> WaitResult {
        WaitResult { status: WaitStatus::Failed, index: 0 }
    }
}

pub fn wait_any(watches: &mut [&mut Watch], timeout_ms: u32) -> WaitResult {
    imp::wait_any(watches, timeout_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_watch_is_not_open() {
        let w = Watch::new();
        assert!(!w.is_open());
        assert_eq!(w.directory(), "");
    }

    #[test]
    fn cancel_clears_state() {
        let mut w = Watch::new();
        let _ = w.open("C:\\temp", 4096);
        w.cancel();
        assert!(!w.is_open());
        assert_eq!(w.directory(), "");
    }

    #[test]
    fn drain_empty_when_not_opened() {
        let mut w = Watch::new();
        assert!(w.drain().is_empty());
    }

    #[test]
    fn change_action_equality() {
        assert_eq!(ChangeAction::Added, ChangeAction::Added);
        assert_ne!(ChangeAction::Added, ChangeAction::Removed);
    }

    #[cfg(windows)]
    #[test]
    fn open_existing_directory_succeeds() {
        let sysdir = crate::platform::dynamic_library::get_system_directory_path();
        if sysdir.is_empty() { return; }
        let mut w = Watch::new();
        assert!(w.open(&sysdir, 4096));
        assert!(w.is_open());
        w.cancel();
        assert!(!w.is_open());
    }

    #[cfg(windows)]
    #[test]
    fn issue_read_then_cancel() {
        let sysdir = crate::platform::dynamic_library::get_system_directory_path();
        if sysdir.is_empty() { return; }
        let mut w = Watch::new();
        assert!(w.open(&sysdir, 4096));
        assert!(w.issue_read());
        w.cancel();
    }
}
