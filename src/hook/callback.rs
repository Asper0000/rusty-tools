//! `Hooks_CallBack.cpp`. Hooks `SendCallbackToPipe`; the body is where
//! callback-data rewrites will land. Currently a pass-through, matching
//! the C++ port.

use std::ffi::c_void;

use crate::steam::types::{HSteamPipe, HSteamUser};
use crate::{hook_fn, install_hook_c, uninstall_hook};

hook_fn! {
    fn SendCallbackToPipe(
        pSteamEngine: *mut c_void,
        hSteamPipe: HSteamPipe,
        iClientUser: HSteamUser,
        iCallback: i32,
        pCallbackData: *mut c_void,
        cubCallbackData: i32,
    ) -> bool {
        // Callback modifier dispatch lands here; see the C++ file for the
        // contract (mutate `pCallbackData` in place before delegating).
        o_SendCallbackToPipe(
            pSteamEngine, hSteamPipe, iClientUser,
            iCallback, pCallbackData, cubCallbackData,
        )
    }
}

pub fn install() -> bool {
    install_hook_c!(SendCallbackToPipe)
}

pub fn uninstall() -> bool {
    uninstall_hook!(SendCallbackToPipe)
}
