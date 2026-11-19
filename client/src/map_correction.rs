//! The map correction PS3 v1.03 added: `x`, `z` and `scale`, which the minimap, the GPS line, the radar, the
//! co-op midpoint and the PDA map click apply between world positions and the PDA movie's map. PC v1.1 has
//! no correction: it hard-codes `x = 35`, `z = 40` and no scale (see [`crate::sites`] for each hooked site).
//!
//! * `Gui.SetMapCorrectionOffset(x, z, scale)` → nothing. PS3 v1.03 (`0x00522C38`) reads three numbers with
//!   its number reader (`0x005037E8`: `lua_isnumber`, so a numeric string counts, then `lua_tonumber`) and
//!   writes all three, or with any of them missing writes nothing, pushes nil and returns 1.
//! * `Gui.GetMapCorrectionOffset()` → `x, z, scale` (`0x00522108`), the three values as Lua numbers (the
//!   game's `lua_Number` is an f32, so they read back exactly).
//!
//! The values start at, and the GUI system's release (PS3 PgSysGUI vtable slot 7, `0x003D4908`) resets them
//! to, 35, 40 and 1.0. The DLC's PDA `Init` sets `(0, 0, 1.1377778)`.
//!
//! What each consumer computes ([`to_map`], [`radar_ratio`], [`midpoint`], [`from_map`]) is written here as
//! PS3 v1.03 computes it, in f32, and the ASI's hooks compute the same with the same instructions. At the
//! defaults every one but the PDA click gives PC's own result bit for bit. The PDA click does not: PC stores
//! the clicked map position `Q` raw (v1.1 `FUN_006A0DD0`, `+0x39C`/`+0x3A0`) while PS3 v1.03 (`0x001CF868`)
//! stores `Q / scale − x`, so with the ASI a GPS destination set on the PDA lands 35 m west and 40 m south
//! of where retail PC put it. That is where the minimap shows the clicked point (world → map adds 35/40 in
//! `FUN_006201A0`, `FUN_00620AA0` and `FUN_00621620`), and where the base game's own Lua already reads the
//! same click (`HandleBeaconCheck`, `mrxguipda.lua`: `Q − 35`, `Q − 40`).
//!
//! The click handler has a second caller: the co-op receive of message `0x14` (PC v1.1 `0x006FE2B6`, PS3
//! v1.03 `0x0003464C`), which formats the partner's stored position as `"%d,%d"` and calls the handler with
//! its parameter 1. PS3 v1.03 corrects that value again, so a partner's destination is corrected twice. The
//! port corrects only the PDA's own click (parameter 0) and stores a partner's as it arrives, already
//! corrected by the sender ([`pda_click_store`]).

use core::sync::atomic::{AtomicU32, Ordering};

use mercs2_formats::mod_runtime::lua::{Lua, Ret, TNUMBER, TSTRING};

pub const DEFAULT_X: f32 = 35.0;
pub const DEFAULT_Z: f32 = 40.0;
pub const DEFAULT_SCALE: f32 = 1.0;

/// The three values, as f32 bits. `repr(C)`: the hooks read `x` at +0, `z` at +4 and `scale` at +8.
#[repr(C)]
pub struct Offsets {
    pub x: AtomicU32,
    pub z: AtomicU32,
    pub scale: AtomicU32,
}

impl Offsets {
    pub const fn new() -> Offsets {
        Offsets {
            x: AtomicU32::new(DEFAULT_X.to_bits()),
            z: AtomicU32::new(DEFAULT_Z.to_bits()),
            scale: AtomicU32::new(DEFAULT_SCALE.to_bits()),
        }
    }

    pub fn get(&self) -> (f32, f32, f32) {
        let f = |a: &AtomicU32| f32::from_bits(a.load(Ordering::Acquire));
        (f(&self.x), f(&self.z), f(&self.scale))
    }

    pub fn set(&self, x: f32, z: f32, scale: f32) {
        self.x.store(x.to_bits(), Ordering::Release);
        self.z.store(z.to_bits(), Ordering::Release);
        self.scale.store(scale.to_bits(), Ordering::Release);
    }

    /// What the GUI system's release does.
    pub fn reset(&self) {
        self.set(DEFAULT_X, DEFAULT_Z, DEFAULT_SCALE);
    }
}

impl Default for Offsets {
    fn default() -> Self {
        Offsets::new()
    }
}

/// The values the hooks read and the bindings write.
pub static OFFSETS: Offsets = Offsets::new();

/// A world coordinate on the map (minimap `0x005DF650`, GPS line `0x005DE020`): `(world + offset) * scale`.
pub fn to_map(world: f32, offset: f32, scale: f32) -> f32 {
    (world + offset) * scale
}

/// The radar blip's distance factor (`0x005DD740`): `(radius / range) * scale`.
pub fn radar_ratio(radius: f32, range: f32, scale: f32) -> f32 {
    (radius / range) * scale
}

/// The co-op midpoint (`0x005DD540`): `((p0 + p1) * half + offset) * scale`, `half` the engine's 0.5.
pub fn midpoint(p0: f32, p1: f32, half: f32, offset: f32, scale: f32) -> f32 {
    ((p0 + p1) * half + offset) * scale
}

/// The PDA click's stored world coordinate (`0x001CF868`): `q / scale − offset`, `q` the clicked map
/// position the movie sent, as the f32 the click handler converts it to.
pub fn from_map(q: f32, offset: f32, scale: f32) -> f32 {
    q / scale - offset
}

/// What the PDA click handler stores for `q` with the hook: the PDA's own click (`from_partner` false, the
/// handler's parameter 0) is converted by [`from_map`]; a co-op partner's (parameter 1, the receive of
/// message `0x14`) is kept, as the sender already converted it.
pub fn pda_click_store(q: f32, from_partner: bool, offset: f32, scale: f32) -> f32 {
    if from_partner { q } else { from_map(q, offset, scale) }
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// `strtoul(s, &end, 16)` over `s` (C locale): the value (negated when signed, `ULONG_MAX` on overflow) and
/// where it stopped, or `None` when it converts nothing.
fn strtoul16(s: &[u8]) -> Option<(f64, usize)> {
    let mut i = s.iter().take_while(|&&c| is_space(c)).count();
    let negative = match s.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    if s.get(i) == Some(&b'0') && matches!(s.get(i + 1), Some(b'x' | b'X')) && s.get(i + 2).is_some_and(u8::is_ascii_hexdigit) {
        i += 2;
    }
    let start = i;
    let mut v: u32 = 0;
    let mut overflow = false;
    while let Some(d) = s.get(i).and_then(|c| (*c as char).to_digit(16)) {
        match v.checked_mul(16).and_then(|v| v.checked_add(d)) {
            Some(n) => v = n,
            None => overflow = true,
        }
        i += 1;
    }
    if i == start {
        return None;
    }
    let v = if overflow { u32::MAX } else if negative { v.wrapping_neg() } else { v };
    Some((f64::from(v), i))
}

/// `strtod(s, &end)` over `s` in the C locale as MSVCR80 parses it (decimal only: no `inf`, `nan` or hex
/// float): the value and where it stopped, or `None` when it converts nothing.
fn strtod(s: &[u8]) -> Option<(f64, usize)> {
    let mut i = s.iter().take_while(|&&c| is_space(c)).count();
    let start = i;
    if matches!(s.get(i), Some(b'+' | b'-')) {
        i += 1;
    }
    let int = s[i..].iter().take_while(|c| c.is_ascii_digit()).count();
    i += int;
    let mut frac = 0;
    if s.get(i) == Some(&b'.') {
        frac = s[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
        if int + frac > 0 {
            i += 1 + frac;
        }
    }
    if int + frac == 0 {
        return None;
    }
    if matches!(s.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(s.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let digits = s[j..].iter().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 {
            i = j + digits;
        }
    }
    let text = std::str::from_utf8(&s[start..i]).expect("ASCII digits, sign, point and exponent");
    let text = text.strip_suffix('.').unwrap_or(text);
    let v: f64 = text.parse().unwrap_or_else(|e| panic!("{text:?} is a decimal number by construction: {e}"));
    Some((v, i))
}

/// Lua 5.1's `luaO_str2d` (the conversion `lua_isnumber` and `lua_tonumber` make of a string), into the
/// game's f32 `lua_Number`.
pub fn str2number(s: &[u8]) -> Option<f32> {
    let (mut v, mut end) = strtod(s)?;
    if matches!(s.get(end), Some(b'x' | b'X')) {
        (v, end) = strtoul16(s)?;
    }
    if s[end..].iter().all(|&c| is_space(c)) { Some(v as f32) } else { None }
}

/// Argument `i` as PS3's number reader takes it: a number, or a string Lua converts to one.
fn number_arg<L: Lua>(s: &L, i: i32) -> Option<f32> {
    match s.arg_type(i) {
        TNUMBER => s.arg_number(i).map(|n| n as f32),
        TSTRING => s.arg_string(i).and_then(|b| str2number(&b)),
        _ => None,
    }
}

/// `Gui.SetMapCorrectionOffset(x, z, scale)`.
pub fn set_map_correction_offset<L: Lua>(s: &mut L, o: &Offsets) -> Ret {
    match (number_arg(s, 0), number_arg(s, 1), number_arg(s, 2)) {
        (Some(x), Some(z), Some(scale)) => {
            o.set(x, z, scale);
            Ret::Values(0)
        }
        _ => {
            s.push_nil();
            Ret::Values(1)
        }
    }
}

/// `Gui.GetMapCorrectionOffset()`.
pub fn get_map_correction_offset<L: Lua>(s: &mut L, o: &Offsets) -> Ret {
    let (x, z, scale) = o.get();
    for v in [x, z, scale] {
        s.push_number(f64::from(v));
    }
    Ret::Values(3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mercs2_formats::mod_runtime::lua::fake::{FakeLua, Val, s};

    const DLC_SCALE: f32 = 1.137_777_8;

    #[test]
    fn the_values_start_at_and_reset_to_pcs_constants() {
        let o = Offsets::new();
        assert_eq!(o.get(), (35.0, 40.0, 1.0));
        o.set(0.0, 0.0, DLC_SCALE);
        assert_eq!(o.get(), (0.0, 0.0, DLC_SCALE));
        o.reset();
        assert_eq!(o.get(), (35.0, 40.0, 1.0));
        assert_eq!(
            [DEFAULT_X, DEFAULT_Z].map(f32::to_bits),
            [crate::sites::PC_X, crate::sites::PC_Z].map(f32::to_bits),
            "the defaults are the constants the hooked sites add"
        );
    }

    #[test]
    fn set_writes_all_three_and_get_reads_them_back_exactly() {
        let o = Offsets::new();
        let mut l = FakeLua::with(vec![Val::Num(0.0), Val::Num(0.0), Val::Num(f64::from(DLC_SCALE))]);
        assert_eq!(set_map_correction_offset(&mut l, &o), Ret::Values(0));
        assert!(l.pushed.is_empty());
        let mut l = FakeLua::default();
        assert_eq!(get_map_correction_offset(&mut l, &o), Ret::Values(3));
        assert_eq!(l.pushed, [Val::Num(0.0), Val::Num(0.0), Val::Num(f64::from(DLC_SCALE))]);
    }

    #[test]
    fn get_returns_x_z_scale_in_that_order() {
        let o = Offsets::new();
        o.set(1.5, -2.25, 3.0);
        let mut l = FakeLua::with(vec![Val::Num(9.0)]);
        assert_eq!(get_map_correction_offset(&mut l, &o), Ret::Values(3), "it reads no argument");
        assert_eq!(l.pushed, [Val::Num(1.5), Val::Num(-2.25), Val::Num(3.0)]);
    }

    #[test]
    fn a_missing_or_non_numeric_argument_writes_nothing_and_returns_nil() {
        for args in [
            vec![],
            vec![Val::Num(1.0), Val::Num(2.0)],
            vec![Val::Num(1.0), Val::Nil, Val::Num(3.0)],
            vec![s("x"), Val::Num(2.0), Val::Num(3.0)],
            vec![Val::Num(1.0), Val::Num(2.0), Val::Bool(true)],
            vec![Val::Num(1.0), Val::Num(2.0), Val::Table],
            vec![Val::Light(1), Val::Num(2.0), Val::Num(3.0)],
            vec![Val::Num(1.0), Val::Num(2.0), s("3 x")],
        ] {
            let o = Offsets::new();
            let mut l = FakeLua::with(args.clone());
            assert_eq!(set_map_correction_offset(&mut l, &o), Ret::Values(1), "{args:?}");
            assert_eq!(l.pushed, [Val::Nil], "{args:?}");
            assert_eq!(o.get(), (35.0, 40.0, 1.0), "{args:?}");
        }
    }

    #[test]
    fn numeric_strings_count_as_lua_converts_them_and_extra_arguments_are_ignored() {
        let o = Offsets::new();
        let mut l = FakeLua::with(vec![s(" 12.5 "), s("0x10"), s("-1e1"), Val::Num(7.0)]);
        assert_eq!(set_map_correction_offset(&mut l, &o), Ret::Values(0));
        assert_eq!(o.get(), (12.5, 16.0, -10.0));
    }

    #[test]
    fn str2number_follows_luao_str2d() {
        for (text, want) in [
            ("1", Some(1.0)),
            ("  -3.5", Some(-3.5)),
            ("2.", Some(2.0)),
            (".5", Some(0.5)),
            ("1e3", Some(1000.0)),
            ("1e", None),
            ("1e+", None),
            ("4\t\n", Some(4.0)),
            ("0x1F", Some(31.0)),
            ("0X1f ", Some(31.0)),
            ("-0x1", Some(4_294_967_295.0)),
            ("0x", None),
            ("", None),
            ("   ", None),
            (".", None),
            ("-", None),
            ("1 2", None),
            ("inf", None),
            ("nan", None),
            ("1.5x", None),
            ("0x1p3", None),
            ("1e400", Some(f32::INFINITY)),
        ] {
            assert_eq!(str2number(text.as_bytes()), want, "{text:?}");
        }
    }

    /// Positions across the world (±3900 m) and fine fractions.
    fn worlds() -> Vec<f32> {
        let mut v = vec![0.0, -0.0, 0.1, -0.1, 1.0 / 3.0, 3899.999, -3899.999, 1234.567, -0.000_01];
        v.extend((-400..=400).map(|k| k as f32 * 9.75 + 0.3));
        v
    }

    #[test]
    fn at_the_defaults_every_world_to_map_formula_is_pcs_own_bit_for_bit() {
        for w in worlds() {
            assert_eq!(to_map(w, DEFAULT_X, DEFAULT_SCALE).to_bits(), (w + 35.0).to_bits(), "x {w}");
            assert_eq!(to_map(w, DEFAULT_Z, DEFAULT_SCALE).to_bits(), (w + 40.0).to_bits(), "z {w}");
            let p1 = w * 0.7 - 11.0;
            assert_eq!(midpoint(w, p1, 0.5, DEFAULT_X, DEFAULT_SCALE).to_bits(), ((w + p1) * 0.5 + 35.0).to_bits(), "midpoint {w}");
            for range in [50.0f32, 120.0, 333.3] {
                assert_eq!(radar_ratio(w.abs() + 1.0, range, DEFAULT_SCALE).to_bits(), ((w.abs() + 1.0) / range).to_bits(), "radar {w}");
            }
        }
    }

    #[test]
    fn the_pda_click_takes_the_offset_off_where_pc_took_none() {
        for q in [0.0f32, 4.0, 1024.0, -2048.0, 8000.0] {
            assert_eq!(from_map(q, DEFAULT_X, DEFAULT_SCALE), q - 35.0);
            assert_eq!(from_map(q, DEFAULT_Z, DEFAULT_SCALE), q - 40.0);
            assert_eq!(from_map(q, 0.0, DLC_SCALE), q / DLC_SCALE);
        }
        // The DLC's values invert the minimap's world → map.
        let w = 812.5f32;
        assert!((from_map(to_map(w, 0.0, DLC_SCALE), 0.0, DLC_SCALE) - w).abs() < 1e-3);
    }

    #[test]
    fn only_the_pdas_own_click_is_converted_and_a_partners_is_stored_as_it_arrives() {
        for (offset, scale) in [(DEFAULT_X, DEFAULT_SCALE), (DEFAULT_Z, DEFAULT_SCALE), (0.0, DLC_SCALE)] {
            for q in [0.0f32, 4.0, 1024.0, -2048.0] {
                let own = pda_click_store(q, false, offset, scale);
                assert_eq!(own.to_bits(), from_map(q, offset, scale).to_bits(), "own click {q}");
                // The sender stored `own`; the receive formats it as "%d,%d" and the receiver keeps it.
                let sent = own.trunc();
                assert_eq!(pda_click_store(sent, true, offset, scale).to_bits(), sent.to_bits(), "partner's {sent}");
            }
        }
        // PS3 v1.03 would store a partner's `trunc(Q − 35) − 35` at the defaults; the port stores `Q − 35`.
        assert_eq!(pda_click_store(pda_click_store(100.0, false, DEFAULT_X, 1.0).trunc(), true, DEFAULT_X, 1.0), 65.0);
    }
}
