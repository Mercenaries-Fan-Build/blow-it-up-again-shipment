#!/usr/bin/env bash
# Build blow_it_up_again.asi and place it at ../src/ (the Shipment's plugin path).
# Requires: rustup target i686-pc-windows-gnu, i686-w64-mingw32-gcc on PATH, and
# scripts/fetch_dw2_libgcc.sh run once per rustc version.
#
# WAD_SIMULATOR_DIR names the wad_simulator checkout whose mercs2_formats the client builds against
# (mercs2_formats::dlc_mount is newer than the published crate). Its m2-sdk crate's MinGW libraries
# (crates/mercs2_mod_runtime/toolchain/i686-mingw, from scripts/fetch_mingw_libs.sh there, the rust-mingw
# component) are linked with Rust's self-contained CRT start-up objects, as m2-sdk.dll is, so the ASI links
# msvcrt.dll whatever CRT the host MinGW defaults to (Homebrew's is the UCRT).
set -euo pipefail
client="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$client"
: "${WAD_SIMULATOR_DIR:?set WAD_SIMULATOR_DIR to the wad_simulator checkout to build mercs2_formats from}"
wad_simulator="$(cd "$WAD_SIMULATOR_DIR" && pwd)"
test -f "$wad_simulator/crates/mercs2_formats/Cargo.toml"
mingw="$wad_simulator/crates/mercs2_mod_runtime/toolchain/i686-mingw"
if [[ "$(cat "$mingw/VERSION" 2>/dev/null)" != "$(rustc -V)" ]]; then
  echo "$mingw holds no MinGW libraries for $(rustc -V); run $wad_simulator/crates/mercs2_mod_runtime/scripts/fetch_mingw_libs.sh" >&2
  exit 1
fi
RUSTFLAGS="-C link-self-contained=yes -L native=$mingw" cargo build --release --target i686-pc-windows-gnu \
  --config "patch.crates-io.mercs2_formats.path=\"$wad_simulator/crates/mercs2_formats\""
dll="target/i686-pc-windows-gnu/release/blow_it_up_again.dll"
# Rust's i686-pc-windows-gnu std targets msvcrt, as every plugin the game loads does. A MinGW that defaults
# to the UCRT (Homebrew's) links api-ms-win-crt-*.dll instead; refuse that build.
pe="target/i686-pc-windows-gnu/release/blow_it_up_again.pe.txt"
i686-w64-mingw32-objdump -p "$dll" > "$pe"
if ! grep -q 'DLL Name: msvcrt.dll' "$pe" || grep -q 'DLL Name: api-ms-win-crt' "$pe"; then
  echo "$dll does not link msvcrt.dll only; its imports are in $pe" >&2
  exit 1
fi
asi="../src/blow_it_up_again.asi"
if [[ -f "$asi" ]]; then
  echo "before: $(shasum -a 256 "$asi")"
fi
cp "$dll" "$asi"
echo "after:  $(shasum -a 256 "$asi")"
