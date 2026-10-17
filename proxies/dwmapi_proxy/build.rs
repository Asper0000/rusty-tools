//! Emits the dwmapi export table as two pieces:
//!
//!   1. `.drectve` sections (object-file linker directives) for the 46
//!      **named** rows in `dwmapi.def`. These are generated into
//!      `$OUT_DIR/dwmapi_exports_drectve.rs`, which `lib.rs` `include!`s.
//!      Each row becomes one `#[used]` static in `#[link_section = ".drectve"]`
//!      holding the NUL-terminated text of an `/EXPORT:` option.
//!
//!   2. A `/DEF:` argument for the 60 **ordinal-only** rows
//!      (`#N=DWMAPI.#N @N NONAME`). This form already links cleanly and
//!      is left alone; the .def is written into `$OUT_DIR`.
//!
//! ## Why `.drectve` for the named rows
//!
//! Both `.def` files and `/EXPORT:` on the linker command line normalise
//! `Module.Name` to a local symbol export whenever `Module` matches the
//! name of the DLL currently being linked. Our output is `dwmapi.dll`, so
//! every `Name=DWMAPI.Name` in either form was rewritten by LINK to
//! `Name=Name`. The 33 exports that the Windows SDK's `DWMAPI.lib`
//! happens to re-export resolved by accident; the 13 that it does not
//! (`DllCanUnloadNow`, `DllGetClassObject`, `DwmTetherTextContact`, and
//! the ten `Dwmp*` internals) failed with `LNK2001: unresolved external
//! symbol`, terminating the build with a fatal `LNK1120`.
//!
//! The C++ build sidestepped this with
//! `#pragma comment(linker, "/EXPORT:Name=DWMAPI.Name,@Ord")`, which
//! emits the text of the option into a `.drectve` section. LINK reads
//! `.drectve` bytes as additional command-line options, but through a
//! different code path than `/EXPORT:` on the actual command line: the
//! directive's module component is preserved verbatim rather than being
//! matched against the output DLL name. This build script generates the
//! same object-file directive from Rust:
//!
//! ```text
//! #[link_section = ".drectve"]
//! #[used]
//! static EXPORT_0: [u8; N] = *b"/EXPORT:DllCanUnloadNow=DWMAPI.DllCanUnloadNow,@111\0";
//! ```
//!
//! `dwmapi.def` remains the single source of truth. It is parsed here,
//! split into named and ordinal-only entries, and never modified.
//!
//! ## Runtime resolution
//!
//! Windows' loader resolves a forwarder to a module by skipping the DLL
//! that is currently being loaded, so `DWMAPI.DllCanUnloadNow` from
//! inside our `dwmapi.dll` resolves to the system32 copy. This is
//! documented loader behaviour; the whole hijack depends on it.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let def = manifest_dir.join("dwmapi.def");

    if !def.exists() {
        panic!(
            "dwmapi.def not found at {}.\n\
             The proxy DLL cannot be built without its export table.",
            def.display()
        );
    }

    println!("cargo:rerun-if-changed=dwmapi.def");

    // LNK4104 nags that DllCanUnloadNow and DllGetClassObject
    // should be PRIVATE. That is true for a normal COM DLL; this is
    // a forwarder proxy, and every export in the table has to be
    // visible. Suppress the warning.
    println!("cargo:rustc-cdylib-link-arg=/ignore:4104");

    let content = fs::read_to_string(&def)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", def.display(), e));

    let mut ordinal_only: Vec<String> = Vec::new();
    let mut named_directives: Vec<String> = Vec::new();
    let mut in_exports = false;

    for (lineno, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() { continue; }
        if line.starts_with(';') { continue; }
        if line.eq_ignore_ascii_case("EXPORTS") {
            in_exports = true;
            continue;
        }
        if !in_exports { continue; }

        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 2 {
            panic!(
                "dwmapi.def:{}: expected `name=target @ordinal [NONAME]`, got `{}`",
                lineno + 1,
                line
            );
        }
        let name_eq_target = tokens[0];
        let ordinal_tok = tokens[1];
        let noname = tokens.iter().any(|t| t.eq_ignore_ascii_case("NONAME"));

        if !ordinal_tok.starts_with('@') {
            panic!(
                "dwmapi.def:{}: expected `@<ordinal>`, got `{}`",
                lineno + 1,
                ordinal_tok
            );
        }
        let ordinal = &ordinal_tok[1..];
        let _: u32 = ordinal.parse().unwrap_or_else(|_| {
            panic!(
                "dwmapi.def:{}: ordinal `{}` is not an integer",
                lineno + 1,
                ordinal
            )
        });

        let eq = match name_eq_target.find('=') {
            Some(i) => i,
            None => panic!(
                "dwmapi.def:{}: expected `name=target`, got `{}`",
                lineno + 1,
                name_eq_target
            ),
        };
        let lhs = &name_eq_target[..eq];
        let rhs = &name_eq_target[eq + 1..];

        if lhs.starts_with('#') {
            ordinal_only.push(line.to_string());
        } else {
            let directive = if noname {
                format!("/EXPORT:{}={},@{},NONAME", lhs, rhs, ordinal)
            } else {
                format!("/EXPORT:{}={},@{}", lhs, rhs, ordinal)
            };
            named_directives.push(directive);
        }
    }

    if named_directives.is_empty() && ordinal_only.is_empty() {
        panic!("dwmapi.def parsed to zero exports");
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // ── 1. Ordinal-only entries → temporary .def → /DEF: ─────────────
    let temp_def = out_dir.join("dwmapi_ordinal_only.def");
    let mut temp = String::from("EXPORTS\n");
    for row in &ordinal_only {
        temp.push_str(row);
        temp.push('\n');
    }
    fs::write(&temp_def, temp.as_bytes())
        .unwrap_or_else(|e| panic!("failed to write {}: {}", temp_def.display(), e));
    println!("cargo:rustc-cdylib-link-arg=/DEF:{}", temp_def.display());

    // ── 2. Named entries → .drectve statics → generated .rs ──────────
    //
    // Each directive becomes its own `#[used]` static. They cannot be
    // combined: the linker reads the whole of each static's bytes as
    // options, and one long `[u8; N]` would let LLVM's `#[used]`
    // placement survive only as one huge blob, which the linker would
    // then try to parse as a single malformed option string.
    let mut rs = String::new();
    rs.push_str("// Auto-generated by dwmapi_proxy/build.rs. Do not edit.\n");
    rs.push_str("// One `#[link_section = \".drectve\"]` static per named export in dwmapi.def.\n");
    rs.push_str("// Equivalent to the C++ `#pragma comment(linker, \"/EXPORT:...\")` block.\n\n");
    for (i, directive) in named_directives.iter().enumerate() {
        let bytes: Vec<u8> = directive
            .as_bytes()
            .iter()
            .copied()
            .chain(std::iter::once(0u8))
            .collect();
        rs.push_str(&format!(
            "#[link_section = \".drectve\"]\n\
             #[used]\n\
             static DWMAPI_EXPORT_{i}: [u8; {n}] = *b\"{escaped}\";\n\n",
            i = i,
            n = bytes.len(),
            escaped = escape_bytes_literal(&bytes),
        ));
    }
    let rs_path = out_dir.join("dwmapi_exports_drectve.rs");
    fs::write(&rs_path, rs.as_bytes())
        .unwrap_or_else(|e| panic!("failed to write {}: {}", rs_path.display(), e));

    eprintln!(
        "dwmapi_proxy: {} named export(s) via .drectve, {} ordinal-only via /DEF",
        named_directives.len(),
        ordinal_only.len()
    );
}

/// Escape a byte sequence for inclusion inside a Rust `b"..."` literal.
/// The directives we generate are ASCII-only (`/EXPORT:...` with a
/// trailing NUL), so the escape set is small: `\\`, `"`, `\n`, `\r`,
/// `\t`, and any byte >= 0x80 or < 0x20 which we emit as `\xNN`.
fn escape_bytes_literal(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'"'  => out.push_str("\\\""),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7E => out.push(b as char),
            _ => out.push_str(&format!("\\x{:02X}", b)),
        }
    }
    out
}
