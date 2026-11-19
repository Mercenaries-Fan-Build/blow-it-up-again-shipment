//! Every Lua name this ASI adds to the game, as `(table, field)`. The Windows side registers exactly these
//! (it stops the game at start-up when what it registered differs), and `tests/lua_surface.rs` checks that
//! the DLC's scripts name no engine function that neither PC v1.1, this list, nor a Shipment `manifest.yaml`
//! requires provides.

pub const ADDED: &[(&str, &str)] = &[
    ("Gui", "GetMapCorrectionOffset"),
    ("Gui", "SetMapCorrectionOffset"),
    ("Gui", "_MarkerAddDiscDLC"),
    ("Human", "SetChatterSet"),
    ("Net", "DLCGetMountPoint"),
    ("Object", "GetInfiniteAmmo"),
];

/// The fields this ASI sets on each state to one of [`ADDED`], as `(table, field, table, field)`: PS3 v1.03's
/// embedded Gui chunk adds `_G.Marker.AddDiscDLC = Gui._MarkerAddDiscDLC` to the `Marker` table PC's Gui
/// chunk builds.
pub const ALIASES: &[(&str, &str, &str, &str)] = &[("Marker", "AddDiscDLC", "Gui", "_MarkerAddDiscDLC")];

/// [`ADDED`] and the alias fields of [`ALIASES`]: every `(table, field)` the ASI provides.
pub fn provided() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = ADDED.iter().map(|(t, f)| (t.to_string(), f.to_string())).collect();
    out.extend(ALIASES.iter().map(|(t, f, _, _)| (t.to_string(), f.to_string())));
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_alias_names_an_added_function() {
        for (t, f, to_t, to_f) in ALIASES {
            assert!(ADDED.contains(&(*to_t, *to_f)), "{t}.{f} aliases {to_t}.{to_f}, which the ASI does not add");
            assert!(!ADDED.contains(&(*t, *f)), "{t}.{f} is both added and an alias");
        }
    }
}
