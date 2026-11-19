//! What each `Net` function does, independent of how it is called.
//!
//! * `Net.DLCGetMountPoint()` → the level's folder, relative to the game folder (`DLC\<level>`,
//!   [`mercs2_formats::dlc_mount`]). PS3 v1.03 (`0x005370B8`) reads no argument and returns the installed
//!   DLC's folder; the DLC's sound bootstrap opens its streams under it. The level is the one folder under
//!   the game folder's `DLC`; none or several raise a Lua error naming what is there.

use mercs2_formats::dlc_mount::{mount_point, single_level};
use mercs2_formats::mod_runtime::lua::{Lua, Ret};

/// `Net.DLCGetMountPoint()`, given the names of the folders under the game folder's `DLC` folder.
pub fn dlc_get_mount_point<L: Lua>(s: &mut L, folders: Result<Vec<String>, String>) -> Ret {
    let folders = match folders {
        Ok(f) => f,
        Err(e) => return Ret::Raise(format!("Net.DLCGetMountPoint: {e}")),
    };
    match single_level(&folders) {
        Ok(level) => {
            s.push_string(mount_point(level).as_bytes());
            Ret::Values(1)
        }
        Err(e) => Ret::Raise(format!("Net.DLCGetMountPoint: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mercs2_formats::mod_runtime::lua::fake::{FakeLua, Val, s};

    #[test]
    fn the_mount_point_is_the_level_folder() {
        let mut l = FakeLua::default();
        assert_eq!(dlc_get_mount_point(&mut l, Ok(vec!["dlc01".into()])), Ret::Values(1));
        assert_eq!(l.pushed, [s("DLC\\dlc01")]);
    }

    #[test]
    fn it_reads_no_argument_like_the_ps3_binding() {
        let mut l = FakeLua::with(vec![Val::Num(3.0)]);
        assert_eq!(dlc_get_mount_point(&mut l, Ok(vec!["dlc01".into()])), Ret::Values(1));
        assert_eq!(l.pushed, [s("DLC\\dlc01")]);
    }

    #[test]
    fn no_level_or_several_or_an_unreadable_folder_raises() {
        let raised = |r: Ret| match r {
            Ret::Raise(m) => m,
            other => panic!("expected a raise, got {other:?}"),
        };
        let mut l = FakeLua::default();
        assert!(raised(dlc_get_mount_point(&mut l, Ok(vec![]))).contains("no level folder"));
        assert!(raised(dlc_get_mount_point(&mut l, Ok(vec!["a".into(), "b".into()]))).contains("a, b"));
        assert!(raised(dlc_get_mount_point(&mut l, Err("no DLC folder".into()))).contains("no DLC folder"));
        assert!(l.pushed.is_empty());
    }
}
