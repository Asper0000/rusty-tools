//! Emits the linker /DEF: directive for the xinput1_4 export table.
//!
//! Unlike dwmapi, this proxy defines real wrapper functions; the .def
//! assigns each of them to a specific ordinal (and marks the six
//! undocumented ordinals NONAME). The ordinals match the real
//! `%WINDIR%\System32\xinput1_4.dll` export table so import-by-ordinal
//! callers (Steam Big Picture reads the guide button via ordinal 100)
//! resolve to our wrapper.

use std::env;
use std::path::PathBuf;

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let def = manifest_dir.join("xinput1_4.def");

    if !def.exists() {
        panic!(
            "xinput1_4.def not found at {}.\n\
             The proxy DLL cannot be built without its ordinal table.",
            def.display()
        );
    }

    println!("cargo:rerun-if-changed=xinput1_4.def");
    println!("cargo:rustc-cdylib-link-arg=/DEF:{}", def.display());
}
