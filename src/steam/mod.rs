//! Steam foundation types. Ported from `src/Steam/`.
//!
//! * [`types`]         - primitive aliases and ID handles       (`Types.h`)
//! * [`enums`]         - all steamclient enums                  (`Enums.h`)
//! * [`structs`]       - layout-critical in-memory structures   (`Structs.h`)
//! * [`net_packet`]    - CNetPacket layout detection            (`NetPacket.h`)
//! * [`callback`]      - callback structs                       (`Callback.h`)
//! * [`ipc_messages`]  - IPC wire schema                        (`IPCMessages.steamd`)
//! * [`ipc_facades`]   - typed request/response views           (`IPCMessages.gen.h`)
//! * [`proto`]         - generated protobuf types               (`steam_messages.proto`)

pub mod types;
pub mod enums;
pub mod structs;
pub mod net_packet;
pub mod callback;
pub mod ipc_messages;
pub mod ipc_facades;
pub mod proto;
