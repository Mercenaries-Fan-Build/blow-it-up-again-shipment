//! Every site the ASI calls or hooks resolves exactly once, at the address read from the disassembly, in the
//! PC reference builds: the v1.0-code builds `v10-securom-unwrap`, `v10-drmfree-laa` and
//! `dump-v10-pmcbb-unpacked` (one layout) and the v1.1 dump `dump-v11-securom-unpacked` (code at other
//! addresses), the builds the research checkout's other site tests use.
//!
//! Each build is found by its catalogued sha256 and size (`crates/mercs2_exemap/data/builds.tsv` of the
//! research checkout's `tools/wad_simulator`) under the canonical tree of PC builds that its
//! `.mercs2-local.toml` key `canonical_dir` names. A missing input fails the test.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use blow_it_up_again::sites::{AmmoSites, Hook, MapSites, MarkerSites, ResetSites, Sites, resolve};
use mercs2_formats::exe_image::Image;
use mercs2_formats::game_paths::local_config_canonical_dir;
use sha2::{Digest, Sha256};

/// The builds under test, each with whether it runs the v1.1 code.
const BUILDS: [(&str, bool); 4] =
    [("v10-securom-unwrap", false), ("v10-drmfree-laa", false), ("dump-v10-pmcbb-unpacked", false), ("dump-v11-securom-unpacked", true)];

fn wad_simulator() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tools/wad_simulator")
}

/// `build_id -> (sha256, size)` from the catalogue.
fn catalogue() -> BTreeMap<String, (String, u64)> {
    let path = wad_simulator().join("crates/mercs2_exemap/data/builds.tsv");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap_or_else(|| panic!("{} is empty", path.display())).split('\t').collect();
    let col = |name: &str| header.iter().position(|h| *h == name).unwrap_or_else(|| panic!("{}: no {name} column", path.display()));
    let (id, sha, size) = (col("build_id"), col("sha256"), col("size"));
    lines
        .filter(|l| !l.is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let n = f[size].parse().unwrap_or_else(|e| panic!("{}: size of {}: {e}", path.display(), f[id]));
            (f[id].to_string(), (f[sha].to_string(), n))
        })
        .collect()
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap_or_else(|e| panic!("{}: {e}", dir.display())).path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// The bytes of every build in [`BUILDS`], in order, each checked against its catalogued sha256.
fn images() -> Vec<(&'static str, bool, Vec<u8>)> {
    let c = catalogue();
    let dir = local_config_canonical_dir(&wad_simulator()).unwrap_or_else(|e| panic!("{e}"));
    let wanted: Vec<&(String, u64)> = BUILDS.iter().map(|(id, _)| c.get(*id).unwrap_or_else(|| panic!("build {id} is not catalogued"))).collect();
    let mut files = Vec::new();
    files_under(&dir, &mut files);
    let mut by_sha: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for path in files {
        let size = std::fs::metadata(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())).len();
        if wanted.iter().any(|(_, n)| *n == size) {
            let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            by_sha.entry(format!("{:x}", Sha256::digest(&bytes))).or_insert(bytes);
        }
    }
    BUILDS
        .iter()
        .map(|&(id, v11)| {
            let (sha, _) = &c[id];
            let bytes = by_sha.remove(sha).unwrap_or_else(|| panic!("build {id} (sha256 {sha}) is not under {}", dir.display()));
            (id, v11, bytes)
        })
        .collect()
}

fn hook(va: u32, len: u32) -> Hook {
    Hook { va, resume: va + len }
}

/// The sites, read with Ghidra: pc-v11 (`dump-v11-securom-unpacked`) and pc-v10 (`dump-v10-pmcbb-unpacked`).
fn expected(v11: bool) -> Sites {
    let pick = |a: u32, b: u32| if v11 { a } else { b };
    Sites {
        map: MapSites {
            minimap: hook(pick(0x0062_037d, V10_MINIMAP), 16),
            gps_load: [hook(pick(0x0062_0ae7, V10_GPS_LOAD[0]), 16), hook(pick(0x0062_0bd3, V10_GPS_LOAD[1]), 16)],
            gps_scale: [
                hook(pick(0x0062_0b1b, V10_GPS_SCALE[0]), 7),
                hook(pick(0x0062_0b2c, V10_GPS_SCALE[1]), 7),
                hook(pick(0x0062_0b3e, V10_GPS_SCALE[2]), 7),
                hook(pick(0x0062_0b5f, V10_GPS_SCALE[3]), 7),
                hook(pick(0x0062_0c37, V10_GPS_SCALE[4]), 7),
                hook(pick(0x0062_0c54, V10_GPS_SCALE[5]), 7),
            ],
            radar: hook(pick(0x0062_13ba, V10_RADAR), 8),
            coop: [hook(pick(0x0062_16e9, V10_COOP[0]), 8), hook(pick(0x0062_1705, V10_COOP[1]), 8)],
            pda: hook(pick(0x006a_0ef6, V10_PDA), 7),
            pda_flag: pick(0x00df_ad75, V10_PDA_FLAG),
        },
        reset: ResetSites {
            update: hook(pick(0x004e_b018, V10_UPDATE), 5),
            inline: [hook(pick(0x004c_07ae, V10_INLINE[0]), 8), hook(pick(0x004c_08c8, V10_INLINE[1]), 8)],
            inline_deactivate: [pick(0x004c_07bb, V10_INLINE[0] + 13), pick(0x004c_08d5, V10_INLINE[1] + 13)],
            gui_init: pick(0x004f_6c30, V10_GUI_INIT),
        },
        ammo: AmmoSites { container: pick(0x00df_8b10, 0x00df_9b10), index: pick(0x0064_9640, 0x0064_96b0) },
        marker: MarkerSites {
            add_disc: pick(0x005b_38c0, 0x005b_38d0),
            creator: pick(0x0061_9e90, 0x0061_9f90),
            flag: hook(pick(0x0061_9f9a, 0x0061_a09a), 13),
            hibernation: hook(pick(0x0061_9fe3, 0x0061_a0e3), 7),
            switch: hook(pick(0x0061_aa52, 0x0061_ab52), 16),
            switch_other: pick(0x0061_ab3f, 0x0061_ac3f),
            scale: hook(pick(0x0061_aae4, 0x0061_abe4), 13),
            disc_checks: [hook(pick(0x0061_ac87, 0x0061_ad87), 8), hook(pick(0x0061_ad11, 0x0061_ae11), 8)],
            disc_check_taken: [pick(0x0061_acc3, 0x0061_adc3), pick(0x0061_ad3e, 0x0061_ae3e)],
        },
    }
}

const V10_MINIMAP: u32 = 0x0062_045d;
const V10_GPS_LOAD: [u32; 2] = [0x0062_0bc7, 0x0062_0cb3];
const V10_GPS_SCALE: [u32; 6] = [0x0062_0bfb, 0x0062_0c0c, 0x0062_0c1e, 0x0062_0c3f, 0x0062_0d17, 0x0062_0d34];
const V10_RADAR: u32 = 0x0062_149a;
const V10_COOP: [u32; 2] = [0x0062_17c9, 0x0062_17e5];
const V10_PDA: u32 = 0x006a_0f56;
const V10_PDA_FLAG: u32 = 0x00df_bd75;
const V10_UPDATE: u32 = 0x004e_ae88;
const V10_INLINE: [u32; 2] = [0x004c_07fe, 0x004c_0918];
const V10_GUI_INIT: u32 = 0x004f_6a40;

#[test]
fn every_site_resolves_once_in_every_reference_build() {
    for (id, v11, bytes) in images() {
        let img = Image::from_file(&bytes).unwrap_or_else(|e| panic!("{id}: {e}"));
        let got = resolve(&img).unwrap_or_else(|e| panic!("{id}: {e}"));
        println!("{id}: {got:#x?}");
        assert_eq!(got, expected(v11), "{id}");
    }
}
