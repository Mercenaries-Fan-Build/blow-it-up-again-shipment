# Blow It Up Again

The Blow It Up Again DLC as a level on PC. This Shipment carries none of the DLC's files: when you build it,
`qm` downloads Sony's PS3 package of the DLC, checks it against the size and sha256 in `manifest.yaml`,
converts it on your machine, and builds the level from the result, so the rest of your Shipments can reach it.

## Play it

Install it with Modkit (from mercs.ink) and Build & Deploy. The first build downloads the package (about
390 MB) into qm's cache and converts it, which takes a minute or two; later builds reuse both. Then start
the game: **Extras ▸ DLC ▸ Blow It Up Again** starts the DLC's level.

In the main campaign, Eva's PMC shop sells the DLC's vehicles (the M1A1 and M1A3 tanks, the Hammerhead,
the Recoilless Panhard and The Hadron), and the wardrobe offers the Obama, Palin and Gramps outfits.

It requires m2-sdk, the unofficial patch and multiplayer-restored; Modkit installs them with it.

## Build it by hand

```
qm lint <this folder>
qm build <this folder> --game <game>
```

`qm lint` needs no package and no network. `qm build` downloads the package unless you name a copy you
already have with `--package biua_ps3=<path to the .pkg>`; either way it is checked against the manifest.
The package is the whole source. PS3 stored a few things less exactly than PC: ground cover reuses a table
of rotations, so a plant can stand up to 10° off, and normals are a little coarser.

## What it installs

`_build/placement.json` lists these, each with its size and sha256:

| file | how |
|---|---|
| `data/dlc01-patch.wad` | compiled at build |
| `data/dlc01.wad` | a copy of your `data/vz.wad`, made at deploy |
| `DLC/dlc01/Data/Audios/*.pws` | built at build, 7 files |
| `scripts/blow_it_up_again.asi` | the Lua functions the DLC calls that PC never had |

The DLC's leaderboards come from multiplayer-restored: it posts your best score per contract to its server
as your multiplayer-restored account, when you have signed in online that session; without a session,
scores stay local, as on a console offline. mercs.ink shows the boards.

The engine opens `data\dlc01.wad`, then `data\dlc01-patch.wad` above it. The DLC does not run alone: its
archive lacks most of what its scripts import, so the level sits over the base game. `vz.wad` itself is
never changed. The copy costs as much disk as `vz.wad`, about 2.4 GB. The main campaign's additions ship in
the Shipment's overlay above `vz.wad`.

## What it carries

1,664 assets: the 1,641 the base game lacks, and 23 the DLC overrides under the base's names. Anything the
base already holds is left to it. See `docs/modernization/blow_it_up_again.md`.

## Releases

A release's `blow-it-up-again-v<version>.zip` holds `manifest.yaml`, `src/blow_it_up_again.asi`,
`src/strings/`, `src/lua/`, this README and `NOTICE`. CI builds the ASI from `client/`, lints the Shipment
and the zip with the `qm` from the Workshop Tools release pinned in `.github/qm-version`, and attaches the
zip to the release (`.github/workflows/release.yml`). Run the workflow with a version, or push a
`v<version>` tag that equals `shipment.version`.

## Attribution

Blow It Up Again is © Electronic Arts / Pandemic Studios. This repository and its releases carry none of
the DLC's files: the package is downloaded from Sony's CDN on the player's machine. This is an unofficial
fan project, not affiliated with or endorsed by Electronic Arts, Pandemic Studios or Sony. See `NOTICE`.
