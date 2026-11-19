//! The Windows side: `DllMain`, start-up, and the bindings' entry points.
//!
//! Start-up runs on a thread started from `DllMain`, never under the loader lock:
//! 1. `blow_it_up_again.log` (m2-sdk.dll's per-module log) and the m2-sdk.dll version check;
//! 2. m2-sdk.dll's Lua host (`m2_lua_install`), the `ChatterSet` component's sites in the running image
//!    (`mercs2_formats::chatter_sites`, which `Human.SetChatterSet` calls the engine through) and the sites of
//!    [`crate::sites`] (what `Gui.*MapCorrectionOffset`, `Gui._MarkerAddDiscDLC` and `Object.GetInfiniteAmmo`
//!    call and hook), the hooks, then each binding added to the engine's own table: the SDK
//!    registers a namespace with the engine's `luaL_register` right after the engine's own namespaces, and
//!    Lua 5.1's `luaL_register` adds to a global table that already exists rather than replacing it;
//! 3. a state callback that, on every state scripts run on, sets the aliases of
//!    [`crate::registry::ALIASES`] and checks that the engine's own functions are still there beside the
//!    added ones, and stops the game with a message when they are not.
//!
//! Every call of a binding is logged: a Lua error raised inside the DLC's sound bootstrap is swallowed by
//! the engine without a line, so this log is the only place it shows.
//!
//! Each hook replaces the instructions at one site with a detour here that does the same work with the
//! map correction's values or the DLC disc's, then jumps back after them (the sites, what they replace and
//! what is live there are in [`crate::sites`]). MinHook places the jump; no trampoline is used.
//!
//! Any start-up failure is fatal: logged, shown in a message box, and the process exits.

use core::ffi::{c_int, c_void};
use mercs2_formats::chatter_sites::{self, CONTAINER_ADD_BASE, CONTAINER_BUMP, CONTAINER_COUNT, CONTAINER_MASK_INDEX, ChatterSites};
use mercs2_formats::exe_image::{Image, parse_headers};
use mercs2_formats::mod_runtime::lua::{Lua, Ret, game};
use mercs2_formats::mod_runtime::{self as rt, LuaReg, ffi};
use std::ffi::CString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, GetModuleFileNameW, GetModuleHandleA};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::ExitProcess;
use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

use crate::human::{self, ChatterComponents};
use crate::map_correction::{self, DEFAULT_SCALE, DEFAULT_X, DEFAULT_Z, OFFSETS};
use crate::marker::{
    DLC_BITS, DLC_Y_SCALE, ENTITY_FADE_DIRTY, ENTITY_FADE_LENGTH, ENTITY_FADE_START, FADE_DIRTY_ALL_VIEWS, FADE_LENGTH, FADE_START,
    HIBERNATION_DISC, HIBERNATION_DLC, KEEP, KIND_DISC, KIND_DLC_DISC, KIND_MASK, KIND_SHIFT,
};
use crate::net;
use crate::object::{self, AmmoRecords, ContainerMemory};
use crate::sites::{self, Sites};

/// The m2-sdk.dll release this ASI is built against (manifest.yaml requires `>=0.6.2`).
const SDK_BUILT: &str = "0.6.2";
const TITLE: &str = "blow-it-up-again";

/// What the state check runs: each engine function beside each added one, whether each alias is the
/// function it names, and the `Marker` alias table the engine's own Gui chunk builds (logged after the
/// checked part).
const STATE_CHECK: &[u8] = b"return type(Net.IsServer), type(Net.DLCGetMountPoint), type(Human.SetFireLock), \
    type(Human.SetChatterSet), type(Gui._MarkerAddDisc), type(Gui._MarkerAddDiscDLC), \
    type(Gui.GetMapCorrectionOffset), type(Gui.SetMapCorrectionOffset), type(Object.SetInfiniteAmmo), \
    type(Object.GetInfiniteAmmo), tostring(rawequal(Marker.AddDiscDLC, Gui._MarkerAddDiscDLC)), type(Marker), \
    type(Marker and Marker.AddDisc)";
const STATE_CHECK_EXPECT: &[u8] = b"[ok]\t\"function\"\t\"function\"\t\"function\"\t\"function\"\t\"function\"\t\"function\"\t\
    \"function\"\t\"function\"\t\"function\"\t\"function\"\t\"true\"";
/// How long start-up waits for SecuROM to decrypt the code the sites are found in.
const SITES_WAIT: Duration = Duration::from_secs(30);
const SITES_RETRY: Duration = Duration::from_millis(50);
/// The headers fit in the first page of the image.
const HEADER_BYTES: usize = 0x1000;

static MODULE: AtomicUsize = AtomicUsize::new(0);
/// The `ChatterSet` sites, and the engine's lookup and add the thunks call through.
static CHATTER: OnceLock<ChatterSites> = OnceLock::new();
static LOOKUP_FN: AtomicUsize = AtomicUsize::new(0);
static ADD_FN: AtomicUsize = AtomicUsize::new(0);
/// The sites of [`crate::sites`].
static SITES: OnceLock<Sites> = OnceLock::new();

fn module() -> HMODULE {
    MODULE.load(Ordering::Acquire) as HMODULE
}

/// One line in `blow_it_up_again.log`.
fn log(line: &str) {
    let c = CString::new(line.replace('\0', " ")).expect("NULs replaced");
    unsafe { ffi::m2_logf_mod(module(), c"%s".as_ptr(), c.as_ptr()) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Log, show the reason, end the process.
fn fatal(msg: &str) -> ! {
    log(&format!("[!] blow_it_up_again: FATAL: {msg}"));
    let text = wide(&format!("{msg}\n\nThe game will now exit."));
    let title = wide(TITLE);
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR);
        ExitProcess(1)
    }
}

/// The game folder: the directory of the process's exe.
fn game_dir() -> Result<PathBuf, String> {
    let mut buf = vec![0u16; 1024];
    loop {
        let n = unsafe { GetModuleFileNameW(std::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
        if n == 0 {
            return Err(format!("GetModuleFileNameW failed: {}", std::io::Error::last_os_error()));
        }
        if n < buf.len() {
            let path = PathBuf::from(std::ffi::OsString::from_wide(&buf[..n]));
            return path.parent().map(PathBuf::from).ok_or_else(|| format!("{} has no parent directory", path.display()));
        }
        buf.resize(buf.len() * 2, 0);
    }
}

/// The names of the folders under the game folder's `DLC` folder, sorted.
fn level_folders() -> Result<Vec<String>, String> {
    let dir = game_dir()?.join(mercs2_formats::dlc_mount::DLC_DIR);
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let kind = entry.file_type().map_err(|e| format!("{}: {e}", entry.path().display()))?;
        if kind.is_dir() {
            let name = entry.file_name();
            names.push(name.to_str().ok_or_else(|| format!("{}: not valid text", entry.path().display()))?.to_string());
        }
    }
    names.sort();
    Ok(names)
}

/// Log a binding's outcome and hand it to the game.
fn logged(what: &str, ret: Ret) -> Ret {
    match &ret {
        Ret::Values(n) => log(&format!("[*] Lua {what} -> {n} value(s)")),
        Ret::Raise(m) => log(&format!("[!] Lua {what} raised: {m}")),
    }
    ret
}

unsafe extern "C" fn net_dlc_get_mount_point(l: *mut c_void) -> c_int {
    unsafe { game::call(l, |g| logged("Net.DLCGetMountPoint()", net::dlc_get_mount_point(g, level_folders()))) }
}

fn mapped() -> Result<Image<'static>, String> {
    let base = unsafe { GetModuleHandleA(core::ptr::null()) } as *const u8;
    if base.is_null() {
        return Err("GetModuleHandleA(NULL) returned no module".into());
    }
    let header = unsafe { core::slice::from_raw_parts(base, HEADER_BYTES) };
    let (image_base, headers) = parse_headers(header).map_err(|e| format!("the game's headers: {e}"))?;
    Ok(unsafe { Image::from_mapped(base, image_base, headers) })
}

/// `resolve` over the running image, retried until SecuROM has decrypted `.text` or [`SITES_WAIT`] runs out.
fn resolve_sites<T>(what: &str, resolve: impl Fn(&Image) -> Result<T, String>) -> Result<T, String> {
    let start = Instant::now();
    loop {
        let img = mapped()?;
        let last = match resolve(&img) {
            Ok(c) => return Ok(c),
            Err(e) => format!("{what}: {e}"),
        };
        if start.elapsed() >= SITES_WAIT {
            return Err(format!("after {} s, {last}", SITES_WAIT.as_secs()));
        }
        std::thread::sleep(SITES_RETRY);
    }
}

/// The engine's component lookup (`ECX` = container, `EAX` = guid; the record or 0 in `EAX`), as cdecl.
#[unsafe(naked)]
unsafe extern "C" fn call_lookup(container: u32, guid: u32) -> u32 {
    core::arch::naked_asm!(
        "push ebx",
        "push esi",
        "push edi",
        "mov ecx, dword ptr [esp+16]",
        "mov eax, dword ptr [esp+20]",
        "call dword ptr [{lookup}]",
        "pop edi",
        "pop esi",
        "pop ebx",
        "ret",
        lookup = sym LOOKUP_FN,
    );
}

/// The engine's component add (`EDI` = container + 0x18; stack `(guid, &value)`, popped by the callee), as cdecl.
#[unsafe(naked)]
unsafe extern "C" fn call_add(add_base: u32, guid: u32, value: *const u32) -> u32 {
    core::arch::naked_asm!(
        "push ebx",
        "push esi",
        "push edi",
        "mov edi, dword ptr [esp+16]",
        // `value` at esp+24; once it is pushed, `guid` is at esp+24 too.
        "push dword ptr [esp+24]",
        "push dword ptr [esp+24]",
        "call dword ptr [{add}]",
        "pop edi",
        "pop esi",
        "pop ebx",
        "ret",
        add = sym ADD_FN,
    );
}

/// The `ChatterSet` container through the engine's own functions.
struct Engine(&'static ChatterSites);

impl ChatterComponents for Engine {
    fn write_existing(&mut self, guid: u32, value: u32) -> bool {
        let record = unsafe { call_lookup(self.0.container, guid) };
        if record == 0 {
            return false;
        }
        unsafe { (record as usize as *mut u32).write_volatile(value) };
        true
    }
    fn count(&self) -> u32 {
        unsafe { ((self.0.container + CONTAINER_COUNT) as usize as *const u32).read_volatile() }
    }
    fn add(&mut self, guid: u32, value: u32) {
        unsafe { call_add(self.0.container + CONTAINER_ADD_BASE, guid, &value) };
    }
    fn bump(&mut self) {
        let p = (self.0.container + CONTAINER_BUMP) as usize as *mut u32;
        unsafe { p.write_volatile(p.read_volatile().wrapping_add(1)) };
    }
    fn flag(&self) -> bool {
        unsafe { (self.0.flag as usize as *const u8).read_volatile() != 0 }
    }
    fn mark(&mut self, guid: u32) {
        let index = unsafe { ((self.0.container + CONTAINER_MASK_INDEX) as usize as *const i16).read_volatile() };
        let mask: unsafe extern "C" fn(u32, i32) = unsafe { core::mem::transmute(self.0.mask as usize) };
        unsafe { mask(guid, i32::from(index)) };
    }
}

fn describe_object<L: Lua>(g: &L, i: i32) -> String {
    g.arg_object(i).map_or("not an object".to_string(), |o| format!("{o:#x}"))
}

unsafe extern "C" fn human_set_chatter_set(l: *mut c_void) -> c_int {
    let sites = CHATTER.get().expect("Human.SetChatterSet is registered only after its sites resolved");
    unsafe {
        game::call(l, |g| {
            let what = format!(
                "Human.SetChatterSet({}, {})",
                describe_object(g, 0),
                g.arg_string(1).map_or("not a string".to_string(), |s| format!("{:?}", String::from_utf8_lossy(&s)))
            );
            logged(&what, human::set_chatter_set(g, &mut Engine(sites)))
        })
    }
}

unsafe extern "C" fn gui_set_map_correction_offset(l: *mut c_void) -> c_int {
    unsafe {
        game::call(l, |g| {
            let ret = map_correction::set_map_correction_offset(g, &OFFSETS);
            logged(&format!("Gui.SetMapCorrectionOffset() [x, z, scale now {:?}]", OFFSETS.get()), ret)
        })
    }
}

unsafe extern "C" fn gui_get_map_correction_offset(l: *mut c_void) -> c_int {
    unsafe {
        game::call(l, |g| {
            logged(&format!("Gui.GetMapCorrectionOffset() [{:?}]", OFFSETS.get()), map_correction::get_map_correction_offset(g, &OFFSETS))
        })
    }
}

/// Live memory, as the presence test reads the container.
struct Live;

impl ContainerMemory for Live {
    fn u32_at(&self, va: u32) -> u32 {
        unsafe { (va as usize as *const u32).read_volatile() }
    }
    fn i16_at(&self, va: u32) -> i16 {
        unsafe { (va as usize as *const i16).read_volatile() }
    }
    fn u8_at(&self, va: u32) -> u8 {
        unsafe { (va as usize as *const u8).read_volatile() }
    }
}

/// The `CheatInfiniteAmmo` container through the engine's index function.
struct Ammo(sites::AmmoSites);

impl AmmoRecords for Ammo {
    fn record(&self, guid: u32) -> u32 {
        let index: unsafe extern "thiscall" fn(u32, u32) -> i32 = unsafe { core::mem::transmute(self.0.index as usize) };
        let slot = unsafe { index(self.0.container, guid) };
        object::record(&Live, self.0.container, slot)
    }
}

unsafe extern "C" fn object_get_infinite_ammo(l: *mut c_void) -> c_int {
    let sites = SITES.get().expect("Object.GetInfiniteAmmo is registered only after its sites resolved");
    unsafe {
        game::call(l, |g| {
            let what = format!("Object.GetInfiniteAmmo({})", describe_object(g, 0));
            logged(&what, object::get_infinite_ammo(g, &Ammo(sites.ammo)))
        })
    }
}

/// Set while PC's `_MarkerAddDisc` runs for `Gui._MarkerAddDiscDLC`: the creator's hooks write the DLC disc's
/// flag byte and hibernation distance. Lua runs on the game thread, as does the creator's other caller (the
/// replicated marker objectives), so nothing else creates a marker while it is set.
static MARKER_DLC: AtomicBool = AtomicBool::new(false);
static ADD_DISC_FN: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn gui_marker_add_disc_dlc(l: *mut c_void) -> c_int {
    if MARKER_DLC.swap(true, Ordering::AcqRel) {
        fatal("Gui._MarkerAddDiscDLC was entered while it was already running");
    }
    let add_disc: unsafe extern "C" fn(*mut c_void) -> c_int = unsafe { core::mem::transmute(ADD_DISC_FN.load(Ordering::Acquire)) };
    // PC's `_MarkerAddDisc` reads its arguments with readers that return a count and pushes its result inline;
    // it raises no Lua error, so it returns here.
    let n = unsafe { add_disc(l) };
    MARKER_DLC.store(false, Ordering::Release);
    log(&format!("[*] Lua Gui._MarkerAddDiscDLC() -> {n} value(s)"));
    n
}

const fn reg(name: &'static std::ffi::CStr, f: rt::LuaCFunction) -> LuaReg {
    LuaReg { name: name.as_ptr(), func: Some(f) }
}

const END: LuaReg = LuaReg { name: std::ptr::null(), func: None };
static NET_LIB: [LuaReg; 2] = [reg(c"DLCGetMountPoint", net_dlc_get_mount_point), END];
static HUMAN_LIB: [LuaReg; 2] = [reg(c"SetChatterSet", human_set_chatter_set), END];
static GUI_LIB: [LuaReg; 4] = [
    reg(c"GetMapCorrectionOffset", gui_get_map_correction_offset),
    reg(c"SetMapCorrectionOffset", gui_set_map_correction_offset),
    reg(c"_MarkerAddDiscDLC", gui_marker_add_disc_dlc),
    END,
];
static OBJECT_LIB: [LuaReg; 2] = [reg(c"GetInfiniteAmmo", object_get_infinite_ammo), END];

/// The chunk that sets [`crate::registry::ALIASES`] on a state; it raises when an alias's table is absent.
fn alias_chunk() -> Vec<u8> {
    let mut out = String::new();
    for (t, f, to_t, to_f) in crate::registry::ALIASES {
        out.push_str(&format!(
            "if type({t}) ~= \"table\" then error(\"{t} is \" .. type({t}) .. \", not the table the engine's Gui chunk builds\") end \
             {t}.{f} = {to_t}.{to_f} "
        ));
    }
    out.into_bytes()
}

static ALIAS_CHUNK: OnceLock<Vec<u8>> = OnceLock::new();

unsafe extern "C" fn check_state(l: *mut c_void, is_new_cycle: c_int, _ud: *mut c_void) {
    let chunk = ALIAS_CHUNK.get().expect("the alias chunk is built before the state callback is registered");
    let set = unsafe { rt::lua_exec(l, chunk, 512) };
    if set.status != rt::EXEC_OK {
        fatal(&format!("on lua_State {l:p} the aliases could not be set: {}", String::from_utf8_lossy(&set.text)));
    }
    let out = unsafe { rt::lua_exec(l, STATE_CHECK, 512) };
    let text = String::from_utf8_lossy(&out.text).into_owned();
    log(&format!("[*] state {l:p} (new cycle {is_new_cycle}): {text}"));
    if out.status != rt::EXEC_OK || !out.text.starts_with(STATE_CHECK_EXPECT) {
        fatal(&format!(
            "on lua_State {l:p} the engine's functions and this ASI's are not all there ({text}); \
             registering into the engine's table did not merge"
        ));
    }
}

/// The `(table, field)` names of `(table, rows)`, without the NULL-name row that ends each.
fn registered(tables: &[(&std::ffi::CStr, &[LuaReg])]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (table, rows) in tables {
        for row in rows.iter().take_while(|r| !r.name.is_null()) {
            let field = unsafe { std::ffi::CStr::from_ptr(row.name) };
            out.push((table.to_string_lossy().into_owned(), field.to_string_lossy().into_owned()));
        }
    }
    out.sort();
    out
}

// ---- The hooks' state: where each detour resumes, and what it reads. ----

static R_MINIMAP: AtomicUsize = AtomicUsize::new(0);
static R_GPS_LOAD: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];
static R_GPS_SCALE: [AtomicUsize; 6] =
    [AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0)];
static R_RADAR: AtomicUsize = AtomicUsize::new(0);
static R_COOP: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];
static R_PDA: AtomicUsize = AtomicUsize::new(0);
/// The address of the byte the PDA click handler tests where its hook sits.
static PDA_FLAG: AtomicUsize = AtomicUsize::new(0);
static R_UPDATE: AtomicUsize = AtomicUsize::new(0);
/// Per inlined update: `[activate path, deactivate path]`.
static R_INLINE: [[AtomicUsize; 2]; 2] = [[AtomicUsize::new(0), AtomicUsize::new(0)], [AtomicUsize::new(0), AtomicUsize::new(0)]];
/// PgSysGUI's init, `vtable+0x0C` of the GUI system.
static GUI_INIT: AtomicUsize = AtomicUsize::new(0);
static R_MARKER_FLAG: AtomicUsize = AtomicUsize::new(0);
static R_MARKER_HIBERNATION: AtomicUsize = AtomicUsize::new(0);
static R_MARKER_DISC: AtomicUsize = AtomicUsize::new(0);
static R_MARKER_OTHER: AtomicUsize = AtomicUsize::new(0);
static R_MARKER_SCALE: AtomicUsize = AtomicUsize::new(0);
/// Per disc test: `[not a disc, a disc]`.
static R_MARKER_CHECK: [[AtomicUsize; 2]; 2] = [[AtomicUsize::new(0), AtomicUsize::new(0)], [AtomicUsize::new(0), AtomicUsize::new(0)]];
/// The f32 operands the marker scale's detour loads.
static ONE: AtomicU32 = AtomicU32::new(1.0f32.to_bits());
static QUARTER: AtomicU32 = AtomicU32::new(DLC_Y_SCALE.to_bits());

/// `addss xmm2,[35]; addss xmm3,[40]` → `(x + fx) * s`, `(z + fz) * s`. SSE leaves EFLAGS alone, so the `jg`
/// after the site still tests the `cmp` before it.
#[unsafe(naked)]
unsafe extern "C" fn detour_minimap() {
    core::arch::naked_asm!(
        "addss xmm2, dword ptr [{o}]",
        "addss xmm3, dword ptr [{o} + 4]",
        "mulss xmm2, dword ptr [{o} + 8]",
        "mulss xmm3, dword ptr [{o} + 8]",
        "jmp dword ptr [{r}]",
        o = sym OFFSETS,
        r = sym R_MINIMAP,
    );
}

/// `movss xmm0,[35]; movss xmm1,[40]`: the GPS line's offsets, held across its loop.
#[unsafe(naked)]
unsafe extern "C" fn detour_gps_load_first() {
    core::arch::naked_asm!(
        "movss xmm0, dword ptr [{o}]",
        "movss xmm1, dword ptr [{o} + 4]",
        "jmp dword ptr [{r}]",
        o = sym OFFSETS,
        r = sym R_GPS_LOAD,
    );
}

/// `movss xmm1,[40]; movss xmm0,[35]`: the reload after the loop's critical section.
#[unsafe(naked)]
unsafe extern "C" fn detour_gps_load_again() {
    core::arch::naked_asm!(
        "movss xmm1, dword ptr [{o} + 4]",
        "movss xmm0, dword ptr [{o}]",
        "jmp dword ptr [{r} + 4]",
        o = sym OFFSETS,
        r = sym R_GPS_LOAD,
    );
}

/// The GPS line's six adds of an offset, each with the scale between it and its conversion:
/// `addss a,b; mulss a,[scale]; cvtps2pd c,d`, as each site's own registers.
#[unsafe(naked)]
unsafe extern "C" fn detour_gps_scale1() {
    core::arch::naked_asm!(
        "addss xmm2, xmm0",
        "mulss xmm2, dword ptr [{o} + 8]",
        "cvtps2pd xmm2, xmm2",
        "jmp dword ptr [{r}]",
        o = sym OFFSETS,
        r = sym R_GPS_SCALE,
    );
}

#[unsafe(naked)]
unsafe extern "C" fn detour_gps_scale2() {
    core::arch::naked_asm!(
        "addss xmm2, xmm1",
        "mulss xmm2, dword ptr [{o} + 8]",
        "cvtps2pd xmm2, xmm2",
        "jmp dword ptr [{r} + 4]",
        o = sym OFFSETS,
        r = sym R_GPS_SCALE,
    );
}

#[unsafe(naked)]
unsafe extern "C" fn detour_gps_scale3() {
    core::arch::naked_asm!(
        "addss xmm2, xmm0",
        "mulss xmm2, dword ptr [{o} + 8]",
        "cvtps2pd xmm2, xmm2",
        "jmp dword ptr [{r} + 8]",
        o = sym OFFSETS,
        r = sym R_GPS_SCALE,
    );
}

#[unsafe(naked)]
unsafe extern "C" fn detour_gps_scale4() {
    core::arch::naked_asm!(
        "addss xmm2, xmm1",
        "mulss xmm2, dword ptr [{o} + 8]",
        "cvtps2pd xmm2, xmm2",
        "jmp dword ptr [{r} + 12]",
        o = sym OFFSETS,
        r = sym R_GPS_SCALE,
    );
}

#[unsafe(naked)]
unsafe extern "C" fn detour_gps_scale5() {
    core::arch::naked_asm!(
        "addss xmm2, xmm0",
        "mulss xmm2, dword ptr [{o} + 8]",
        "cvtps2pd xmm0, xmm2",
        "jmp dword ptr [{r} + 16]",
        o = sym OFFSETS,
        r = sym R_GPS_SCALE,
    );
}

#[unsafe(naked)]
unsafe extern "C" fn detour_gps_scale6() {
    core::arch::naked_asm!(
        "addss xmm0, xmm1",
        "mulss xmm0, dword ptr [{o} + 8]",
        "cvtps2pd xmm0, xmm0",
        "jmp dword ptr [{r} + 20]",
        o = sym OFFSETS,
        r = sym R_GPS_SCALE,
    );
}

/// `divss xmm2,[esi+0x19c]` → `(radius / range) * s`.
#[unsafe(naked)]
unsafe extern "C" fn detour_radar() {
    core::arch::naked_asm!(
        "divss xmm2, dword ptr [esi + 0x19c]",
        "mulss xmm2, dword ptr [{o} + 8]",
        "jmp dword ptr [{r}]",
        o = sym OFFSETS,
        r = sym R_RADAR,
    );
}

/// `addss xmm0,[35]` → `(mid + x) * s`.
#[unsafe(naked)]
unsafe extern "C" fn detour_coop_x() {
    core::arch::naked_asm!(
        "addss xmm0, dword ptr [{o}]",
        "mulss xmm0, dword ptr [{o} + 8]",
        "jmp dword ptr [{r}]",
        o = sym OFFSETS,
        r = sym R_COOP,
    );
}

/// `addss xmm1,[40]` → `(mid + z) * s`.
#[unsafe(naked)]
unsafe extern "C" fn detour_coop_z() {
    core::arch::naked_asm!(
        "addss xmm1, dword ptr [{o} + 4]",
        "mulss xmm1, dword ptr [{o} + 8]",
        "jmp dword ptr [{r} + 4]",
        o = sym OFFSETS,
        r = sym R_COOP,
    );
}

/// Before `cmp byte [flag],0`, after the `SetGPSDest` event took the raw clicked position: the stored position
/// becomes `Q / s − x`, `Q / s − z` (`EDI` = the PDA). Then the `cmp` itself, whose flags the `je` after it
/// reads. `EAX` is kept; `XMM0` is reloaded before its next read on both paths.
#[unsafe(naked)]
unsafe extern "C" fn detour_pda() {
    core::arch::naked_asm!(
        "movss xmm0, dword ptr [edi + 0x39c]",
        "divss xmm0, dword ptr [{o} + 8]",
        "subss xmm0, dword ptr [{o}]",
        "movss dword ptr [edi + 0x39c], xmm0",
        "movss xmm0, dword ptr [edi + 0x3a0]",
        "divss xmm0, dword ptr [{o} + 8]",
        "subss xmm0, dword ptr [{o} + 4]",
        "movss dword ptr [edi + 0x3a0], xmm0",
        "push eax",
        "mov eax, dword ptr [{flag}]",
        "cmp byte ptr [eax], 0",
        "pop eax",
        "jmp dword ptr [{r}]",
        o = sym OFFSETS,
        flag = sym PDA_FLAG,
        r = sym R_PDA,
    );
}

/// The system state update's deactivate call, `mov eax,[edx+0x18]; call eax` (`ECX` = the system, `EDX` its
/// vtable): when the system is the GUI's, the map correction's values go back to the defaults first, as PS3's
/// PgSysGUI slot 7 does.
#[unsafe(naked)]
unsafe extern "C" fn detour_deactivate_update() {
    core::arch::naked_asm!(
        "mov eax, dword ptr [ecx]",
        "mov eax, dword ptr [eax + 0xc]",
        "cmp eax, dword ptr [{gui}]",
        "jne 2f",
        "mov dword ptr [{o}], {x0}",
        "mov dword ptr [{o} + 4], {z0}",
        "mov dword ptr [{o} + 8], {s0}",
        "2:",
        "mov eax, dword ptr [edx + 0x18]",
        "call eax",
        "jmp dword ptr [{r}]",
        gui = sym GUI_INIT,
        o = sym OFFSETS,
        x0 = const DEFAULT_X.to_bits(),
        z0 = const DEFAULT_Z.to_bits(),
        s0 = const DEFAULT_SCALE.to_bits(),
        r = sym R_UPDATE,
    );
}

/// An inlined update's `test bl,bl; mov edx,[esi]; mov ecx,esi; jz deactivate`: the same, with the reset on
/// the deactivate path. `$k` is the copy (0: release loop, 1: acquire loop).
macro_rules! detour_inline {
    ($name:ident, $k:literal) => {
        #[unsafe(naked)]
        unsafe extern "C" fn $name() {
            core::arch::naked_asm!(
                "mov edx, dword ptr [esi]",
                "mov ecx, esi",
                "test bl, bl",
                "jz 2f",
                concat!("jmp dword ptr [{r} + 8 * ", $k, "]"),
                "2:",
                "mov eax, dword ptr [ecx]",
                "mov eax, dword ptr [eax + 0xc]",
                "cmp eax, dword ptr [{gui}]",
                "jne 3f",
                "mov dword ptr [{o}], {x0}",
                "mov dword ptr [{o} + 4], {z0}",
                "mov dword ptr [{o} + 8], {s0}",
                "3:",
                concat!("jmp dword ptr [{r} + 8 * ", $k, " + 4]"),
                gui = sym GUI_INIT,
                o = sym OFFSETS,
                x0 = const DEFAULT_X.to_bits(),
                z0 = const DEFAULT_Z.to_bits(),
                s0 = const DEFAULT_SCALE.to_bits(),
                r = sym R_INLINE,
            );
        }
    };
}

detour_inline!(detour_inline_release, 0);
detour_inline!(detour_inline_acquire, 1);

/// The creator's `movss [esp+0xa8],xmm0; mov [esp+0x58],al`: for a DLC disc the flag byte is
/// `(AL & 0x80) | 0x36` (kind 3, pulsing, removed when the pulse halts).
#[unsafe(naked)]
unsafe extern "C" fn detour_marker_flag() {
    core::arch::naked_asm!(
        "movss dword ptr [esp + 0xa8], xmm0",
        "cmp byte ptr [{dlc}], 0",
        "je 2f",
        "and al, {keep}",
        "or al, {bits}",
        "2:",
        "mov byte ptr [esp + 0x58], al",
        "jmp dword ptr [{r}]",
        dlc = sym MARKER_DLC,
        keep = const KEEP,
        bits = const DLC_BITS,
        r = sym R_MARKER_FLAG,
    );
}

/// The creator's `mov word [esp+0x14],100`: 400 for a DLC disc. `EAX` (the record's address, an argument of
/// the call after it) is kept.
#[unsafe(naked)]
unsafe extern "C" fn detour_marker_hibernation() {
    core::arch::naked_asm!(
        "cmp byte ptr [{dlc}], 0",
        "jne 2f",
        "mov word ptr [esp + 0x14], {disc}",
        "jmp dword ptr [{r}]",
        "2:",
        "mov word ptr [esp + 0x14], {dlc_distance}",
        "jmp dword ptr [{r}]",
        dlc = sym MARKER_DLC,
        disc = const HIBERNATION_DISC,
        dlc_distance = const HIBERNATION_DLC,
        r = sym R_MARKER_HIBERNATION,
    );
}

/// The update's kind switch, `mov al,[ebp]; shr al,4; and al,3; cmp al,1; jne other` (`EBP` = the marker,
/// `EBX` = its spawned entity): kind 3 takes the disc path after the entity's fade writes PS3 v1.03's kind 2
/// makes. Other kinds leave with `AL` and the flags the replaced instructions left.
#[unsafe(naked)]
unsafe extern "C" fn detour_marker_switch() {
    core::arch::naked_asm!(
        "mov al, byte ptr [ebp]",
        "shr al, {shift}",
        "and al, 3",
        "cmp al, {disc}",
        "je 3f",
        "cmp al, {dlc}",
        "jne 2f",
        "or word ptr [ebx + {dirty}], {all_views}",
        "mov dword ptr [ebx + {start}], {start_v}",
        "mov dword ptr [ebx + {length}], {length_v}",
        "3:",
        "jmp dword ptr [{disc_path}]",
        "2:",
        "cmp al, {disc}",
        "jmp dword ptr [{other}]",
        shift = const KIND_SHIFT,
        disc = const KIND_DISC,
        dlc = const KIND_DLC_DISC,
        dirty = const ENTITY_FADE_DIRTY,
        all_views = const FADE_DIRTY_ALL_VIEWS,
        start = const ENTITY_FADE_START,
        start_v = const FADE_START.to_bits(),
        length = const ENTITY_FADE_LENGTH,
        length_v = const FADE_LENGTH.to_bits(),
        disc_path = sym R_MARKER_DISC,
        other = sym R_MARKER_OTHER,
    );
}

/// The model's Y scale, `movss xmm0,[1.0]; divss xmm0,[ebp+4]` (1/radius): 0.25 for a DLC disc. `EAX` is
/// dead here.
#[unsafe(naked)]
unsafe extern "C" fn detour_marker_scale() {
    core::arch::naked_asm!(
        "mov al, byte ptr [ebp]",
        "and al, {mask}",
        "cmp al, {dlc}",
        "je 2f",
        "movss xmm0, dword ptr [{one}]",
        "divss xmm0, dword ptr [ebp + 4]",
        "jmp dword ptr [{r}]",
        "2:",
        "movss xmm0, dword ptr [{quarter}]",
        "jmp dword ptr [{r}]",
        mask = const KIND_MASK,
        dlc = const KIND_DLC_DISC << KIND_SHIFT,
        one = sym ONE,
        quarter = sym QUARTER,
        r = sym R_MARKER_SCALE,
    );
}

/// A disc test, `and REG,0x30; cmp REG,0x10; je disc`: a DLC disc is a disc too. Neither target reads the
/// flags.
macro_rules! detour_marker_check {
    ($name:ident, $reg:literal, $k:literal) => {
        #[unsafe(naked)]
        unsafe extern "C" fn $name() {
            core::arch::naked_asm!(
                concat!("and ", $reg, ", {mask}"),
                concat!("cmp ", $reg, ", {disc}"),
                "je 2f",
                concat!("cmp ", $reg, ", {dlc}"),
                "je 2f",
                concat!("jmp dword ptr [{r} + 8 * ", $k, "]"),
                "2:",
                concat!("jmp dword ptr [{r} + 8 * ", $k, " + 4]"),
                mask = const KIND_MASK,
                disc = const KIND_DISC << KIND_SHIFT,
                dlc = const KIND_DLC_DISC << KIND_SHIFT,
                r = sym R_MARKER_CHECK,
            );
        }
    };
}

detour_marker_check!(detour_marker_check1, "bl", 0);
detour_marker_check!(detour_marker_check2, "dl", 1);

fn store(slot: &AtomicUsize, va: u32) {
    slot.store(va as usize, Ordering::Release);
}

/// Every hook: record where each detour resumes and what it reads, then attach them.
fn install(s: &Sites) -> Result<(), String> {
    let m = &s.map;
    store(&R_MINIMAP, m.minimap.resume);
    for k in 0..2 {
        store(&R_GPS_LOAD[k], m.gps_load[k].resume);
        store(&R_COOP[k], m.coop[k].resume);
    }
    for k in 0..6 {
        store(&R_GPS_SCALE[k], m.gps_scale[k].resume);
    }
    store(&R_RADAR, m.radar.resume);
    store(&R_PDA, m.pda.resume);
    store(&PDA_FLAG, m.pda_flag);
    let r = &s.reset;
    store(&R_UPDATE, r.update.resume);
    for k in 0..2 {
        store(&R_INLINE[k][0], r.inline[k].resume);
        store(&R_INLINE[k][1], r.inline_deactivate[k]);
    }
    store(&GUI_INIT, r.gui_init);
    let mk = &s.marker;
    store(&ADD_DISC_FN, mk.add_disc);
    store(&R_MARKER_FLAG, mk.flag.resume);
    store(&R_MARKER_HIBERNATION, mk.hibernation.resume);
    store(&R_MARKER_DISC, mk.switch.resume);
    store(&R_MARKER_OTHER, mk.switch_other);
    store(&R_MARKER_SCALE, mk.scale.resume);
    for k in 0..2 {
        store(&R_MARKER_CHECK[k][0], mk.disc_checks[k].resume);
        store(&R_MARKER_CHECK[k][1], mk.disc_check_taken[k]);
    }
    let hooks: [(&str, u32, unsafe extern "C" fn()); 22] = [
        ("minimap", m.minimap.va, detour_minimap),
        ("GPS line offsets", m.gps_load[0].va, detour_gps_load_first),
        ("GPS line offsets, again", m.gps_load[1].va, detour_gps_load_again),
        ("GPS line scale 1", m.gps_scale[0].va, detour_gps_scale1),
        ("GPS line scale 2", m.gps_scale[1].va, detour_gps_scale2),
        ("GPS line scale 3", m.gps_scale[2].va, detour_gps_scale3),
        ("GPS line scale 4", m.gps_scale[3].va, detour_gps_scale4),
        ("GPS line scale 5", m.gps_scale[4].va, detour_gps_scale5),
        ("GPS line scale 6", m.gps_scale[5].va, detour_gps_scale6),
        ("radar blip", m.radar.va, detour_radar),
        ("co-op midpoint x", m.coop[0].va, detour_coop_x),
        ("co-op midpoint z", m.coop[1].va, detour_coop_z),
        ("PDA map click", m.pda.va, detour_pda),
        ("system deactivate", r.update.va, detour_deactivate_update),
        ("system release (reset all)", r.inline[0].va, detour_inline_release),
        ("system acquire (reset all)", r.inline[1].va, detour_inline_acquire),
        ("marker flag byte", mk.flag.va, detour_marker_flag),
        ("marker hibernation distance", mk.hibernation.va, detour_marker_hibernation),
        ("marker kind switch", mk.switch.va, detour_marker_switch),
        ("marker Y scale", mk.scale.va, detour_marker_scale),
        ("marker disc test 1", mk.disc_checks[0].va, detour_marker_check1),
        ("marker disc test 2", mk.disc_checks[1].va, detour_marker_check2),
    ];
    for (what, va, detour) in hooks {
        unsafe { rt::hook_attach(va as usize as *mut c_void, detour as *mut c_void, None, what) }?;
        log(&format!("[*] blow_it_up_again: hooked the {what} at {va:#x}"));
    }
    Ok(())
}

fn start() -> Result<(), String> {
    unsafe { ffi::m2_log_init(module()) };
    log("==========================================");
    log(&format!("[*] blow_it_up_again {} loading", env!("CARGO_PKG_VERSION")));
    log("==========================================");
    rt::compatible(rt::loaded_version(), SDK_BUILT)?;
    let tables: [(&'static std::ffi::CStr, &'static [LuaReg]); 4] =
        [(c"Gui", &GUI_LIB), (c"Human", &HUMAN_LIB), (c"Net", &NET_LIB), (c"Object", &OBJECT_LIB)];
    let mut declared: Vec<(String, String)> =
        crate::registry::ADDED.iter().map(|(t, f)| (t.to_string(), f.to_string())).collect();
    declared.sort();
    if registered(&tables) != declared {
        return Err(format!("the registered names {:?} are not registry::ADDED {declared:?}", registered(&tables)));
    }
    rt::lua_install().map_err(|e| format!("m2-sdk.dll's Lua host: {e}"))?;
    let chatter = resolve_sites("the ChatterSet sites", |img| chatter_sites::resolve(img).map_err(|e| e.to_string()))?;
    log(&format!(
        "[*] blow_it_up_again: ChatterSet container {:#x} (writer {:#x}); lookup {:#x}, add {:#x}, mask {:#x}, flag {:#x}",
        chatter.container, chatter.writer, chatter.lookup, chatter.add, chatter.mask, chatter.flag
    ));
    LOOKUP_FN.store(chatter.lookup as usize, Ordering::Release);
    ADD_FN.store(chatter.add as usize, Ordering::Release);
    if CHATTER.set(chatter).is_err() {
        return Err("the ChatterSet sites were resolved twice".into());
    }
    let sites = resolve_sites("the map correction, marker and CheatInfiniteAmmo sites", sites::resolve)?;
    log(&format!("[*] blow_it_up_again: sites {sites:#x?}"));
    if SITES.set(sites).is_err() {
        return Err("the sites were resolved twice".into());
    }
    install(&sites)?;
    if ALIAS_CHUNK.set(alias_chunk()).is_err() {
        return Err("the alias chunk was built twice".into());
    }
    for (table, rows) in tables {
        rt::lua_add_namespace(table, rows).map_err(|e| format!("adding to {}: {e}", table.to_string_lossy()))?;
    }
    unsafe { rt::lua_on_state(check_state, std::ptr::null_mut()) }?;
    log(&format!(
        "[*] blow_it_up_again: {} added through m2-sdk.dll; ready",
        crate::registry::provided().iter().map(|(t, f)| format!("{t}.{f}")).collect::<Vec<_>>().join(", ")
    ));
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "system" fn DllMain(h: HMODULE, reason: u32, _reserved: *mut c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        MODULE.store(h as usize, Ordering::Release);
        unsafe { DisableThreadLibraryCalls(h) };
        if let Err(e) = std::thread::Builder::new().name("blow_it_up_again start".into()).spawn(|| {
            if let Err(e) = start() {
                fatal(&e);
            }
        }) {
            fatal(&format!("could not start the start-up thread: {e}"));
        }
    }
    1
}
