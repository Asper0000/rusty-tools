//! Windows-specific backends. Source: `OSTPlatform/Windows/`
//!
//! * [`handles`]      - RAII wrappers for Win32 resources (`Handles.h`)
//! * [`nt_abi`]       - PEB / RTL structs and function types (`NtAbi.h`)
//! * [`detours_ffi`]  - raw FFI to the vendored Microsoft Detours
//!   static library. Declared on Windows only; the `detour` module in
//!   `platform/` is the safe wrapper callers use.

pub mod handles;
pub mod nt_abi;

#[cfg(windows)]
pub mod detours_ffi;
