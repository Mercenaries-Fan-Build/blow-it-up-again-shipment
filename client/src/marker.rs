//! `Gui._MarkerAddDiscDLC(guid, radius = 1, r = 255, g = 255, b = 255, f6 = 0)` → marker id or nil, and the
//! `Marker.AddDiscDLC` alias PS3 v1.03's embedded Gui chunk adds for it.
//!
//! PS3 v1.03's binding (`0x00523ED0`) is `Gui._MarkerAddDisc` (`0x005241C0`) instruction for instruction but
//! for the creator it calls: `0x005D4058` for the disc creator `0x005D42A0`. That creator writes the marker's
//! flag word with bits 62 and 61 set and kind 2 in bits 59..58 (the disc's: both clear, kind 1), and gives the
//! marker's object a hibernation distance of 400 for 100. The marker update (`0x005D4758`) draws kind 2 as a
//! disc, plus three writes to the spawned entity (every view's fade dirty, fade start 400 m, fade length
//! 100 m) and a model Y scale of 0.25 for 1/radius, and treats kind 2 as a disc at its two disc tests.
//!
//! PC's flag byte mirrors PS3's word field by field (PC bit 0 is PS3 bit 63, bits 5..4 the kind): bit 1 is
//! "removed when the pulse halts", bit 2 "pulsing". Its kinds are those of PS3 v1.00: 0 icon (`_MarkerAdd`),
//! 1 disc, 2 tripwire; 3 is set by no PC creator and read only by the update's kind switch. So on PC the
//! DLC disc is kind 3, and every existing marker keeps its meaning. The ASI runs PC's own `_MarkerAddDisc`
//! with a flag set that the creator's hooks read, and the update's hooks ([`crate::sites::MarkerSites`]) draw
//! kind 3 as above.

/// The bits the disc creator's flag byte keeps from the stack (bit 7, which no creator writes).
pub const KEEP: u8 = 0x80;
/// The DLC disc's bits: kind 3 (`0x30`), pulsing (`0x04`), removed when the pulse halts (`0x02`).
pub const DLC_BITS: u8 = 0x36;
/// The kind field.
pub const KIND_SHIFT: u8 = 4;
pub const KIND_MASK: u8 = 0x30;
pub const KIND_DISC: u8 = 1;
pub const KIND_TRIPWIRE: u8 = 2;
pub const KIND_DLC_DISC: u8 = 3;

/// The hibernation distance the creator gives the marker's object.
pub const HIBERNATION_DISC: u16 = 100;
pub const HIBERNATION_DLC: u16 = 400;

/// What the update writes to the spawned entity for a DLC disc: PC offsets of PS3's `+0xA`, `+0x1A0` and
/// `+0x1A4` (the base entity constructor `0x0046E470` zeroes `+0x16` and sets `+0x1B0 = 50.0`,
/// `+0x1B4 = 10.0` where PS3's `0x00240C20` does `+0xA`, `+0x1A0`, `+0x1A4`).
pub const ENTITY_FADE_DIRTY: u32 = 0x16;
pub const FADE_DIRTY_ALL_VIEWS: u16 = 0xf;
pub const ENTITY_FADE_START: u32 = 0x1b0;
pub const ENTITY_FADE_LENGTH: u32 = 0x1b4;
pub const FADE_START: f32 = 400.0;
pub const FADE_LENGTH: f32 = 100.0;
/// The model's Y scale for a DLC disc, where a disc's is 1/radius.
pub const DLC_Y_SCALE: f32 = 0.25;

/// The flag byte the DLC creator writes, from the one the disc creator wrote.
pub fn dlc_flag(disc: u8) -> u8 {
    (disc & KEEP) | DLC_BITS
}

pub fn kind(flag: u8) -> u8 {
    (flag & KIND_MASK) >> KIND_SHIFT
}

/// The update's two disc tests after the hook: a disc or a DLC disc.
pub fn drawn_as_disc(flag: u8) -> bool {
    let k = flag & KIND_MASK;
    k == KIND_DISC << KIND_SHIFT || k == KIND_DLC_DISC << KIND_SHIFT
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The disc creator's byte: `(AL & 0x80) | 0x10` (v1.1 `0x00619F72` `or al,0x10` after clearing bits 0..6).
    fn disc(stack: u8) -> u8 {
        (stack & 0x80) | 0x10
    }

    /// The tripwire creator's (v1.1 `0x0061A0B0`): bit 0 set, kind 2.
    fn tripwire(stack: u8) -> u8 {
        (stack & 0x80) | 0x21
    }

    #[test]
    fn the_dlc_disc_is_kind_3_pulsing_and_removed_when_its_pulse_halts() {
        for stack in [0x00, 0x80, 0xff, 0x7f] {
            let f = dlc_flag(disc(stack));
            assert_eq!(kind(f), KIND_DLC_DISC);
            assert_eq!((f & 0x04 != 0, f & 0x02 != 0, f & 0x01, f & 0x08, f & 0x40), (true, true, 0, 0, 0));
            assert_eq!(f & KEEP, stack & KEEP, "bit 7 is the disc creator's");
        }
        assert_eq!(kind(disc(0)), KIND_DISC);
    }

    #[test]
    fn discs_and_dlc_discs_are_drawn_as_discs_and_nothing_else_is() {
        for stack in [0x00, 0x80] {
            assert!(drawn_as_disc(disc(stack)));
            assert!(drawn_as_disc(dlc_flag(disc(stack))));
            assert!(!drawn_as_disc(tripwire(stack)), "kind 2 stays the tripwire");
            assert!(!drawn_as_disc(stack & 0xcf), "kind 0, the icon marker");
        }
        assert_eq!(kind(tripwire(0)), KIND_TRIPWIRE);
    }

    #[test]
    fn the_fade_values_are_ps3s_constants() {
        assert_eq!((FADE_START.to_bits(), FADE_LENGTH.to_bits(), DLC_Y_SCALE.to_bits()), (0x43c8_0000, 0x42c8_0000, 0x3e80_0000));
    }
}
