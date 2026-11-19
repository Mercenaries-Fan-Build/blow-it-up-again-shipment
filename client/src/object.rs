//! What each `Object` function does, independent of how it is called.
//!
//! * `Object.GetInfiniteAmmo(object)` → boolean. PS3 v1.03 (`0x00544530`) reads the object with its object
//!   reader (`0x00505790`: a light userdata's value or a full userdata's payload; anything else fails) and
//!   pushes nil when that fails. Otherwise it pushes whether the `CheatInfiniteAmmo` component has a record
//!   for the object: its record lookup (`0x003218D0`) asks the container's index function (`0x003206C8`, with
//!   the template fallback the container's flags allow) for the slot and turns it into the record's address,
//!   0 when there is none. PC has the same two functions (v1.1 `0x00521010`, a thunk to the record lookup,
//!   and the index function `0x00649640`) and tests presence the same way inline (v1.1 `0x005858F3`, see
//!   [`crate::sites`]); the binding calls the index function and computes the record as that inline test
//!   does ([`record`]).

use mercs2_formats::mod_runtime::lua::{Lua, Ret};

use crate::sites::{CONTAINER_INDEX_TABLE, CONTAINER_PAGE_SHIFT, CONTAINER_PAGE_SIZE, CONTAINER_PAGES, CONTAINER_STRIDE};

/// The `CheatInfiniteAmmo` component, as the engine reaches it.
pub trait AmmoRecords {
    /// The record address for `guid`, 0 when there is none.
    fn record(&self, guid: u32) -> u32;
}

/// The container's memory, as the presence test reads it.
pub trait ContainerMemory {
    fn u32_at(&self, va: u32) -> u32;
    fn i16_at(&self, va: u32) -> i16;
    fn u8_at(&self, va: u32) -> u8;
}

/// The record address of `slot` in `container` (the index function's answer), as the engine computes it
/// (v1.1 `0x00585905`–`0x00585932`): `k = index_table[slot]`; `pages[k >> shift] + (k & (page_size − 1)) *
/// stride`. A negative slot has none.
pub fn record<M: ContainerMemory>(m: &M, container: u32, slot: i32) -> u32 {
    if slot < 0 {
        return 0;
    }
    let k = m.u32_at(m.u32_at(container + CONTAINER_INDEX_TABLE).wrapping_add((slot as u32).wrapping_mul(4)));
    let shift = u32::from(m.u8_at(container + CONTAINER_PAGE_SHIFT)) & 0x1f;
    let page = ((k as i32) >> shift) as u32;
    let offset = (m.u32_at(container + CONTAINER_PAGE_SIZE).wrapping_sub(1) & k).wrapping_mul(i32::from(m.i16_at(container + CONTAINER_STRIDE)) as u32);
    offset.wrapping_add(m.u32_at(m.u32_at(container + CONTAINER_PAGES).wrapping_add(page.wrapping_mul(4))))
}

/// `Object.GetInfiniteAmmo(object)`.
pub fn get_infinite_ammo<L: Lua, A: AmmoRecords>(s: &mut L, a: &A) -> Ret {
    match s.arg_object(0) {
        Some(guid) => s.push_bool(a.record(guid) != 0),
        None => s.push_nil(),
    }
    Ret::Values(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mercs2_formats::mod_runtime::lua::fake::{FakeLua, Val, s};
    use std::collections::BTreeMap;

    struct Records(BTreeMap<u32, u32>);

    impl AmmoRecords for Records {
        fn record(&self, guid: u32) -> u32 {
            self.0.get(&guid).copied().unwrap_or(0)
        }
    }

    #[test]
    fn an_object_with_a_record_is_true_and_one_without_is_false() {
        let a = Records(BTreeMap::from([(0x4000_0915, 0x2000_0010)]));
        let mut l = FakeLua::with(vec![Val::Light(0x4000_0915)]);
        assert_eq!(get_infinite_ammo(&mut l, &a), Ret::Values(1));
        assert_eq!(l.pushed, [Val::Bool(true)]);
        let mut l = FakeLua::with(vec![Val::Light(0x4000_0916)]);
        assert_eq!(get_infinite_ammo(&mut l, &a), Ret::Values(1));
        assert_eq!(l.pushed, [Val::Bool(false)]);
    }

    #[test]
    fn a_full_userdata_is_read_as_its_payload_address() {
        let a = Records(BTreeMap::from([(0x0050_0018, 1)]));
        let mut l = FakeLua::with(vec![Val::Userdata(0x0050_0000)]);
        assert_eq!(get_infinite_ammo(&mut l, &a), Ret::Values(1));
        assert_eq!(l.pushed, [Val::Bool(true)]);
    }

    #[test]
    fn anything_but_an_object_pushes_nil() {
        let a = Records(BTreeMap::from([(0, 1), (1, 1)]));
        for args in [vec![], vec![Val::Nil], vec![Val::Num(1.0)], vec![s("1")], vec![Val::Bool(true)], vec![Val::Table]] {
            let mut l = FakeLua::with(args.clone());
            assert_eq!(get_infinite_ammo(&mut l, &a), Ret::Values(1), "{args:?}");
            assert_eq!(l.pushed, [Val::Nil], "{args:?}");
        }
    }

    /// A container in a flat little-endian memory.
    struct Mem(BTreeMap<u32, u8>);

    impl Mem {
        fn put(&mut self, va: u32, bytes: &[u8]) {
            for (i, b) in bytes.iter().enumerate() {
                self.0.insert(va + i as u32, *b);
            }
        }
    }

    impl ContainerMemory for Mem {
        fn u32_at(&self, va: u32) -> u32 {
            u32::from_le_bytes([0, 1, 2, 3].map(|i| self.0[&(va + i)]))
        }
        fn i16_at(&self, va: u32) -> i16 {
            i16::from_le_bytes([0, 1].map(|i| self.0[&(va + i)]))
        }
        fn u8_at(&self, va: u32) -> u8 {
            self.0[&va]
        }
    }

    #[test]
    fn the_record_is_the_page_plus_the_slot_within_it_times_the_stride() {
        // CheatInfiniteAmmo's own shape (v1.1 0x00DF8B10): page size 0x80, stride 1, shift 7.
        const C: u32 = 0x00df_8b10;
        let mut m = Mem(BTreeMap::new());
        m.put(C + CONTAINER_INDEX_TABLE, &0x3000_0000u32.to_le_bytes());
        m.put(C + CONTAINER_PAGE_SIZE, &0x80u32.to_le_bytes());
        m.put(C + CONTAINER_STRIDE, &1i16.to_le_bytes());
        m.put(C + CONTAINER_PAGE_SHIFT, &[7]);
        m.put(C + CONTAINER_PAGES, &0x3100_0000u32.to_le_bytes());
        m.put(0x3000_0000 + 5 * 4, &0x83u32.to_le_bytes()); // slot 5 -> record 0x83: page 1, entry 3
        m.put(0x3100_0004, &0x2200_0000u32.to_le_bytes());
        assert_eq!(record(&m, C, 5), 0x2200_0003);
        assert_eq!(record(&m, C, -1), 0, "no slot, no record");
    }
}
