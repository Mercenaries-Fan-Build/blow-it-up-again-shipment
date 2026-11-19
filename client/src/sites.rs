//! The PC code the bindings call or hook, found in the game image by byte signature, so one ASI serves every
//! PC build: a site resolves only where its signature matches exactly once in `.text`, and the operands read
//! out of it are checked (the constants it adds are 35.0 and 40.0, the container names itself
//! `CheatInfiniteAmmo`). Every signature, offset and VA below was read from the disassembly of retail v1.1
//! (`pc-v11`, the dump `dump-v11-securom-unpacked`); `tests/pc_sites.rs` checks each resolves, at the
//! addresses read with Ghidra, in that build and in the three v1.0 reference builds.
//!
//! Map correction (PS3 v1.03 `Gui.SetMapCorrectionOffset`): PC hard-codes `+35`/`+40` (the f32 constants
//! at v1.1 `0x00BEAA40`/`0x00B9B724`) and no scale where PS3 v1.03 reads `x`, `z` and `scale`. Each hook
//! replaces the instructions of one site with the same instructions reading the ASI's values, and returns
//! to the instruction after them:
//!
//! | site | v1.1 VA | PC instructions replaced | PS3 v1.03 |
//! |---|---|---|---|
//! | [`MapSites::minimap`] | `0x0062037D` in `FUN_006201A0` | `addss xmm2,[35]; addss xmm3,[40]` | `0x5DF650`: `(x + fx) * s`, `(z + fz) * s` |
//! | [`MapSites::gps_load`] | `0x00620AE7`, `0x00620BD3` in `FUN_00620AA0` | `movss xmm0,[35]; movss xmm1,[40]` (and reversed) | `0x5DE020`: `x`, `z` held across the loop |
//! | [`MapSites::gps_scale`] | `0x00620B1B`, `0x00620B2C`, `0x00620B3E`, `0x00620B5F`, `0x00620C37`, `0x00620C54` | `addss a,b; cvtps2pd c,d` | `(x + p.x) * s`, `(z + p.z) * s` |
//! | [`MapSites::radar`] | `0x006213BA` in `FUN_00621380` | `divss xmm2,[esi+0x19C]` | `0x5DD740`: `(radius / range) * s` |
//! | [`MapSites::coop`] | `0x006216E9`, `0x00621705` in `FUN_00621620` | `addss xmm0,[35]`, `addss xmm1,[40]` | `0x5DD540`: `((p0 + p1) * 0.5 + x) * s` |
//! | [`MapSites::pda`] | `0x006A0EF6` in `FUN_006A0DD0` | `cmp byte [flag],0` | `0x1CF868`: stored `Q / s − x`, `Q / s − z` |
//!
//! The PDA click handler stores the movie's map position `Q` raw at `this+0x39C`/`+0x3A0` (`EDI` = this)
//! and sends it in the `SetGPSDest` event; PS3 v1.03 sends the same raw `Q` in the event and stores
//! `Q / s − x`. The hook runs after the event is sent, before the stored values are read for the network
//! packet, so both keep PS3's values.
//!
//! The reset (PS3 v1.03 PgSysGUI vtable slot 7, `0x3D4908`, run when the GUI system is released): on PC that
//! slot is `vtable+0x18`, the shared `ret` stub, in `.rdata`. Every place the engine calls `vtable+0x18` on
//! a deactivating system (the system state update and its two copies inlined in reset-all-systems; the
//! update's state store, which follows each call, occurs nowhere else in `.text`) is hooked instead, and
//! resets the values when the system is the GUI's (its `vtable+0x0C` is the GUI init
//! [`ResetSites::gui_init`]):
//!
//! | site | v1.1 VA | what it is |
//! |---|---|---|
//! | [`ResetSites::update`] | `0x004EB018` in `FUN_004EAFE0` | the system state update's `mov eax,[edx+0x18]; call eax` |
//! | [`ResetSites::inline`] | `0x004C07AE`, `0x004C08C8` in `FUN_004C06E0` | reset-all-systems' inlined updates (release loop, acquire loop): `test bl,bl; mov edx,[esi]; mov ecx,esi; jz deactivate` |
//! | [`ResetSites::gui_init`] | `0x004F6C30` | PgSysGUI's init, `vtable+0x0C`: `mov al,1; mov [ecx+0x10],al; mov [ecx+0x11],al; mov [g],6; ret` |
//!
//! `Object.GetInfiniteAmmo` (PS3 v1.03 `0x544530`): [`AmmoSites`] from the engine's own presence test of the
//! `CheatInfiniteAmmo` component in v1.1 `FUN_00585840` at `0x005858F3`: `push guid; mov ecx,container;
//! call index` (`__thiscall`, `ret 4`; the slot, or < 0 when absent, after the container's template
//! fallback), then the record address from the container's index table, page table, page mask, stride and
//! shift.
//!
//! `Gui._MarkerAddDiscDLC` (PS3 v1.03 `0x523ED0`) is PC's `Gui._MarkerAddDisc` (found through its `luaL_Reg`
//! row) run with the DLC disc's creator and update. PS3 v1.03's DLC creator `0x5D4058` differs from the disc
//! creator `0x5D42A0` in the marker's flag word (pulsing, removed when the pulse halts, kind 2) and in the
//! hibernation distance (400 for 100); its update `0x5D4758` gives kind 2 the disc's path plus the spawned
//! entity's fade (400 m start, 100 m length, every view's fade dirty) and a Y scale of 0.25 for 1/radius.
//! PC's kind 2 is the tripwire (as on PS3 v1.00) and kind 3 is unused, so the DLC disc is PC kind 3:
//!
//! | site | v1.1 VA | PC instructions replaced |
//! |---|---|---|
//! | [`MarkerSites::creator`] | `0x00619E90` | the disc creator (`_MarkerAddDisc`'s only creator call) |
//! | [`MarkerSites::flag`] | `0x00619F9A` | `movss [esp+0xa8],xmm0; mov [esp+0x58],al` (the flag byte, `(AL & 0x80) \| 0x10`) |
//! | [`MarkerSites::hibernation`] | `0x00619FE3` | `mov word [esp+0x14],100` |
//! | [`MarkerSites::switch`] | `0x0061AA52` in the update `FUN_0061A990` | `mov al,[ebp]; shr al,4; and al,3; cmp al,1; jne other` |
//! | [`MarkerSites::scale`] | `0x0061AAE4` | `movss xmm0,[1.0]; divss xmm0,[ebp+4]` |
//! | [`MarkerSites::disc_checks`] | `0x0061AC87`, `0x0061AD11` | `and bl,0x30; cmp bl,0x10; je` (and `dl`) |

use mercs2_formats::exe_image::Image;
use mercs2_formats::lua_sites::Pattern;

/// The f32 constants PC adds where PS3 v1.03 adds the map correction's `x` and `z`.
pub const PC_X: f32 = 35.0;
pub const PC_Z: f32 = 40.0;
/// The container the binding reads, and the string its name accessor returns.
pub const AMMO_COMPONENT: &str = "CheatInfiniteAmmo";

/// Where an ECS container's fields are, as the engine's presence test reads them.
pub const CONTAINER_INDEX_TABLE: u32 = 0x48;
pub const CONTAINER_PAGE_SHIFT: u32 = 0x26;
pub const CONTAINER_PAGE_SIZE: u32 = 0x20;
pub const CONTAINER_STRIDE: u32 = 0x24;
pub const CONTAINER_PAGES: u32 = 0x70;
/// The name accessor's slot in a container's vtable: `mov eax, <name>; ret`.
pub const CONTAINER_NAME_SLOT: u32 = 0x34;

/// One signature: where its site is in the match, and how many bytes of instructions the hook replaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    Minimap,
    GpsLoadFirst,
    GpsLoadAgain,
    GpsScale1,
    GpsScale2,
    GpsScale3,
    GpsScale4,
    GpsScale5,
    GpsScale6,
    Radar,
    CoopX,
    CoopZ,
    PdaStores,
    PdaFlag,
    DeactivateUpdate,
    DeactivateRelease,
    DeactivateAcquire,
    UpdateTail,
    GuiInit,
    AmmoPresence,
    MarkerCreator,
    MarkerFlag,
    MarkerHibernation,
    MarkerSwitch,
    MarkerScale,
    MarkerCheck1,
    MarkerCheck2,
}

impl Site {
    pub const ALL: [Site; 27] = [
        Site::Minimap,
        Site::GpsLoadFirst,
        Site::GpsLoadAgain,
        Site::GpsScale1,
        Site::GpsScale2,
        Site::GpsScale3,
        Site::GpsScale4,
        Site::GpsScale5,
        Site::GpsScale6,
        Site::Radar,
        Site::CoopX,
        Site::CoopZ,
        Site::PdaStores,
        Site::PdaFlag,
        Site::DeactivateUpdate,
        Site::DeactivateRelease,
        Site::DeactivateAcquire,
        Site::UpdateTail,
        Site::GuiInit,
        Site::AmmoPresence,
        Site::MarkerCreator,
        Site::MarkerFlag,
        Site::MarkerHibernation,
        Site::MarkerSwitch,
        Site::MarkerScale,
        Site::MarkerCheck1,
        Site::MarkerCheck2,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Site::Minimap => "Minimap",
            Site::GpsLoadFirst => "GpsLoadFirst",
            Site::GpsLoadAgain => "GpsLoadAgain",
            Site::GpsScale1 => "GpsScale1",
            Site::GpsScale2 => "GpsScale2",
            Site::GpsScale3 => "GpsScale3",
            Site::GpsScale4 => "GpsScale4",
            Site::GpsScale5 => "GpsScale5",
            Site::GpsScale6 => "GpsScale6",
            Site::Radar => "Radar",
            Site::CoopX => "CoopX",
            Site::CoopZ => "CoopZ",
            Site::PdaStores => "PdaStores",
            Site::PdaFlag => "PdaFlag",
            Site::DeactivateUpdate => "DeactivateUpdate",
            Site::DeactivateRelease => "DeactivateRelease",
            Site::DeactivateAcquire => "DeactivateAcquire",
            Site::UpdateTail => "UpdateTail",
            Site::GuiInit => "GuiInit",
            Site::AmmoPresence => "AmmoPresence",
            Site::MarkerCreator => "MarkerCreator",
            Site::MarkerFlag => "MarkerFlag",
            Site::MarkerHibernation => "MarkerHibernation",
            Site::MarkerSwitch => "MarkerSwitch",
            Site::MarkerScale => "MarkerScale",
            Site::MarkerCheck1 => "MarkerCheck1",
            Site::MarkerCheck2 => "MarkerCheck2",
        }
    }

    /// The signature (space-separated hex, `??` wildcards on absolute operands and branch displacements), the
    /// site's offset in it, and the length of the instructions a hook there replaces (0: not hooked).
    pub fn def(self) -> (&'static str, usize, usize) {
        match self {
            // cmp [ebx+0x100c],0; movss xmm2,[ebx+0x190]; movss xmm3,[ebx+0x198]; >addss xmm2,[35];
            // addss xmm3,[40]; jg
            Site::Minimap => (
                "83 bb 0c 10 00 00 00 f3 0f 10 93 90 01 00 00 f3 0f 10 9b 98 01 00 00 \
                 f3 0f 58 15 ?? ?? ?? ?? f3 0f 58 1d ?? ?? ?? ?? 0f 8f",
                23,
                16,
            ),
            // cmp ebx,0xff; >movss xmm0,[35]; movss xmm1,[40]; jge
            Site::GpsLoadFirst => ("81 fb ff 00 00 00 f3 0f 10 05 ?? ?? ?? ?? f3 0f 10 0d ?? ?? ?? ?? 0f 8d", 6, 16),
            // call [LeaveCriticalSection]; >movss xmm1,[40]; movss xmm0,[35]; mov eax,3; add ebx,1
            Site::GpsLoadAgain => {
                ("ff 15 ?? ?? ?? ?? f3 0f 10 0d ?? ?? ?? ?? f3 0f 10 05 ?? ?? ?? ?? b8 03 00 00 00 83 c3 01", 6, 16)
            }
            // movss xmm2,[edi-4]; cmp ebx,[ebp+0xff0]; >addss xmm2,xmm0; cvtps2pd xmm2,xmm2; movsd [esp+0x28],xmm2
            Site::GpsScale1 => ("f3 0f 10 57 fc 3b 9d f0 0f 00 00 f3 0f 58 d0 0f 5a d2 f2 0f 11 54 24 28", 11, 7),
            // movss xmm2,[edi]; >addss xmm2,xmm1; cvtps2pd xmm2,xmm2; movsd [esp+0x38],xmm2
            Site::GpsScale2 => ("f3 0f 10 17 f3 0f 58 d1 0f 5a d2 f2 0f 11 54 24 38", 4, 7),
            // movss xmm2,[edi+4]; >addss xmm2,xmm0; cvtps2pd xmm2,xmm2; movsd [esp+0x48],xmm2
            Site::GpsScale3 => ("f3 0f 10 57 04 f3 0f 58 d0 0f 5a d2 f2 0f 11 54 24 48", 5, 7),
            // cmp [esi+0x1e0],0; >addss xmm2,xmm1; cvtps2pd xmm2,xmm2; mov [esp+0x10],2
            Site::GpsScale4 => ("83 be e0 01 00 00 00 f3 0f 58 d1 0f 5a d2 c7 44 24 10 02 00 00 00", 7, 7),
            // movss xmm2,[ebp+0x190]; >addss xmm2,xmm0; cvtps2pd xmm0,xmm2; movsd [esp+0x48],xmm0
            Site::GpsScale5 => ("f3 0f 10 95 90 01 00 00 f3 0f 58 d0 0f 5a c2 f2 0f 11 44 24 48", 8, 7),
            // lea ecx,[esp+0x18]; >addss xmm0,xmm1; cvtps2pd xmm0,xmm0; push ecx; mov edi,<name>
            Site::GpsScale6 => ("8d 4c 24 18 f3 0f 58 c1 0f 5a c0 51 bf", 4, 7),
            // movaps xmm2,xmm5; >divss xmm2,[esi+0x19c]; mulss xmm0,xmm2; mulss xmm1,xmm2
            Site::Radar => ("0f 28 d5 f3 0f 5e 96 9c 01 00 00 f3 0f 59 c2 f3 0f 59 ca", 3, 8),
            // mulss xmm0,xmm2; >addss xmm0,[35]; mulss xmm1,xmm2
            Site::CoopX => ("f3 0f 59 c2 f3 0f 58 05 ?? ?? ?? ?? f3 0f 59 ca", 4, 8),
            // mulss xmm2,[k]; >addss xmm1,[40]; mov al,1
            Site::CoopZ => ("f3 0f 59 15 ?? ?? ?? ?? f3 0f 58 0d ?? ?? ?? ?? b0 01", 8, 8),
            // cvtsi2ss xmm0,[esp+0x18]; movss [edi+0x39c],xmm0; cvtsi2ss xmm0,[esp+0x1c]; movss [edi+0x3a0],xmm0
            Site::PdaStores => (
                "f3 0f 2a 44 24 18 f3 0f 11 87 9c 03 00 00 f3 0f 2a 44 24 1c f3 0f 11 87 a0 03 00 00",
                0,
                0,
            ),
            // call <free>; >cmp byte [flag],0; je; cmp byte [esp+0x54],0; jne
            Site::PdaFlag => ("e8 ?? ?? ?? ?? 80 3d ?? ?? ?? ?? 00 74 ?? 80 7c 24 54 00 75", 5, 7),
            // test bl,bl; mov edx,[esi]; mov ecx,esi; je; mov eax,[edx+0x14]; call eax; ...; pop ebx; ret;
            // >mov eax,[edx+0x18]; call eax
            Site::DeactivateUpdate => (
                "84 db 8b 16 8b ce 74 12 8b 42 14 ff d0 02 db 32 5e 04 80 e3 02 30 5e 04 5b c3 8b 42 18 ff d0",
                26,
                5,
            ),
            // The update inlined in reset-all-systems' release loop, then its loop step (`sub edi,1`):
            // >test bl,bl; mov edx,[esi]; mov ecx,esi; je +5; mov eax,[edx+0x14]; jmp +3; mov eax,[edx+0x18];
            // call eax; add bl,bl; xor bl,[esi+4]; and bl,2; xor [esi+4],bl; mov ebx,[esp+0x10]; sub edi,1
            Site::DeactivateRelease => (
                "84 db 8b 16 8b ce 74 05 8b 42 14 eb 03 8b 42 18 ff d0 02 db 32 5e 04 80 e3 02 30 5e 04 8b 5c 24 10 83 ef 01",
                0,
                8,
            ),
            // The same, inlined in its acquire loop (`add ebp,1`).
            Site::DeactivateAcquire => (
                "84 db 8b 16 8b ce 74 05 8b 42 14 eb 03 8b 42 18 ff d0 02 db 32 5e 04 80 e3 02 30 5e 04 8b 5c 24 10 83 c5 01",
                0,
                8,
            ),
            // The update's state store after either vtable call: add bl,bl; xor bl,[esi+4]; and bl,2;
            // xor [esi+4],bl. Matched by every copy of the update, so it counts them.
            Site::UpdateTail => ("02 db 32 5e 04 80 e3 02 30 5e 04", 0, 0),
            // mov al,1; mov [ecx+0x10],al; mov [ecx+0x11],al; mov dword [g],6; ret
            Site::GuiInit => ("b0 01 88 41 10 88 41 11 c7 05 ?? ?? ?? ?? 06 00 00 00 c3", 0, 0),
            // mov eax,[esi+0x18]; push eax; mov ecx,<C>; call <index>; test eax,eax; jl; mov edx,[C+0x48];
            // mov eax,[edx+eax*4]; mov cl,[C+0x26]; mov edx,eax; sar edx,cl; mov ecx,[C+0x20]; add ecx,-1;
            // and ecx,eax; movsx eax,word [C+0x24]; imul ecx,eax; mov eax,[C+0x70]; add ecx,[eax+edx*4];
            // jne; xor eax,eax; jmp; mov eax,1
            Site::AmmoPresence => (
                "8b 46 18 50 b9 ?? ?? ?? ?? e8 ?? ?? ?? ?? 85 c0 7c 32 8b 15 ?? ?? ?? ?? 8b 04 82 8a 0d ?? ?? ?? ?? \
                 8b d0 d3 fa 8b 0d ?? ?? ?? ?? 83 c1 ff 23 c8 0f bf 05 ?? ?? ?? ?? 0f af c8 a1 ?? ?? ?? ?? \
                 03 0c 90 75 04 33 c0 eb 05 b8 01 00 00 00",
                0,
                0,
            ),
            // push ebp; mov ebp,esp; and esp,-16; sub esp,0xb4; mov eax,[g]; push ebx; mov ebx,[eax+0x10];
            // push esi; push edi; lea ecx,[ebx+1]; lea esi,[esp+0x28]; mov edi,edx; mov [eax+0x10],ecx; call
            Site::MarkerCreator => (
                "55 8b ec 83 e4 f0 81 ec b4 00 00 00 a1 ?? ?? ?? ?? 53 8b 58 10 56 57 8d 4b 01 8d 74 24 28 8b fa 89 48 10 e8",
                0,
                0,
            ),
            // >movss [esp+0xa8],xmm0; mov [esp+0x58],al; call <pool add>
            Site::MarkerFlag => ("f3 0f 11 84 24 a8 00 00 00 88 44 24 58 e8", 0, 13),
            // and byte [esp+0x15],0xfe; push ebx; lea eax,[esp+0x14]; >mov word [esp+0x14],100;
            // mov byte [esp+0x16],0xa0; mov byte [esp+0x17],0x3c; mov byte [esp+0x18],0x14; call <hibernation>
            Site::MarkerHibernation => (
                "80 64 24 15 fe 53 8d 44 24 14 66 c7 44 24 14 64 00 c6 44 24 16 a0 c6 44 24 17 3c c6 44 24 18 14 e8",
                10,
                7,
            ),
            // >mov al,[ebp]; shr al,4; and al,3; cmp al,1; jne <other kinds>; fld [ebp+4]; add [ebx+0x30],-1;
            // fstp [ebx+0x80]
            Site::MarkerSwitch => (
                "8a 45 00 c0 e8 04 24 03 3c 01 0f 85 ?? ?? ?? ?? d9 45 04 83 43 30 ff d9 9b 80 00 00 00",
                0,
                16,
            ),
            // movq [esp+0x20],xmm0; >movss xmm0,[1.0]; divss xmm0,[ebp+4]; movss xmm1,[esp+0x20];
            // mov [esp+0x28],edx; movss [esp+0x3c],xmm0
            Site::MarkerScale => (
                "66 0f d6 44 24 20 f3 0f 10 05 ?? ?? ?? ?? f3 0f 5e 45 04 f3 0f 10 4c 24 20 89 54 24 28 f3 0f 11 44 24 3c",
                6,
                13,
            ),
            // mov bl,[ebp]; push eax; lea eax,[esp+0x3c]; call; >and bl,0x30; cmp bl,0x10; je
            Site::MarkerCheck1 => ("8a 5d 00 50 8d 44 24 3c e8 ?? ?? ?? ?? 80 e3 30 80 fb 10 74 ??", 13, 8),
            // mov dl,[ebp]; >and dl,0x30; cmp dl,0x10; je
            Site::MarkerCheck2 => ("8a 55 00 80 e2 30 80 fa 10 74 ??", 3, 8),
        }
    }
}

/// Where the PDA click handler's flag test is, from its stores of the clicked position (both `+0x10B` in
/// v1.1 `FUN_006A0DD0`: the stores at `0x006A0DEB`, the test at `0x006A0EF6`).
const PDA_STORES_TO_FLAG: u32 = 0x10b;
/// The GPS line's sites, in address order, all within this many bytes of the first.
const GPS_SPAN: u32 = 0x200;
/// Operand offsets in the signatures.
const MINIMAP_X_AT: usize = 27;
const MINIMAP_Z_AT: usize = 35;
const GPS_FIRST_X_AT: usize = 10;
const GPS_FIRST_Z_AT: usize = 18;
const GPS_AGAIN_Z_AT: usize = 10;
const GPS_AGAIN_X_AT: usize = 18;
const COOP_X_AT: usize = 8;
const COOP_Z_AT: usize = 12;
const PDA_FLAG_AT: usize = 7;
/// An inlined update's deactivate path (`mov eax,[edx+0x18]`) starts this far into its hooked window.
const INLINE_DEACTIVATE_AT: u32 = 13;
/// `mov eax,[edx+0x18]; call eax` (or `+0x14`): the state store follows it.
const UPDATE_CALL_LEN: u32 = 5;
/// In the update function, the activate call's state store (`0x004EB00B`) is this far before the deactivate
/// call (`0x004EB018`).
const UPDATE_DEACTIVATE_TO_ACTIVATE_TAIL: u32 = 0xd;
const AMMO_CONTAINER_AT: usize = 5;
const AMMO_INDEX_CALL_AT: usize = 9;
const AMMO_FIELDS: [(usize, u32); 5] = [
    (20, CONTAINER_INDEX_TABLE),
    (29, CONTAINER_PAGE_SHIFT),
    (39, CONTAINER_PAGE_SIZE),
    (51, CONTAINER_STRIDE),
    (59, CONTAINER_PAGES),
];

/// The Lua function the DLC binding runs PC's disc creation through, by its `luaL_Reg` name.
pub const ADD_DISC: &str = "_MarkerAddDisc";
/// `_MarkerAddDisc`'s length (v1.1 `0x005B38C0`, 0x203 bytes); its one call of the creator is inside.
const ADD_DISC_LEN: u32 = 0x203;
/// The creator's length (v1.1 `0x00619E90`, 0x212 bytes); the flag and hibernation sites are inside.
const CREATOR_LEN: u32 = 0x212;
/// The marker update's sites (`FUN_0061A990`: the switch `0x61AA52` to the second check `0x61AD11`) lie
/// within this many bytes of the switch.
const MARKER_UPDATE_SPAN: u32 = 0x300;
const MARKER_SWITCH_OTHER_AT: usize = 12;
const MARKER_SCALE_ONE_AT: usize = 10;
const MARKER_CHECK1_REL8_AT: usize = 20;
const MARKER_CHECK2_REL8_AT: usize = 10;

/// A hook: the first instruction it replaces, and where execution resumes after the replaced ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hook {
    pub va: u32,
    pub resume: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapSites {
    pub minimap: Hook,
    /// The two loads of the offsets into `xmm0`/`xmm1`: `[x first, z first]`, `[z first, x first]`.
    pub gps_load: [Hook; 2],
    /// The six adds of an offset, each followed by its `cvtps2pd`, in address order.
    pub gps_scale: [Hook; 6],
    pub radar: Hook,
    /// `[x, z]`.
    pub coop: [Hook; 2],
    pub pda: Hook,
    /// The byte the PDA click handler tests where the hook sits (whether to send the network packet).
    pub pda_flag: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResetSites {
    pub update: Hook,
    /// The two copies inlined in reset-all-systems, `[release loop, acquire loop]`. Each resumes at the
    /// activate path (`resume`) or the deactivate path (`inline_deactivate`).
    pub inline: [Hook; 2],
    pub inline_deactivate: [u32; 2],
    pub gui_init: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmmoSites {
    /// The `CheatInfiniteAmmo` container.
    pub container: u32,
    /// `__thiscall (container, guid) -> slot`, `ret 4`; < 0 when the entity has no record.
    pub index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkerSites {
    /// PC's `Gui._MarkerAddDisc`, a `lua_CFunction`.
    pub add_disc: u32,
    /// The disc creator `_MarkerAddDisc` calls.
    pub creator: u32,
    pub flag: Hook,
    pub hibernation: Hook,
    /// Resumes at the disc path (`resume`) or the other kinds' path (`switch_other`, where the replaced `jne`
    /// went).
    pub switch: Hook,
    pub switch_other: u32,
    pub scale: Hook,
    /// The two "kind is disc" tests: each resumes after its `je` (`resume`) or where the `je` went.
    pub disc_checks: [Hook; 2],
    pub disc_check_taken: [u32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sites {
    pub map: MapSites,
    pub reset: ResetSites,
    pub ammo: AmmoSites,
    pub marker: MarkerSites,
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

struct Text<'a> {
    va: u32,
    bytes: &'a [u8],
}

impl Text<'_> {
    /// The VA of the one match of `site`'s signature.
    fn find(&self, site: Site) -> Result<u32, String> {
        let (pattern, _, _) = site.def();
        let p = Pattern::parse(pattern).map_err(|e| format!("{}: the built-in signature does not parse: {e:?}", site.name()))?;
        match p.find_all(self.bytes)[..] {
            [one] => Ok(self.va + one as u32),
            ref hits => Err(format!("the {} signature matched {} time(s) in .text, not once", site.name(), hits.len())),
        }
    }

    fn bytes_from(&self, va: u32) -> &[u8] {
        &self.bytes[(va - self.va) as usize..]
    }

    fn hook(&self, site: Site) -> Result<Hook, String> {
        let (_, at, len) = site.def();
        let m = self.find(site)?;
        Ok(Hook { va: m + at as u32, resume: m + (at + len) as u32 })
    }
}

/// The f32 at `va`, which must be `want`.
fn constant(img: &Image, site: Site, va: u32, want: f32) -> Result<(), String> {
    let got = img.u32_at(va).ok_or_else(|| format!("{}: its constant {va:#x} is not in the image", site.name()))?;
    if got != want.to_bits() {
        return Err(format!("{}: the constant at {va:#x} is {} ({got:#010x}), not {want}", site.name(), f32::from_bits(got)));
    }
    Ok(())
}

fn resolve_map(img: &Image, text: &Text) -> Result<MapSites, String> {
    let minimap = text.hook(Site::Minimap)?;
    let m = text.find(Site::Minimap)?;
    let b = text.bytes_from(m);
    constant(img, Site::Minimap, u32_at(b, MINIMAP_X_AT), PC_X)?;
    constant(img, Site::Minimap, u32_at(b, MINIMAP_Z_AT), PC_Z)?;

    let first = text.hook(Site::GpsLoadFirst)?;
    let b = text.bytes_from(text.find(Site::GpsLoadFirst)?);
    constant(img, Site::GpsLoadFirst, u32_at(b, GPS_FIRST_X_AT), PC_X)?;
    constant(img, Site::GpsLoadFirst, u32_at(b, GPS_FIRST_Z_AT), PC_Z)?;
    let again = text.hook(Site::GpsLoadAgain)?;
    let b = text.bytes_from(text.find(Site::GpsLoadAgain)?);
    constant(img, Site::GpsLoadAgain, u32_at(b, GPS_AGAIN_Z_AT), PC_Z)?;
    constant(img, Site::GpsLoadAgain, u32_at(b, GPS_AGAIN_X_AT), PC_X)?;
    let scale = [
        text.hook(Site::GpsScale1)?,
        text.hook(Site::GpsScale2)?,
        text.hook(Site::GpsScale3)?,
        text.hook(Site::GpsScale4)?,
        text.hook(Site::GpsScale5)?,
        text.hook(Site::GpsScale6)?,
    ];
    // One function: the first load, four adds in the loop, the reload after the loop's critical section, the
    // two adds after the loop.
    let order = [first.va, scale[0].va, scale[1].va, scale[2].va, scale[3].va, again.va, scale[4].va, scale[5].va];
    if !order.windows(2).all(|w| w[0] < w[1]) || order[7] - order[0] >= GPS_SPAN {
        return Err(format!("the GPS line's sites {order:#x?} are not one function's, in order, within {GPS_SPAN:#x} bytes"));
    }

    let radar = text.hook(Site::Radar)?;
    let coop_x = text.hook(Site::CoopX)?;
    constant(img, Site::CoopX, u32_at(text.bytes_from(text.find(Site::CoopX)?), COOP_X_AT), PC_X)?;
    let coop_z = text.hook(Site::CoopZ)?;
    constant(img, Site::CoopZ, u32_at(text.bytes_from(text.find(Site::CoopZ)?), COOP_Z_AT), PC_Z)?;
    if !(coop_x.va < coop_z.va && coop_z.va - coop_x.va < 0x40) {
        return Err(format!("the co-op midpoint's sites {:#x} and {:#x} are not one block's, x before z", coop_x.va, coop_z.va));
    }

    let stores = text.find(Site::PdaStores)?;
    let pda = text.hook(Site::PdaFlag)?;
    if pda.va.wrapping_sub(stores) != PDA_STORES_TO_FLAG {
        return Err(format!(
            "the PDA click handler's flag test {:#x} is not {PDA_STORES_TO_FLAG:#x} bytes after its stores {stores:#x}",
            pda.va
        ));
    }
    let pda_flag = u32_at(text.bytes_from(text.find(Site::PdaFlag)?), PDA_FLAG_AT);
    if img.section_of(pda_flag) != Some(".data") {
        return Err(format!("the PDA click handler's flag {pda_flag:#x} is not in .data"));
    }
    Ok(MapSites { minimap, gps_load: [first, again], gps_scale: scale, radar, coop: [coop_x, coop_z], pda, pda_flag })
}

fn resolve_reset(text: &Text) -> Result<ResetSites, String> {
    let update = text.hook(Site::DeactivateUpdate)?;
    let inline = [text.hook(Site::DeactivateRelease)?, text.hook(Site::DeactivateAcquire)?];
    let inline_deactivate = inline.map(|h| h.va + INLINE_DEACTIVATE_AT);
    // Every copy of the update is hooked: its state store follows the update's two calls and each inlined
    // copy's one, and nothing else.
    let tails = Pattern::parse(Site::UpdateTail.def().0).expect("built-in signature parses").find_all(text.bytes);
    let mut tails: Vec<u32> = tails.into_iter().map(|o| text.va + o as u32).collect();
    tails.sort();
    let mut want = vec![update.va - UPDATE_DEACTIVATE_TO_ACTIVATE_TAIL, update.va + UPDATE_CALL_LEN, inline_deactivate[0] + UPDATE_CALL_LEN, inline_deactivate[1] + UPDATE_CALL_LEN];
    want.sort();
    if tails != want {
        return Err(format!("the system state update's copies end at {tails:#x?}, not at the hooked copies' {want:#x?}"));
    }
    let gui_init = text.find(Site::GuiInit)?;
    Ok(ResetSites { update, inline, inline_deactivate, gui_init })
}

fn resolve_ammo(img: &Image, text: &Text) -> Result<AmmoSites, String> {
    let m = text.find(Site::AmmoPresence)?;
    let b = text.bytes_from(m);
    let container = u32_at(b, AMMO_CONTAINER_AT);
    let index = m.wrapping_add(AMMO_INDEX_CALL_AT as u32 + 5).wrapping_add(u32_at(b, AMMO_INDEX_CALL_AT + 1));
    for (at, field) in AMMO_FIELDS {
        let have = u32_at(b, at);
        if have != container.wrapping_add(field) {
            return Err(format!("the {AMMO_COMPONENT} presence test reads {have:#x}, not container {container:#x} + {field:#x}"));
        }
    }
    let in_text = |va: u32| img.section(".text").is_some_and(|t| t.contains(va));
    if !in_text(index) {
        return Err(format!("the {AMMO_COMPONENT} index function {index:#x} is not in .text"));
    }
    // The container's vtable is stored by its static constructor (`mov dword [container], <vtable>`, v1.1
    // `0x00A7C82E`), so before that runs `.data` holds no vtable; the name is read through every such store.
    let name_of = |vtable: u32| {
        img.u32_at(vtable + CONTAINER_NAME_SLOT)
            .filter(|&accessor| in_text(accessor))
            .and_then(|accessor| img.bytes_at(accessor))
            .filter(|b| b.len() >= 6 && b[0] == 0xb8 && b[5] == 0xc3)
            .and_then(|b| img.cstr_at(u32_at(b, 1)))
    };
    let mut store = vec![0xc7, 0x05];
    store.extend_from_slice(&container.to_le_bytes());
    let vtables: Vec<u32> = text.bytes.windows(10).filter(|w| w[..6] == store[..]).map(|w| u32_at(w, 6)).collect();
    let names: Vec<Option<String>> = vtables.iter().map(|&v| name_of(v).map(|n| String::from_utf8_lossy(n).into_owned())).collect();
    if names.iter().filter(|n| n.as_deref() == Some(AMMO_COMPONENT)).count() != 1 {
        return Err(format!(
            "the container {container:#x} the presence test reads is not named {AMMO_COMPONENT:?} by exactly one vtable stored into it through vtable+{CONTAINER_NAME_SLOT:#x} (stores {vtables:#x?} name {names:?})"
        ));
    }
    Ok(AmmoSites { container, index })
}

/// The function of the one `luaL_Reg` row `{name, func}` whose name is the C string `name`: every 4-byte
/// aligned dword of `.rdata` and `.data` that holds the address of a NUL-preceded `name\0` in `.rdata`,
/// followed by an address in `.text`.
fn lua_function(img: &Image, name: &str) -> Result<u32, String> {
    let rdata = img.section(".rdata").ok_or("the image has no .rdata section")?;
    let text = img.section(".text").ok_or("the image has no .text section")?;
    let mut want = name.as_bytes().to_vec();
    want.push(0);
    let strings: Vec<u32> = rdata
        .bytes
        .windows(want.len())
        .enumerate()
        .filter(|&(i, w)| w == want.as_slice() && (i == 0 || rdata.bytes[i - 1] == 0))
        .map(|(i, _)| rdata.va + i as u32)
        .collect();
    let mut rows = Vec::new();
    for section in [".rdata", ".data"] {
        let s = img.section(section).ok_or_else(|| format!("the image has no {section} section"))?;
        for i in (0..s.bytes.len().saturating_sub(7)).step_by(4) {
            let (n, f) = (u32_at(s.bytes, i), u32_at(s.bytes, i + 4));
            if strings.contains(&n) && text.contains(f) {
                rows.push(f);
            }
        }
    }
    match rows[..] {
        [f] => Ok(f),
        _ => Err(format!("{} luaL_Reg row(s) name {name:?} (strings at {strings:#x?}), not one", rows.len())),
    }
}

/// How many `call rel32` (`E8`) starting in `[from, from + len)` target `to`.
fn calls(text: &Text, from: u32, len: u32, to: u32) -> usize {
    let b = text.bytes_from(from);
    (0..len as usize).filter(|&i| b[i] == 0xe8 && from.wrapping_add(i as u32 + 5).wrapping_add(u32_at(b, i + 1)) == to).count()
}

fn resolve_marker(img: &Image, text: &Text) -> Result<MarkerSites, String> {
    let add_disc = lua_function(img, ADD_DISC)?;
    let creator = text.find(Site::MarkerCreator)?;
    let n = calls(text, add_disc, ADD_DISC_LEN, creator);
    if n != 1 {
        return Err(format!("{ADD_DISC} ({add_disc:#x}) calls the disc creator {creator:#x} {n} time(s), not once"));
    }
    let flag = text.hook(Site::MarkerFlag)?;
    let hibernation = text.hook(Site::MarkerHibernation)?;
    for (what, h) in [("flag", flag), ("hibernation", hibernation)] {
        if !(creator < h.va && h.resume <= creator + CREATOR_LEN) {
            return Err(format!("the creator's {what} site {:#x} is not inside the creator {creator:#x}", h.va));
        }
    }

    let m = text.find(Site::MarkerSwitch)?;
    let switch = text.hook(Site::MarkerSwitch)?;
    let switch_other = switch.resume.wrapping_add(u32_at(text.bytes_from(m), MARKER_SWITCH_OTHER_AT));
    let scale = text.hook(Site::MarkerScale)?;
    constant(img, Site::MarkerScale, u32_at(text.bytes_from(text.find(Site::MarkerScale)?), MARKER_SCALE_ONE_AT), 1.0)?;
    let mut disc_checks = [Hook { va: 0, resume: 0 }; 2];
    let mut disc_check_taken = [0; 2];
    for (k, (site, rel8_at)) in [(Site::MarkerCheck1, MARKER_CHECK1_REL8_AT), (Site::MarkerCheck2, MARKER_CHECK2_REL8_AT)].into_iter().enumerate() {
        let m = text.find(site)?;
        let h = text.hook(site)?;
        let rel8 = text.bytes_from(m)[rel8_at] as i8;
        disc_checks[k] = h;
        disc_check_taken[k] = h.resume.wrapping_add_signed(i32::from(rel8));
    }
    let order = [switch.va, scale.va, disc_checks[0].va, disc_checks[1].va];
    if !order.windows(2).all(|w| w[0] < w[1]) || order[3] - order[0] >= MARKER_UPDATE_SPAN {
        return Err(format!("the marker update's sites {order:#x?} are not one function's, in order, within {MARKER_UPDATE_SPAN:#x} bytes"));
    }
    if !(switch.resume < switch_other && switch_other < order[0] + MARKER_UPDATE_SPAN) {
        return Err(format!("the marker update's other-kinds path {switch_other:#x} is not after its disc path {:#x}", switch.resume));
    }
    Ok(MarkerSites { add_disc, creator, flag, hibernation, switch, switch_other, scale, disc_checks, disc_check_taken })
}

/// Every site in `img`.
pub fn resolve(img: &Image) -> Result<Sites, String> {
    let text = img.section(".text").ok_or("the image has no .text section")?;
    let text = Text { va: text.va, bytes: text.bytes };
    Ok(Sites {
        map: resolve_map(img, &text)?,
        reset: resolve_reset(&text)?,
        ammo: resolve_ammo(img, &text)?,
        marker: resolve_marker(img, &text)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_signature_parses_and_its_offsets_name_their_instructions() {
        for s in Site::ALL {
            let (pattern, at, len) = s.def();
            let p = Pattern::parse(pattern).unwrap_or_else(|e| panic!("{}: {e:?}", s.name()));
            assert!(at + len <= p.len(), "{}: the hooked bytes are inside the signature", s.name());
            assert!(len == 0 || len >= 5, "{}: a hook replaces at least the 5 bytes of its jump", s.name());
        }
        let toks = |s: Site| -> Vec<&'static str> { s.def().0.split_whitespace().collect() };
        let at = |s: Site| s.def().1;
        // Each hooked site starts with the instruction its detour reproduces.
        assert_eq!(toks(Site::Minimap)[at(Site::Minimap)..][..4], ["f3", "0f", "58", "15"]);
        assert_eq!(toks(Site::GpsLoadFirst)[at(Site::GpsLoadFirst)..][..4], ["f3", "0f", "10", "05"]);
        assert_eq!(toks(Site::GpsLoadAgain)[at(Site::GpsLoadAgain)..][..4], ["f3", "0f", "10", "0d"]);
        for (s, add, cvt) in [
            (Site::GpsScale1, "d0", "d2"),
            (Site::GpsScale2, "d1", "d2"),
            (Site::GpsScale3, "d0", "d2"),
            (Site::GpsScale4, "d1", "d2"),
            (Site::GpsScale5, "d0", "c2"),
            (Site::GpsScale6, "c1", "c0"),
        ] {
            assert_eq!(toks(s)[at(s)..][..7], ["f3", "0f", "58", add, "0f", "5a", cvt], "{}", s.name());
        }
        assert_eq!(toks(Site::Radar)[at(Site::Radar)..][..8], ["f3", "0f", "5e", "96", "9c", "01", "00", "00"]);
        assert_eq!(toks(Site::CoopX)[at(Site::CoopX)..][..4], ["f3", "0f", "58", "05"]);
        assert_eq!(toks(Site::CoopZ)[at(Site::CoopZ)..][..4], ["f3", "0f", "58", "0d"]);
        assert_eq!(toks(Site::PdaFlag)[at(Site::PdaFlag)..][..2], ["80", "3d"]);
        assert_eq!(toks(Site::DeactivateUpdate)[at(Site::DeactivateUpdate)..][..5], ["8b", "42", "18", "ff", "d0"]);
        for s in [Site::DeactivateRelease, Site::DeactivateAcquire] {
            let all = toks(s);
            assert_eq!(all[INLINE_DEACTIVATE_AT as usize..][..5], ["8b", "42", "18", "ff", "d0"], "{}", s.name());
            assert_eq!((all[6], all[7]), ("74", "05"), "{}: the je lands on the deactivate path", s.name());
            let tail = INLINE_DEACTIVATE_AT as usize + UPDATE_CALL_LEN as usize;
            assert_eq!(all[tail..][..11], *toks(Site::UpdateTail), "{}: the state store follows the call", s.name());
        }
        let upd = toks(Site::DeactivateUpdate);
        let deact = at(Site::DeactivateUpdate);
        let activate_tail = deact - UPDATE_DEACTIVATE_TO_ACTIVATE_TAIL as usize;
        assert_eq!(upd[activate_tail..][..11], *toks(Site::UpdateTail), "the activate call's state store");
        // Operands that are read, not matched.
        for (s, at) in [
            (Site::Minimap, MINIMAP_X_AT),
            (Site::Minimap, MINIMAP_Z_AT),
            (Site::GpsLoadFirst, GPS_FIRST_X_AT),
            (Site::GpsLoadFirst, GPS_FIRST_Z_AT),
            (Site::GpsLoadAgain, GPS_AGAIN_Z_AT),
            (Site::GpsLoadAgain, GPS_AGAIN_X_AT),
            (Site::CoopX, COOP_X_AT),
            (Site::CoopZ, COOP_Z_AT),
            (Site::PdaFlag, PDA_FLAG_AT),
            (Site::AmmoPresence, AMMO_CONTAINER_AT),
            (Site::AmmoPresence, AMMO_INDEX_CALL_AT + 1),
        ] {
            assert_eq!(toks(s)[at..at + 4], ["??"; 4], "{} operand at {at}", s.name());
        }
        for (at, _) in AMMO_FIELDS {
            assert_eq!(toks(Site::AmmoPresence)[at..at + 4], ["??"; 4], "AmmoPresence field at {at}");
        }
        assert_eq!(toks(Site::AmmoPresence)[AMMO_INDEX_CALL_AT], "e8");
        assert_eq!(toks(Site::MarkerFlag)[..13], ["f3", "0f", "11", "84", "24", "a8", "00", "00", "00", "88", "44", "24", "58"]);
        assert_eq!(toks(Site::MarkerHibernation)[at(Site::MarkerHibernation)..][..7], ["66", "c7", "44", "24", "14", "64", "00"]);
        assert_eq!(toks(Site::MarkerSwitch)[..12], ["8a", "45", "00", "c0", "e8", "04", "24", "03", "3c", "01", "0f", "85"]);
        assert_eq!(toks(Site::MarkerSwitch)[MARKER_SWITCH_OTHER_AT..][..4], ["??"; 4]);
        assert_eq!(toks(Site::MarkerScale)[at(Site::MarkerScale)..][..4], ["f3", "0f", "10", "05"]);
        assert_eq!(toks(Site::MarkerScale)[MARKER_SCALE_ONE_AT..][..4], ["??"; 4]);
        assert_eq!(toks(Site::MarkerScale)[at(Site::MarkerScale) + 8..][..5], ["f3", "0f", "5e", "45", "04"]);
        assert_eq!(toks(Site::MarkerCheck1)[at(Site::MarkerCheck1)..][..7], ["80", "e3", "30", "80", "fb", "10", "74"]);
        assert_eq!(toks(Site::MarkerCheck1)[MARKER_CHECK1_REL8_AT], "??");
        assert_eq!(toks(Site::MarkerCheck2)[at(Site::MarkerCheck2)..][..7], ["80", "e2", "30", "80", "fa", "10", "74"]);
        assert_eq!(toks(Site::MarkerCheck2)[MARKER_CHECK2_REL8_AT], "??");
        assert_eq!((PC_X.to_bits(), PC_Z.to_bits()), (0x420c_0000, 0x4220_0000));
    }
}
