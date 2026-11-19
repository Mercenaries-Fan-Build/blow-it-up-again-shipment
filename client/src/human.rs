//! What each `Human` function does, independent of how it is called.
//!
//! * `Human.SetChatterSet(character, name)` → nothing. PS3 v1.03 (`0x005300F8`) reads the character (an object
//!   argument) and the name (a string, hashed with `pandemic_hash_m2`; an empty one fails). With either missing it
//!   returns nil. Otherwise it writes the hash into the character's `ChatterSet` record, which is the engine's
//!   lookup's answer and may be the character's template's record. With no record it adds one and, when that
//!   grew the container, does what the engine's own `ChatterSet` field writer does after an add: bump the
//!   container's counter and, while the flag is set, mark the character's component bit. The engine's chatter
//!   reads the value as the name of a chatter asset and skips the chatter when that asset is not loaded.

use mercs2_formats::hash::pandemic_hash_m2_bytes;
use mercs2_formats::mod_runtime::lua::{Lua, Ret};

/// The `ChatterSet` component, as the engine's functions reach it.
pub trait ChatterComponents {
    /// Write `value` into the record the engine's lookup returns for `guid`; false when it returns none.
    fn write_existing(&mut self, guid: u32, value: u32) -> bool;
    /// The container's record count.
    fn count(&self) -> u32;
    /// The engine's add: a record for `guid` holding `value`.
    fn add(&mut self, guid: u32, value: u32);
    /// The container's counter the field writer bumps after a new record.
    fn bump(&mut self);
    /// Whether the field writer would mark the entity's component bit now.
    fn flag(&self) -> bool;
    /// Mark `guid`'s bit for the component.
    fn mark(&mut self, guid: u32);
}

/// `Human.SetChatterSet(character, name)`.
pub fn set_chatter_set<L: Lua, C: ChatterComponents>(s: &mut L, c: &mut C) -> Ret {
    let guid = s.arg_object(0);
    let hash = s.arg_string(1).map(|name| pandemic_hash_m2_bytes(&name)).filter(|&h| h != 0);
    let (Some(guid), Some(hash)) = (guid, hash) else {
        s.push_nil();
        return Ret::Values(1);
    };
    if c.write_existing(guid, hash) {
        return Ret::Values(0);
    }
    let before = c.count();
    c.add(guid, hash);
    if c.count() != before {
        c.bump();
        if c.flag() {
            c.mark(guid);
        }
    }
    Ret::Values(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mercs2_formats::mod_runtime::lua::fake::{FakeLua, Val, s};
    use std::collections::BTreeMap;

    /// A container: the records, the counter and flag, and the entities whose bit was marked.
    #[derive(Default)]
    struct Fake {
        records: BTreeMap<u32, u32>,
        bumps: u32,
        flag: bool,
        marked: Vec<u32>,
    }

    impl ChatterComponents for Fake {
        fn write_existing(&mut self, guid: u32, value: u32) -> bool {
            match self.records.get_mut(&guid) {
                Some(v) => {
                    *v = value;
                    true
                }
                None => false,
            }
        }
        fn count(&self) -> u32 {
            self.records.len() as u32
        }
        fn add(&mut self, guid: u32, value: u32) {
            self.records.insert(guid, value);
        }
        fn bump(&mut self) {
            self.bumps += 1;
        }
        fn flag(&self) -> bool {
            self.flag
        }
        fn mark(&mut self, guid: u32) {
            self.marked.push(guid);
        }
    }

    const PALIN: u32 = 0xfe96_ac37;

    #[test]
    fn the_name_hash_is_the_chatter_assets_name() {
        assert_eq!(pandemic_hash_m2_bytes(b"Palin_Palin01"), PALIN);
        assert_eq!(pandemic_hash_m2_bytes(b"Obama_Obama01"), 0xb287_020b);
    }

    #[test]
    fn a_character_with_a_record_gets_the_hash_written_and_nothing_else() {
        let mut c = Fake { records: BTreeMap::from([(0x4000_0915, 7)]), flag: true, ..Fake::default() };
        let mut l = FakeLua::with(vec![Val::Light(0x4000_0915), s("Palin_Palin01")]);
        assert_eq!(set_chatter_set(&mut l, &mut c), Ret::Values(0));
        assert_eq!(c.records[&0x4000_0915], PALIN);
        assert_eq!((c.bumps, c.marked.len()), (0, 0));
        assert!(l.pushed.is_empty());
    }

    #[test]
    fn a_character_without_one_gets_a_record_the_counter_and_while_the_flag_is_set_its_bit() {
        let mut c = Fake { flag: true, ..Fake::default() };
        let mut l = FakeLua::with(vec![Val::Light(0x4000_0915), s("Palin_Palin01")]);
        assert_eq!(set_chatter_set(&mut l, &mut c), Ret::Values(0));
        assert_eq!((c.records[&0x4000_0915], c.bumps, c.marked.clone()), (PALIN, 1, vec![0x4000_0915]));
        let mut c = Fake::default();
        assert_eq!(set_chatter_set(&mut FakeLua::with(vec![Val::Light(9), s("Palin_Palin01")]), &mut c), Ret::Values(0));
        assert_eq!((c.bumps, c.marked.len()), (1, 0), "the flag off: no bit");
    }

    #[test]
    fn a_full_userdata_is_read_as_its_payload_address_like_the_games_reader() {
        let mut c = Fake::default();
        assert_eq!(set_chatter_set(&mut FakeLua::with(vec![Val::Userdata(0x0050_0000), s("Palin_Palin01")]), &mut c), Ret::Values(0));
        assert_eq!(c.records[&0x0050_0018], PALIN);
    }

    #[test]
    fn a_missing_or_wrong_argument_returns_nil_and_touches_nothing() {
        for args in [
            vec![],
            vec![Val::Light(1)],
            vec![s("Palin_Palin01"), s("Palin_Palin01")],
            vec![Val::Num(1.0), s("Palin_Palin01")],
            vec![Val::Light(1), s("")],
            vec![Val::Light(1), Val::Nil],
        ] {
            let mut c = Fake::default();
            let mut l = FakeLua::with(args.clone());
            assert_eq!(set_chatter_set(&mut l, &mut c), Ret::Values(1), "{args:?}");
            assert_eq!(l.pushed, [Val::Nil], "{args:?}");
            assert!(c.records.is_empty() && c.bumps == 0, "{args:?}");
        }
    }
}
