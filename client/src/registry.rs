//! Every Lua name this ASI adds to the game, as `(table, field)`. The Windows side registers exactly these
//! (it stops the game at start-up when what it registered differs), and `tests/lua_surface.rs` checks that
//! the DLC's scripts name no engine function that neither PC v1.1, this list, nor a Shipment `manifest.yaml`
//! requires provides.

pub const ADDED: &[(&str, &str)] = &[("Human", "SetChatterSet"), ("Net", "DLCGetMountPoint")];
