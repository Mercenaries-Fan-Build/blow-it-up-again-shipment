//! The DLC's scripts name no engine function that neither PC v1.1, this ASI, nor a Shipment this one
//! requires provides.
//!
//! A call to a name PC lacks raises a Lua error the engine swallows without a line (that is how
//! `Net.DLCGetMountPoint` stopped every sound bank), so this is checked from data, not found in a run:
//!
//! * what PC provides: `tests/data/pc-v11-lua-surface.tsv`, every field of every global table, and of each
//!   table one level below, read from the running game through the Lua bridge, with this ASI's own names
//!   left out (its header names the exe's sha256). The binding tables alone are not the surface: the
//!   engine also builds tables in Lua (`Marker` over `Gui._Marker*`) and registers `Event`'s constants
//!   natively;
//! * what this ASI adds: [`blow_it_up_again::registry::ADDED`] and the alias fields of
//!   [`blow_it_up_again::registry::ALIASES`];
//! * what the Shipments `manifest.yaml` requires add: the `Table.Field` entries of their native_hook `touches`,
//!   from their `manifest.yaml` under the research checkout's `shipments/` (each required name must be the
//!   `shipment.name` of exactly one of them; multiplayer-restored provides the DLC's leaderboard, user-name and
//!   cheats bindings);
//! * what the scripts reference: every `Table.Field` in the decompiled DLC Lua (`docs/mercs2-dlc-luacd/src` in
//!   the research checkout this Shipment sits in) whose table is a global table of that surface.
//!
//! A name the shipped DLC script itself gets wrong, which raises on every platform, is listed in
//! [`KNOWN_DLC_FAULTS`] with the one place the script names it; the test checks it is named there and
//! nowhere else, and that no platform provides it.
//!
//! A missing input fails the test.

use std::collections::{BTreeMap, BTreeSet};

/// Faults in the shipped DLC scripts, as `(table, field, file under the research checkout, line)`.
/// `Object.Delete` is registered by none of the 13 executable images (PC, PS3 and Xbox 360,
/// `output/_exemap_exports/*.luareg.tsv`; parent repo `docs/reverse_engineer/dlc_engine_bindings.md`, "Not a
/// binding"), so the call raises wherever the script runs.
const KNOWN_DLC_FAULTS: &[(&str, &str, &str, usize)] = &[("Object", "Delete", "docs/mercs2-dlc-luacd/src/dlc01/dlccon004_cash.lua", 98)];
use std::path::{Path, PathBuf};

fn research_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// `(table, field)` rows of the surface file; `table` may be dotted (`Graphics.Camera`).
fn surface(path: &Path) -> BTreeSet<(String, String)> {
    let text = String::from_utf8(read(path)).expect("the surface file is text");
    let mut rows = BTreeSet::new();
    for line in text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let (table, field) = line.split_once('\t').unwrap_or_else(|| panic!("{}: a row without a tab: {line:?}", path.display()));
        rows.insert((table.to_string(), field.to_string()));
    }
    assert!(!rows.is_empty(), "{} holds no row", path.display());
    rows
}

fn yaml(path: &Path) -> serde_norway::Value {
    serde_norway::from_slice(&read(path)).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The `shipment.name` of a manifest.
fn shipment_name(path: &Path, manifest: &serde_norway::Value) -> String {
    manifest["shipment"]["name"].as_str().unwrap_or_else(|| panic!("{}: no `shipment.name` string", path.display())).to_string()
}

/// The `shipment` names of a manifest's `load.requires`. Every entry must name a Shipment.
fn required_shipments(path: &Path, manifest: &serde_norway::Value) -> Vec<String> {
    let requires = manifest["load"]["requires"].as_sequence().unwrap_or_else(|| panic!("{}: no `load.requires` list", path.display()));
    requires
        .iter()
        .map(|r| r["shipment"].as_str().unwrap_or_else(|| panic!("{}: a `load.requires` entry without a `shipment` name: {r:?}", path.display())).to_string())
        .collect()
}

/// The `Table.Field` entries of a manifest's native_hook `touches`, as `(table, field)`. A touch without a `.`
/// (a C symbol or an address) names no Lua field.
fn hook_lua_touches(path: &Path, manifest: &serde_norway::Value) -> BTreeSet<(String, String)> {
    let contributions = manifest["contributions"].as_sequence().unwrap_or_else(|| panic!("{}: no `contributions` list", path.display()));
    let mut out = BTreeSet::new();
    for c in contributions.iter().filter(|c| c["kind"].as_str() == Some("native_hook")) {
        let touches = c["touches"].as_sequence().unwrap_or_else(|| panic!("{}: a native_hook without a `touches` list", path.display()));
        for t in touches {
            let t = t.as_str().unwrap_or_else(|| panic!("{}: a native_hook touch that is not a string: {t:?}", path.display()));
            if let Some((table, field)) = t.split_once('.') {
                out.insert((table.to_string(), field.to_string()));
            }
        }
    }
    out
}

/// Every `shipments/*/manifest.yaml` of the research checkout, by `shipment.name`.
fn shipment_manifests(root: &Path) -> BTreeMap<String, Vec<PathBuf>> {
    let dir = root.join("shipments");
    let mut out: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap_or_else(|e| panic!("{}: {e}", dir.display())).path().join("manifest.yaml");
        if path.is_file() {
            let name = shipment_name(&path, &yaml(&path));
            out.entry(name).or_default().push(path);
        }
    }
    out
}

/// This Shipment's own native_hook touches are exactly what the ASI registers and the aliases it sets.
#[test]
fn this_shipments_hook_touches_are_what_the_asi_adds() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../manifest.yaml");
    let added: BTreeSet<(String, String)> = blow_it_up_again::registry::provided().into_iter().collect();
    assert_eq!(hook_lua_touches(&path, &yaml(&path)), added);
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Every `A.B` in `text` where `A` starts a word, with its byte offset.
fn dotted(text: &[u8]) -> Vec<(String, String, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if is_ident_start(text[i]) && (i == 0 || !(is_ident(text[i - 1]) || text[i - 1] == b'.')) {
            let a0 = i;
            while i < text.len() && is_ident(text[i]) {
                i += 1;
            }
            if i + 1 < text.len() && text[i] == b'.' && is_ident_start(text[i + 1]) {
                let mut j = i + 1;
                while j < text.len() && is_ident(text[j]) {
                    j += 1;
                }
                out.push((String::from_utf8_lossy(&text[a0..i]).into_owned(), String::from_utf8_lossy(&text[i + 1..j]).into_owned(), a0));
            }
        } else {
            i += 1;
        }
    }
    out
}

fn lua_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            lua_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "lua") {
            out.push(path);
        }
    }
}

#[test]
fn the_dlc_names_no_engine_function_pc_and_this_asi_lack() {
    let pc = surface(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/pc-v11-lua-surface.tsv"));
    let asi = blow_it_up_again::registry::provided();
    for (table, field) in &asi {
        assert!(!pc.contains(&(table.clone(), field.clone())), "{table}.{field} is in PC's own surface, so this ASI must not add it");
    }
    let tables: BTreeSet<String> = pc.iter().map(|(t, _)| t.clone()).filter(|t| !t.contains('.')).collect();
    let mut provided = pc;
    provided.extend(asi);

    let root = research_root();
    let own = Path::new(env!("CARGO_MANIFEST_DIR")).join("../manifest.yaml");
    let manifests = shipment_manifests(&root);
    for name in required_shipments(&own, &yaml(&own)) {
        let path = match manifests.get(&name).map(Vec::as_slice) {
            Some([one]) => one,
            None | Some([]) => panic!("no shipments/*/manifest.yaml under {} names the required Shipment {name:?}", root.display()),
            Some(several) => panic!(
                "the required Shipment {name:?} is the shipment.name of {} manifests, so which one provides it is not known: {}",
                several.len(),
                several.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
            ),
        };
        provided.extend(hook_lua_touches(path, &yaml(path)));
    }
    let mut files = Vec::new();
    lua_files(&root.join("docs/mercs2-dlc-luacd/src"), &mut files);
    files.sort();
    assert!(!files.is_empty(), "no DLC Lua under docs/mercs2-dlc-luacd/src");
    let mut missing: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for file in &files {
        let text = read(file);
        for (a, b, at) in dotted(&text) {
            if tables.contains(&a) && !provided.contains(&(a.clone(), b.clone())) {
                let line = text[..at].iter().filter(|c| **c == b'\n').count() + 1;
                let rel = file.strip_prefix(&root).unwrap_or(file).display().to_string();
                missing.entry((a, b)).or_default().push(format!("{rel}:{line}"));
            }
        }
    }
    for (table, field, file, line) in KNOWN_DLC_FAULTS {
        let key = (table.to_string(), field.to_string());
        assert!(!provided.contains(&key), "{table}.{field} is listed as a DLC fault but is provided");
        let sites = missing.remove(&key).unwrap_or_default();
        assert_eq!(sites, [format!("{file}:{line}")], "the DLC fault {table}.{field} is named at exactly its listed place");
    }
    let report: Vec<String> = missing.iter().map(|((a, b), sites)| format!("{a}.{b}: {}", sites.join(", "))).collect();
    assert!(missing.is_empty(), "the DLC names {} engine field(s) neither PC v1.1, this ASI, nor a required Shipment provides:\n{}", missing.len(), report.join("\n"));
}
