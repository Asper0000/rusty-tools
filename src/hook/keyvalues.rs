//! `Hooks_KeyValues.cpp`. Resolves `FindOrCreateKey` (for future KV
//! tree manipulation) and hooks `KeyValues::ReadAsBinary` as a
//! pass-through. The orchestrator does not call `install()` by default,
//! matching the commented-out line in `HookManager.cpp`.

use crate::steam::structs::KeyValues;
use crate::{hook_fn, install_hook_c, resolve_c, resolve_fn, uninstall_hook};

resolve_fn! {
    fn FindOrCreateKey(
        parent: *mut KeyValues,
        name: *const i8,
        create: bool,
        out: *mut *mut KeyValues,
    ) -> *mut KeyValues
}

hook_fn! {
    fn ReadAsBinary(
        root: *mut KeyValues,
        buf: *mut std::ffi::c_void,
        depth: i32,
        text_mode: bool,
        sym_table: *mut std::ffi::c_void,
    ) -> bool {
        o_ReadAsBinary(root, buf, depth, text_mode, sym_table)
    }
}

pub fn install() -> bool {
    resolve_c!(FindOrCreateKey);
    install_hook_c!(ReadAsBinary)
}

pub fn uninstall() -> bool {
    uninstall_hook!(ReadAsBinary)
}
