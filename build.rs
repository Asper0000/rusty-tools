//! Build-time code generator. Emits per-module `LOG_*` macros, compiles
//! the Steam protobuf schemas into `OUT_DIR/steam_messages.rs`, and
//! emits link directives for the vendored Microsoft Detours static
//! library on Windows targets.
//!
//! The macros are NOT `#[macro_export]`. They are `include!`d at the
//! top of `lib.rs`, so they live at the crate root and are visible to
//! every subsequent module through Rust's textual macro scoping.
//! Calls are unqualified: `LOG_INFO!(...)`.
//!
//! Module names are uppercased for the macro name so the caller writes
//! `LOG_PACKAGE_INFO!`, matching the C++ naming.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const MODULES: &[(&str, &str)] = &[
    ("IPC",           "ipc"),
    ("NetPacket",     "netpacket"),
    ("Manifest",      "manifest"),
    ("KeyValue",      "keyvalue"),
    ("DecryptionKey", "decryptionkey"),
    ("Misc",          "misc"),
    ("Achievement",   "achievement"),
    ("Pics",          "pics"),
    ("OnlineFix",     "onlinefix"),
    ("RichPresence",  "richpresence"),
    ("Package",       "package"),
    ("SteamUI",       "steamui"),
    ("Inject",        "inject"),
    ("Pipe",          "pipe"),
    ("Platform",      "platform"),
];

const LEVELS: &[(&str, &str)] = &[
    ("TRACE", "Trace"),
    ("DEBUG", "Debug"),
    ("INFO",  "Info"),
    ("WARN",  "Warn"),
    ("ERROR", "Error"),
];

fn emit(out: &mut String, name: &str, variant: &str, level_variant: &str) {
    out.push_str(&format!(
"#[allow(unused_macros)]\nmacro_rules! {name} {{\n    ($($arg:tt)*) => {{\n        $crate::utils::logging::log_at(\n            $crate::utils::logging::LogModule::{variant},\n            $crate::utils::logging::Level::{level_variant},\n            format_args!($($arg)*),\n            $crate::utils::logging::Source {{ file: file!(), line: line!(), function: \"\" }},\n        )\n    }};\n}}\n\n"
    ));
}

fn emit_detours_link(crate_root: &Path) {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let detours = crate_root.join("third_party").join("Detours");
    let lib_dir = detours.join("lib.X64");
    let header  = detours.join("include").join("detours.h");
    let lib     = lib_dir.join("detours.lib");

    if !header.exists() {
        panic!(
            "third_party/Detours/include/detours.h is missing.\n\
             Clone microsoft/Detours into third_party/Detours and pin a commit.\n\
             See CHANGELOG.md Batch 06a-prep for the build steps."
        );
    }
    if !lib.exists() {
        panic!(
            "third_party/Detours/lib.X64/detours.lib is missing.\n\
             From an x64 Native Tools Command Prompt:\n  \
               cd third_party\\Detours\\src\n  \
               set DETOURS_TARGET_PROCESSOR=X64\n  \
               nmake\n\
             Then rerun cargo build."
        );
    }

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=detours");
    println!("cargo:rerun-if-changed={}", lib.display());
    println!("cargo:rerun-if-changed={}", header.display());
}

fn compile_protos(crate_root: &Path) {
    // Prefer the vendored protoc so the build does not depend on the
    // host's PATH. `prost-build` falls back to its pure-Rust parser if
    // the env var is not set, but pinning the binary avoids a class of
    // "works on my machine" issues.
    if let Ok(protoc) = protoc_bin_vendored::protoc_bin_path() {
        env::set_var("PROTOC", protoc);
    }

    let proto_dir = crate_root.join("tools").join("proto");
    let proto_file = proto_dir.join("steam_messages.proto");

    if !proto_file.exists() {
        panic!(
            "tools/proto/steam_messages.proto is missing.\n\
             The NetPacket hook (batch 6e) needs the generated CMsg* types."
        );
    }

    let mut cfg = prost_build::Config::new();
    cfg.compile_protos(&[proto_file.as_path()], &[proto_dir.as_path()])
        .expect("prost-build: compile_protos failed");

    // prost-build names its output after the proto package. With no
    // `package` declaration in `steam_messages.proto`, the generated
    // file is `_.rs`. Rename it to a stable name so `include!` can
    // reference it without an empty-package filename. Renaming here (as
    // opposed to adding a `package` line to the proto) keeps the proto
    // untouched and the generated types at the module root, so
    // `crate::steam::proto::CMsgProtoBufHeader` is the path callers use.
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let generated = out_dir.join("_.rs");
    let target = out_dir.join("steam_messages.rs");
    if generated.exists() {
        // Idempotent: remove any stale target from a previous run so the
        // rename does not fail on Windows.
        let _ = fs::remove_file(&target);
        fs::rename(&generated, &target)
            .unwrap_or_else(|e| panic!("failed to rename {} -> {}: {}",
                generated.display(), target.display(), e));
    } else if !target.exists() {
        panic!(
            "prost-build produced neither `_.rs` nor `steam_messages.rs` in {}.\n\
             Check that tools/proto/steam_messages.proto compiled without errors.",
            out_dir.display()
        );
    }

    println!("cargo:rerun-if-changed={}", proto_file.display());
}

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let crate_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut out = String::from("// Auto-generated by build.rs. Do not edit.\n\n");

    for (lname, lvariant) in LEVELS {
        emit(&mut out, &format!("LOG_{lname}"), "Main", lvariant);
    }
    for (mname, _file) in MODULES {
        let up = mname.to_uppercase();
        for (lname, lvariant) in LEVELS {
            emit(&mut out, &format!("LOG_{up}_{lname}"), mname, lvariant);
        }
    }

    fs::write(out_dir.join("log_macros.rs"), out).unwrap();

    emit_detours_link(&crate_root);
    compile_protos(&crate_root);

    println!("cargo:rerun-if-changed=build.rs");
}
