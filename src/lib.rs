//! rusty_tools - Rust port of OpenSteamTool / BetterSteamTools.

#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

// Macros must be defined before any `mod` declaration so their textual
// scope covers every subsequent module. This is a build.rs output.
include!(concat!(env!("OUT_DIR"), "/log_macros.rs"));

pub mod bridge;
pub mod utils;
pub mod steam;
pub mod platform;
pub mod pipe;
pub mod hook;

#[cfg(windows)]
pub mod dllmain;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn init() -> anyhow::Result<()> {
    tracing::info!("rusty_tools v{} starting", VERSION);
    Ok(())
}
