//! `blow_it_up_again.asi` — the Lua engine functions the Blow It Up Again DLC calls that PC v1.1 never
//! registered.
//!
//! The DLC's scripts were written for the PS3 title update v1.03 and the Xbox title update, which added
//! engine bindings PC never got (`output/_exemap_exports/pc-v11.luareg.tsv` against
//! `ps3-v103-na.luareg.tsv`). On PC an unguarded call to one raises a Lua error the engine swallows
//! without a log line; `Net.DLCGetMountPoint` alone stops the level's sound bootstrap before any bank
//! loads. This ASI registers them through m2-sdk.dll's Lua host, into the engine's own tables.
//!
//! Module map: [`registry`] (every name the ASI adds), [`net`] and [`human`] (what each `Net` and `Human`
//! function does, independent of how it is called), and the Windows-only entry point and registration in `win`.

pub mod human;
pub mod net;
pub mod registry;

#[cfg(all(windows, target_arch = "x86"))]
mod win;
