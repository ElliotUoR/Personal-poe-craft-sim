# Notes for future Claude sessions driving poecraft

Workflow when the user describes a craft in plain English:
1. Find the mods: `python -m poecraft mods "<base>" --influence <i1>,<i2> --ilvl N --search "<text>"`.
   Use `--elevated` for elevated versions and `--tag caster` (etc.) to see what a Harvest reforge
   tag can hit. Reuse or add short names in `data/aliases.json`.
2. Write a recipe in `recipes/<name>.craft`. Reuse `recipes/lib/*.craft` (`include`). Always add
   a `goal` and, for nice-to-haves, `track` lines.
3. `python -m poecraft check recipes/<name>.craft`. The errors give `file:line` and a hint.
4. `python -m poecraft sample recipes/<name>.craft --seed 1` to see one finished item.
5. `python -m poecraft run ... --mode both` on small parts to cross-check exact vs Monte Carlo,
   then `compare` the candidate routes.
6. Report success %, cost per success, p90 and risk. State which prices are estimates
   (`data/prices.json` → `estimates`).

Rules of thumb found so far:
* Check "no other suffixes" (etc.) **before** paying for Dominance, not after.
* "Suffixes Cannot Be Changed" is a prefix (and vice versa). Keep a slot open for it.
* Every locked Harvest reforge retry needs a new metamod. The beastcraft (Wild Bristle Matron)
  is cheaper than the 3.30 bench craft (4 div + 4 blessed).
* "Add a Mod to a <Conqueror> Item" beastcrafts cost ~2–3c and are the cheap way to get a
  second influenced mod for Dominance.

Refreshing data: see `tools/build_data.py` (RePoE via git) and the `source`/`fetched` fields of
`data/prices.json`. In this environment poewiki.net returns 403. PoEDB, sanavixx.com and
outof.games are reachable once allowed in the environment's network settings.
