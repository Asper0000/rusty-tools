//! Generated protobuf types for Steam messages.
//!
//! Source of truth: `proto/steam_messages.proto`. The build script runs
//! `prost-build` and drops the generated Rust module into `OUT_DIR`;
//! this file includes it.
//!
//! Field numbers and wire types are preserved exactly from the
//! `.proto`. The prost runtime uses a different in-memory
//! representation from the C++ protobuf runtime, so any code path that
//! needs to pass a message through a C++ protobuf helper (e.g.
//! `RepeatedField<uint32>::Add`) cannot be ported directly. See
//! `STUBS.md` "Known behavioural divergences".
//!
//! ## Naming
//!
//! prost converts the proto message and field names to Rust
//! conventions. Message names like `CMsgProtoBufHeader` stay as-is;
//! field names like `jobid_source` stay in snake_case; enum variants
//! are prefixed with the enum name and PascalCased. `optional` fields
//! become `Option<T>`; `repeated` fields become `Vec<T>`.

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(clippy::all)]

include!(concat!(env!("OUT_DIR"), "/steam_messages.rs"));
