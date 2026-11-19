//! The Windows side: `DllMain`, start-up, and the bindings' entry points.
//!
//! Start-up runs on a thread started from `DllMain`, never under the loader lock:
//! 1. `blow_it_up_again.log` (m2-sdk.dll's per-module log) and the m2-sdk.dll version check;
//! 2. m2-sdk.dll's Lua host (`m2_lua_install`), the `ChatterSet` component's sites in the running image
//!    (`mercs2_formats::chatter_sites`, which `Human.SetChatterSet` calls the engine through), then each binding
//!    added to the engine's own table: the SDK
//!    registers a namespace with the engine's `luaL_register` right after the engine's own namespaces, and
//!    Lua 5.1's `luaL_register` adds to a global table that already exists rather than replacing it;
//! 3. a state callback that checks, on every state scripts run on, that the engine's own functions are
//!    still there beside the added ones, and stops the game with a message when they are not.
//!
//! Every call of a binding is logged: a Lua error raised inside the DLC's sound bootstrap is swallowed by
//! the engine without a line, so this log is the only place it shows.
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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, GetModuleFileNameW, GetModuleHandleA};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::ExitProcess;
use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

use crate::human::{self, ChatterComponents};
use crate::net;

/// The m2-sdk.dll release this ASI is built against (manifest.yaml requires `>=0.6.2`).
const SDK_BUILT: &str = "0.6.2";
const TITLE: &str = "blow-it-up-again";

/// What the state check runs: each engine function beside each added one, and the `Marker` alias table the
/// engine's own Gui chunk builds (whether it exists yet on this state says when the chunk ran).
const STATE_CHECK: &[u8] = b"return type(Net.IsServer), type(Net.DLCGetMountPoint), type(Human.SetFireLock), \
    type(Human.SetChatterSet), type(Marker), type(Marker and Marker.AddDisc)";
const STATE_CHECK_EXPECT: &[u8] = b"[ok]\t\"function\"\t\"function\"\t\"function\"\t\"function\"";
/// How long start-up waits for SecuROM to decrypt the code the `ChatterSet` sites are found in.
const SITES_WAIT: Duration = Duration::from_secs(30);
const SITES_RETRY: Duration = Duration::from_millis(50);
/// The headers fit in the first page of the image.
const HEADER_BYTES: usize = 0x1000;

static MODULE: AtomicUsize = AtomicUsize::new(0);
/// The `ChatterSet` sites, and the engine's lookup and add the thunks call through.
static CHATTER: OnceLock<ChatterSites> = OnceLock::new();
static LOOKUP_FN: AtomicUsize = AtomicUsize::new(0);
static ADD_FN: AtomicUsize = AtomicUsize::new(0);

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

/// The `ChatterSet` sites, retried until SecuROM has decrypted `.text` or [`SITES_WAIT`] runs out.
fn resolve_sites() -> Result<ChatterSites, String> {
    let start = Instant::now();
    loop {
        let img = mapped()?;
        let last = match chatter_sites::resolve(&img) {
            Ok(c) => return Ok(c),
            Err(e) => format!("the ChatterSet sites: {e}"),
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

unsafe extern "C" fn human_set_chatter_set(l: *mut c_void) -> c_int {
    let sites = CHATTER.get().expect("Human.SetChatterSet is registered only after its sites resolved");
    unsafe {
        game::call(l, |g| {
            let what = format!(
                "Human.SetChatterSet({}, {})",
                g.arg_object(0).map_or("not an object".to_string(), |o| format!("{o:#x}")),
                g.arg_string(1).map_or("not a string".to_string(), |s| format!("{:?}", String::from_utf8_lossy(&s)))
            );
            logged(&what, human::set_chatter_set(g, &mut Engine(sites)))
        })
    }
}

const fn reg(name: &'static std::ffi::CStr, f: rt::LuaCFunction) -> LuaReg {
    LuaReg { name: name.as_ptr(), func: Some(f) }
}

const END: LuaReg = LuaReg { name: std::ptr::null(), func: None };
static NET_LIB: [LuaReg; 2] = [reg(c"DLCGetMountPoint", net_dlc_get_mount_point), END];
static HUMAN_LIB: [LuaReg; 2] = [reg(c"SetChatterSet", human_set_chatter_set), END];

unsafe extern "C" fn check_state(l: *mut c_void, is_new_cycle: c_int, _ud: *mut c_void) {
    let out = unsafe { rt::lua_exec(l, STATE_CHECK, 512) };
    let text = String::from_utf8_lossy(&out.text).into_owned();
    log(&format!("[*] state {l:p} (new cycle {is_new_cycle}): {text}"));
    if out.status != rt::EXEC_OK || !out.text.starts_with(STATE_CHECK_EXPECT) {
        fatal(&format!(
            "on lua_State {l:p} the engine's Net functions and this ASI's are not both there ({text}); \
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

fn start() -> Result<(), String> {
    unsafe { ffi::m2_log_init(module()) };
    log("==========================================");
    log(&format!("[*] blow_it_up_again {} loading", env!("CARGO_PKG_VERSION")));
    log("==========================================");
    rt::compatible(rt::loaded_version(), SDK_BUILT)?;
    let tables: [(&'static std::ffi::CStr, &'static [LuaReg]); 2] = [(c"Human", &HUMAN_LIB), (c"Net", &NET_LIB)];
    let mut declared: Vec<(String, String)> =
        crate::registry::ADDED.iter().map(|(t, f)| (t.to_string(), f.to_string())).collect();
    declared.sort();
    if registered(&tables) != declared {
        return Err(format!("the registered names {:?} are not registry::ADDED {declared:?}", registered(&tables)));
    }
    rt::lua_install().map_err(|e| format!("m2-sdk.dll's Lua host: {e}"))?;
    let chatter = resolve_sites()?;
    log(&format!(
        "[*] blow_it_up_again: ChatterSet container {:#x} (writer {:#x}); lookup {:#x}, add {:#x}, mask {:#x}, flag {:#x}",
        chatter.container, chatter.writer, chatter.lookup, chatter.add, chatter.mask, chatter.flag
    ));
    LOOKUP_FN.store(chatter.lookup as usize, Ordering::Release);
    ADD_FN.store(chatter.add as usize, Ordering::Release);
    if CHATTER.set(chatter).is_err() {
        return Err("the ChatterSet sites were resolved twice".into());
    }
    for (table, rows) in tables {
        rt::lua_add_namespace(table, rows).map_err(|e| format!("adding to {}: {e}", table.to_string_lossy()))?;
    }
    unsafe { rt::lua_on_state(check_state, std::ptr::null_mut()) }?;
    log(&format!("[*] blow_it_up_again: {} added through m2-sdk.dll; ready", declared.iter().map(|(t, f)| format!("{t}.{f}")).collect::<Vec<_>>().join(", ")));
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
