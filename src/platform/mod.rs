//! Platform abstraction layer. Ported from `src/OSTPlatform/`.
//!
//! ## Fully implemented
//!
//! * [`logging`]  - the log facade             (`Log.cpp`, `include/Log.h`)
//! * [`encoding`] - wide <-> utf8 conversions  (`Encoding.cpp`, `include/Encoding.h`)
//! * [`numbers`]  - strict integer parsers     (`Numbers.cpp`,  `include/Numbers.h`)
//! * [`hash`]     - SHA-256 (uses `sha2`)      (`include/Hash.h`)
//! * [`stopwatch`]- monotonic timing           (`include/Stopwatch.h`)
//! * [`thread`]   - detached thread spawn      (`include/Thread.h`)
//! * [`byte_search`] - in-memory pattern scan  (`include/ByteSearch.h`)
//! * [`process`]  - `is_system_module_path`    (`include/Process.h`)
//!
//! ## Types complete, implementations stubbed
//!
//! These modules have their full public surface defined and will be
//! filled in by sub-batches 2c-2g:
//!
//! * [`detour`]      - Detours transactions         (`include/Detour.h`)
//! * [`dialog`]      - MessageBox wrappers          (`include/Dialog.h`)
//! * [`directory_watch`] - ReadDirectoryChangesW    (`include/DirectoryWatch.h`)
//! * [`dynamic_library`] - LoadLibrary / GetProc    (`include/DynamicLibrary.h`)
//! * [`http`]        - WinHTTP wrapper              (`include/Http.h`)
//! * [`memory`]      - VirtualQuery / VirtualProtect(`include/Memory.h`)
//! * [`pe`]          - PE parsing                   (`include/PE.h`)
//! * [`remote_process`] - remote DLL injection      (`include/RemoteProcess.h`)
//! * [`steam_credential_store`] - HKCU registry     (`include/SteamCredentialStore.h`)
//! * [`trap`]        - VEH install / CONTEXT access (`include/Trap.h`)

pub mod logging;
pub mod encoding;
pub mod numbers;

pub mod byte_search;
pub mod detour;
pub mod dialog;
pub mod directory_watch;
pub mod dynamic_library;
pub mod hash;
pub mod http;
pub mod memory;
pub mod pe;
pub mod process;
pub mod remote_process;
pub mod steam_credential_store;
pub mod stopwatch;
pub mod thread;
pub mod trap;

pub mod windows;
