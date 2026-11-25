# Headless Path of Building bridge

Runs [Path of Building Community](https://github.com/PathOfBuildingCommunity/PathOfBuilding)
(MIT-style licence) without a GUI under stock LuaJIT, so poecraft can score items against a real build.

## Setup
```
bash pob_bridge/setup.sh          # LuaJIT + lua-zlib + luautf8, PoB pinned to 2.69.0 in $POB_HOME (/home/user/pob)
```
PoB itself is not vendored into this repo (it is ~1.2 GB with data); the script clones it at a pinned commit.

## Use
```
python pob_bridge/decode_pob_code.py build.pobcode build.xml       # PoB export code -> XML
cd /home/user/pob/PathOfBuilding/src
luajit /home/user/Personal-poe-craft-sim/pob_bridge/load_build.lua build.xml </dev/null   # stats
luajit /home/user/Personal-poe-craft-sim/pob_bridge/time_calcs.lua build.xml "Body Armour" </dev/null
```
Always pass `</dev/null`: on an error PoB shows "Press Enter to dismiss" and would otherwise wait forever.

## What had to be fixed to run under stock LuaJIT
* `lua-utf8` C module: built from starwing/luautf8 0.1.6 (the version PoB's Dockerfile installs).
* `Inflate`/`Deflate`: `_SimpleGraphic.def.lua` stubs them to return `""`, which silently drops
  timeless-jewel lookup tables ("Missing LUT: Lethal Pride") and made DPS 14% too low on the test
  build. `pob_env.lua` replaces them with lua-zlib.
* PoB 2.69.0 `Classes/CompareBuySimilar.lua` uses `?.` and `..=` (not Lua 5.1/LuaJIT syntax);
  `pob-2.69.0-fix.patch` rewrites those three lines. It is the only such file in `src/` outside
  `Export/`.
* PoB replaces the global `arg` while booting: read command-line arguments first.

## Validation (2026-11-25)
SanaviCovens (level 100 Slayer, Cyclone of Tumult; export code from the owner): every stat the
export stores (TotalDPS 91,874,011, CombinedDPS, crit, life, EHP, max hits, resistances) matches
headless PoB to 0.00%.

## Speed (same build, this container, 1 thread)
* full output rebuild: ~43 ms
* item-swap calculation (`GetMiscCalculator` with `repSlotName`/`repItem`, what PoB's tooltips use): ~7.5 ms
  (~130 items/second per thread)
* boot + load build: ~4 s (pay once per worker)

## Relation to `tools/pob/` and `pob-headless`
The local web app (`crates/poecraft-server`, "Test in PoB") drives an *installed* Windows PoB through
the sibling `pob-headless` harness (`POB_HEADLESS`) with `tools/pob/score_items.lua`. This bridge is
the Linux/cloud equivalent (stock LuaJIT + a pinned PoB checkout), so Claude sessions without the
owner's machine can load builds and score items too. Measured here: the tooltip calculator
(`GetMiscCalculator`) gives the same TotalDPS as a full rebuild for an unchanged item, at ~6x the speed.
