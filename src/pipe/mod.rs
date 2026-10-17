//! Pipe layer. Ported from `src/Pipe/`.
//!
//! * [`pipe_types`]        - `ProcessKey`, `PipeKey`, `PipeContext`
//! * [`process_inspector`] - process snapshot + Steam env query
//! * [`protection_scan`]   - Denuvo-specific module scanner
//! * [`denuvo_auth`]       - per-process Denuvo detection + authorization state
//! * [`injection`]         - per-pipe DLL injection
//! * [`pipe_manager`]      - `OnHandshake` entry point

pub mod pipe_types;
pub mod process_inspector;
pub mod injection;
pub mod pipe_manager;
