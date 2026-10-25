//! Island Shell's module protocol (docs/design/modules-handoff.md §4): what the base and a module
//! backend say to each other, and the `module.json` manifest. Shared by the base (src-tauri), the
//! module SDK (crates/island-module) and the pack tool. No platform code.
//!
//! **Transport.** The base starts a backend with its stdin and stdout connected to anonymous pipes
//! that only that process inherits, and reads its stderr as the module's log. Each message is one line
//! of UTF-8 JSON (JSON-RPC 2.0) ending in `\n`, at most [`MAX_FRAME`] bytes.

pub mod frame;
pub mod manifest;
pub mod msg;

pub use frame::{write_message, FrameError, FrameReader, MAX_FRAME};
pub use manifest::Manifest;
pub use msg::{Message, RpcError};

/// The module API version this crate speaks. The base accepts modules built for `API` and `API - 1`.
pub const API: u32 = 1;

/// Method names. Base → module methods start with `island.`, module → base ones with `host.`;
/// anything else is a UI call forwarded from `mod_call`.
pub mod method {
    pub const INIT: &str = "island.init";
    pub const TICK: &str = "island.tick";
    pub const EVENT: &str = "island.event";
    pub const SHUTDOWN: &str = "island.shutdown";
    pub const EMIT: &str = "host.emit";
    pub const NOTIFY: &str = "host.notify";
    pub const ACTIVITY: &str = "host.activity";
    pub const CALENDAR: &str = "host.calendar";
    pub const PUBLISH: &str = "host.publish";
    pub const LOG: &str = "host.log";

    /// UI calls can't use the reserved prefixes.
    pub fn is_ui_call(name: &str) -> bool {
        !name.is_empty() && !name.starts_with("island.") && !name.starts_with("host.")
    }
}

/// JSON-RPC `error.code` values (§4.6).
pub mod code {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const UNKNOWN_METHOD: i64 = -32601;
    pub const BAD_PARAMS: i64 = -32602;
    /// Failed; the message is shown to the user.
    pub const FAILED: i64 = -32000;
    /// Not set up: the UI shows its setup state.
    pub const NOT_SET_UP: i64 = -32001;
    /// Offline, or the service is unreachable.
    pub const OFFLINE: i64 = -32002;
    /// The module is stopped or crashed (sent by the base's supervisor).
    pub const STOPPED: i64 = -32003;
}

/// Topic names the base publishes (§4.7). Modules publish their own (`media.state`, `presence`).
pub mod topic {
    pub const ISLAND_GEOMETRY: &str = "island.geometry";
    pub const FULLSCREEN: &str = "fullscreen";
    pub const SESSION: &str = "session";
    pub const POWER: &str = "power";
    pub const THEME: &str = "theme";
    pub const MEDIA_STATE: &str = "media.state";
    pub const PRESENCE: &str = "presence";

    /// Lowercase words separated by dots or dashes.
    pub fn valid(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 64
            && name.split('.').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
    }
}

/// `^[a-z][a-z0-9-]{1,31}$` (§3).
pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    (2..=32).contains(&b.len()) && b[0].is_ascii_lowercase() && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}
