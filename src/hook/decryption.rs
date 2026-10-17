//! `Hooks_Decryption.cpp`. Serves Lua-provided depot decryption keys.
//!
//! The ticket-readback helpers (`get_cache_app_ownership_ticket` and the
//! captured local ConfigStore) moved to `unused_denuvo/` along with the
//! rest of the ticket pipeline.

use std::ffi::c_void;

use crate::steam::types::AppId_t;
use crate::utils::config::lua_config;
use crate::{hook_fn, install_hook_c, uninstall_hook};

const DECRYPTION_KEY_SUFFIX: &str = "\\DecryptionKey";

fn try_extract_depot_id(key_name: &str) -> Option<AppId_t> {
    let suffix_at = key_name.find(DECRYPTION_KEY_SUFFIX)?;
    let before = &key_name[..suffix_at];
    let last_backslash = before.rfind('\\')?;
    let digits = &before[last_backslash + 1..];
    crate::platform::numbers::parse_u32(digits)
}

hook_fn! {
    fn ConfigStoreGetBinary(
        p_object: *mut c_void,
        e_config_store: i32,
        key_name: *const i8,
        key: *mut i8,
        key_size: u32,
    ) -> i32 {
        if !key_name.is_null() {
            let name = std::ffi::CStr::from_ptr(key_name).to_string_lossy();
            LOG_DECRYPTIONKEY_DEBUG!(
                "ConfigStore::GetBinary called for pObject={:p}, eConfigStore={}, KeyName='{}'",
                p_object,
                e_config_store,
                name
            );

            if let Some(depot_id) = try_extract_depot_id(&name) {
                let key_bytes = lua_config::get_decryption_key(depot_id);
                if !key_bytes.is_empty() {
                    if (key_size as usize) >= key_bytes.len() && !key.is_null() {
                        LOG_DECRYPTIONKEY_INFO!(
                            "Providing decryption key for depot {}: {}",
                            depot_id,
                            hex::encode(&key_bytes)
                        );
                        std::ptr::copy_nonoverlapping(
                            key_bytes.as_ptr(),
                            key as *mut u8,
                            key_bytes.len(),
                        );
                        return key_bytes.len() as i32;
                    }
                    LOG_DECRYPTIONKEY_WARN!(
                        "Decryption key for depot {} is too large ({} bytes) for buffer ({} bytes)",
                        depot_id,
                        key_bytes.len(),
                        key_size
                    );
                }
            }
        }

        o_ConfigStoreGetBinary(p_object, e_config_store, key_name, key, key_size)
    }
}

pub fn install() -> bool {
    install_hook_c!(ConfigStoreGetBinary)
}

pub fn uninstall() -> bool {
    uninstall_hook!(ConfigStoreGetBinary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_depot_id_typical() {
        let id = try_extract_depot_id("UserLocal\\appinfo\\730\\DecryptionKey");
        assert_eq!(id, Some(730));
    }

    #[test]
    fn extract_depot_id_no_prefix() {
        assert_eq!(try_extract_depot_id("730\\DecryptionKey"), None);
    }

    #[test]
    fn extract_depot_id_missing_suffix() {
        assert_eq!(try_extract_depot_id("730\\SomeOtherKey"), None);
    }

    #[test]
    fn extract_depot_id_non_numeric() {
        assert_eq!(try_extract_depot_id("abc\\DecryptionKey"), None);
    }

    #[test]
    fn extract_depot_id_no_backslash_before() {
        assert_eq!(try_extract_depot_id("730DecryptionKey"), None);
    }
}