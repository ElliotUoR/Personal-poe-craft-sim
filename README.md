# poecraft — Path of Exile 1 crafting simulator / calculator

Write a crafting strategy as a small **recipe** and get its success probability, the currency
it is expected to use, and its expected cost in chaos and divines, with risk and variance.
Small cases are computed **exactly** (enumeration plus absorbing Markov chains). Larger ones use
**Monte Carlo**. When both run, the results are cross-checked.

The tool is meant to be driven by Claude: you describe a craft in plain English, Claude writes a
recipe, runs it, and compares alternatives.

There are two engines with the same recipe language and commands:

* **Rust (main engine, phase 1 of [docs/spec.md](docs/spec.md))**: `crates/`. Monte Carlo runs on
  every CPU core and is roughly 300x faster than Python (8,000 chest runs in about 0.4 s).
* **Python (reference)**: `poecraft/`. It stays as the cross-check until the Rust results have
  matched it on every recipe for a while (`tools/parity_engine.py`, `tools/parity_data.py`).

```
cargo build --release                         # Rust 1.80+; binary: target/release/poecraft
target/release/poecraft run recipes/endgame_chest.craft
target/release/poecraft compare recipes/boots/*.craft
target/release/poecraft rules                 # every rule with its source and status
cargo test --release                          # Rust tests

pip install numpy pytest                      # Python reference
python -m poecraft run recipes/endgame_chest.craft
python -m pytest -q
python tools/parity_engine.py                 # Rust vs Python on every recipe
```

**Web app (local only):** double-click `start.bat` (or `cargo run --release -p poecraft-server`),
then open http://127.0.0.1:8790. Tabs: Item builder (base, item level, influences, legal mods only,
mod pool with weights/chances and why a mod is blocked, elevated versions, bench crafts, crafting
hints computed from the pool, paste/import, Copy item for PoB), Simulator (pick or edit a recipe,
live progress bar), Compare (rank several recipes), Rules, Prices. The JSON API is documented at the
top of `crates/poecraft-server/src/main.rs`.

Test in PoB: the item builder's "Test in Path of Building" card scores the item in any of your
PoB builds (found under Documents/Path of Building/Builds) through the sibling `pob-headless`
harness (`tools/pob/score_items.lua`): Pinnacle boss, guard skills excluded from EHP, optional
resists-capped fill, flasks up and down, DPS/EHP/max-hit change against the item currently in the
slot. About 4.5 s per build; the build files are never modified. Env overrides: POB_HEADLESS,
POB_BUILDS.

Trade links: the item builder's "Open trade search" (or `POST /api/trade/link`) builds a
pathofexile.com search for the item: one stat filter per mod line (lower end of the tier's range,
negative values as a maximum), influence filters, exact base or any base of the class, instant
buyout. Stat ids come from `data/trade_stats.json` (`python tools/fetch_trade_stats.py`).

Item text (spec phase 2, started 2026-11-18): `poecraft sample RECIPE --pob` prints the crafted
item as Path of Building text (paste via Items > Create custom), and `poecraft parse-item FILE|-`
reads PoB or in-game clipboard text (incl. Ctrl+Alt+C advanced copy) back into a crafting item.
Checked end to end through PoB itself: PoB reads the export with both influences, and our parser
recovers the identical item from PoB's re-export.

Every Rust result lists the unverified or disputed rules it depends on (see `poecraft rules`
and docs/spec.md §7). Rust-only additions so far: partial locks (`craft lock_1_suffix`,
`lock_2_suffixes`, `lock_1_prefix`, `lock_2_prefixes`), "cannot roll attack/caster modifiers"
(`craft no_attack_mods` / `no_caster_mods`), metamods for every item class, and all item classes
(armour, weapons, jewellery, quivers, shields, jewels, flasks, tinctures).

## Commands
| command | what it does |
| --- | --- |
| `run RECIPE [--runs N] [--mode auto/exact/mc/both] [--json] [--show-item]` | Evaluate one recipe. `auto` tries exact first (Rust: 10 s / 300k paths; Python: 90 s) and falls back to Monte Carlo. `both` reports both plus an agreement check. |
| `compare R1 R2 ... [--sort cost_per_success/mean/risk/std] [-v] [--json]` | Rank recipes against the same prices. |
| `sample RECIPE [--seed S]` | Run once, print the final item, the cost and any `show` output. |
| `check RECIPE` | Parse and run 20 times to catch errors quickly. |
| `mods "Astral Plate" --influence hunter,warlord --ilvl 86 [--side prefix] [--search text] [--tag caster] [--elevated]` | List rollable mods with weights and percentages. |
| `prices [--grep x]` | Show the price table in use. |
| global `--prices FILE` | Merge a price file over `data/prices.json` (repeatable). `data/prices.local.json` is merged automatically if present (git-ignored). |

The Python API mirrors the CLI: `poecraft.lang.parse_file`, `poecraft.engine.Engine`,
`poecraft.analysis.analyse/compare`.

## Recipe language (short version; full reference: [docs/language.md](docs/language.md))
```
include "lib/elevate.craft"                       # reusable sub-recipes

buy "Astral Plate" ilvl 86 influence warlord for price "base:astral_plate_warlord_86"
repeat {
    repeat { alt_regal_two_mods("Warlord: mana cost") } until suffix_count == 1
    if influenced_count < 2 { beastcraft add_mod warlord }
    until has "Warlord: mana cost" elevated or not has "Warlord: mana cost" { dominance }
} until has "Warlord: mana cost" elevated and suffix_count == 1
beastcraft lock_suffixes
scour
set_aside warlord
...
take warlord
awaken hunter
beastcraft lock_suffixes
harvest reforge caster
until has "flat life" tier <= 2 or open_prefix == 0 limit 10 { beastcraft lock_suffixes  harvest reforge caster }

goal has "Warlord: mana cost" elevated and has "Hunter: attack crit" elevated and has "Hunter: additional curse"
track "life prefix" has "flat life" tier <= 2
```

## First test case: Sanavixx "Endgame Chest"
The recipes follow the method as the project owner crafts it (corrections of 2026-11-18):
* "Suffixes Cannot Be Changed" on a **rare** is the bench craft (4 div + 4 blessed). The metamod
  beastcraft only works on **magic** items.
* **One** locked Caster reforge. Then 2 open prefixes → Foulborn exalt; 1 open → bench
  (56–74)% armour (flat armour / life if the % armour group is taken); full → keep the item.
  Annulling to make room is too risky.
* Hinekora previews on the Awakener are used to farm a good **third suffix** (T1–T2
  fire/cold/lightning/chaos resistance or Warlord all-res) together with an open prefix.

Recipes: `endgame_chest.craft` (with preview farming) and `endgame_chest_no_preview.craft`
(Warlord piece stays rare; take whatever the Awakener gives). The two pieces can also be run
alone: `chest_warlord_piece.craft` and `chest_hunter_piece.craft`.

3000 Monte Carlo runs each; prices from outof.games (Covens league) fetched 2026-11-18,
divine = 203c.

| recipe | success | expected cost | cost per success | p90 | good 3rd suffix | prefixes full after reforge |
| --- | --- | --- | --- | --- | --- | --- |
| endgame_chest_no_preview | 89.8% | **15.3 div** | 17.0 div | 20.9 div | 10.3% | 21.5% |
| endgame_chest (Hinekora farming) | 100% | 35.0 div | 35.0 div | 51.2 div | 87.3% | 8.3% |

(Re-run 2026-11-18 after two data fixes: mods that spawn only via the `default` tag, such as
life regen and some flat-life tiers, were missing from every pool; and tiers are now ranked per
base instead of across all item classes.)

Where the money goes in the preview version: ~8.5 Hinekora previews (Craicic Croaker + Black
Mórrigan ≈ 1.5 div each, ≈ 13 div), 2 bench locks (8 div), 4 Dominance (6.4 div), a Foulborn
exalt in 66% of runs (3.2 div each).

**Compared with the ~17 div market price:** taking the Awakener result as it comes costs about
15 div (p90 21 div). The cheapest elevated-crit Royal Plates on trade were 18–20 div
(2026-11-18), and selling at 16 div yields about +0.7 div per attempt (`endgame_chest_sell.craft`). 11% of runs get stuck (no open prefix for the lock) and need a new
attempt. Farming a T1–T2 resistance third suffix with previews roughly doubles the cost.
Each good suffix needs ~8.5 previews, and that assumes a declined preview can be re-locked for a
fresh one (UNVERIFIED). That is only worth it if such an item sells for well over 17 div.

Prefix outcomes after the single locked Caster reforge (Awakener taken as it comes; the 89% of
items that had room for the lock): curse only (2 open) 58%, curse + 1 (1 open) 31%,
curse + 2 (full) 12%.

**Checks:**
* *Sanity check (2 influenced mods, 50% per Dominance):* the Hunter piece uses **2.002**
  Dominance (exact) and the Warlord piece **1.99**. Exact and Monte Carlo agree on both pieces
  (`--mode both`).
* *Trap found:* if the Regal is allowed to add a non-influenced suffix before Dominance, the
  Warlord piece needs 4.3 Dominance (+3.4 div). Check "no other suffixes" **before**
  paying for Dominance.
* "Suffixes Cannot Be Changed" is itself a *prefix*, so the Awakener result must leave one
  prefix open (about 90% do).
* **Dex and Int cannot roll on an Astral Plate** (pure strength base). Use a hybrid base if you
  want them.

## Blind test: Hunter/Redeemer speed boots
Full write-up: [docs/blind-test-boots.md](docs/blind-test-boots.md). Recipes are in `recipes/boots/`.

| route | success | expected cost | cost per success | p90 | movement speed |
| --- | --- | --- | --- | --- | --- |
| **A: mine** (Critical reforge → guaranteed Elusive, bench MS) | 95.5% | **12.6 div** | **13.2 div** | 18.0 div | bench 20–24% |
| D: hybrid (A + Sanavixx's Veiled finish) | 91.0% | 19.0 div | 20.9 div | 24.4 div | Chosen 25–30% in 45% |
| Sanavixx method | 78.5% | 31.4 div | 40.0 div | 46.3 div | Chosen 25–30% in 75% |

(Re-run with bench locks on rares after the "beast metamod is magic-only" correction; the first
blind-test numbers in docs/blind-test-boots.md used the ~1 div beast lock.)

## Implemented mechanics
All rules are in [`poecraft/mechanics.py`](poecraft/mechanics.py), each with its source.
* Mod rolling: chance = weight / total weight of mods that can roll now. Respects ilvl, base and
  influence tags, open prefix/suffix slots and mod-group exclusion (RePoE spawn weights, first
  matching tag wins; generation-weight multipliers).
* Transmute, alteration, augment, regal, alchemy, chaos, exalt, annul, scour, fracture, essences,
  crafting bench (incl. metamods, one crafted mod unless multimod).
* Conqueror exalts; Orb of Dominance (tier upgrade, T1 → Elevated, skips fractured and
  metamod-protected mods); Awakener's Orb (one random influenced mod kept from each item,
  same-group conflict, elevated mods transfer, target keeps base/ilvl).
* Harvest reforge `<tag>` with lifeforce costs. Respects metamods and consumes them (in-game
  ground truth).
* Beastcrafts: metamod, imprint/restore, Hinekora's Lock + `foresee`, max links,
  "Add a Mod to a Hunter/Warlord/Redeemer Item", add prefix/remove suffix and the reverse.
  Each one costs its specific beasts.
* Veiled Exalted Orb + unveil (RePoE unveiled mods); Foulborn Exalted Orb (approximation).
* Omen of Blanching: stub (cost only).
* Not implemented: recombinators (out of scope), fossils, Eldritch implicits, socket colours,
  mod values, Harvest "more/less likely" reforges.

## UNVERIFIED rules (assumptions; please confirm in game)
1. Rare mod count when rerolling (chaos/alch/reforge/Awakener fill/essence): 4/5/6 mods with
   weights 8/3/1. This is the community figure; PoEDB does not confirm it.
2. Magic items roll 1 or 2 affixes with equal chance.
3. Harvest reforge with a locked side: kept mods count toward the 4–6 total, with at least one new
   mod, and the guaranteed tagged mod is always a new mod. Sanavixx says "0–2 extra prefixes";
   the model gives 1–2 with two kept suffixes and 0–2 with three.
4. Orb of Dominance: target and victim chosen uniformly. Tier upgrades below T1 ignore ilvl.
   It needs ≥2 *changeable* influenced mods.
5. Awakener's Orb: the random pick among several influenced mods is uniform. The result is
   filled to the usual 4–6 mods.
6. Chaos Orb keeps the side protected by a metamod.
7. Beastcraft "Add a crafted Meta-modifier": magic items only (in-game, user). UNVERIFIED: the
   player chooses which metamod; it is the same crafted mod as the bench one, so it takes an
   affix slot.
8. Beastcraft "Add a Mod to a <Conqueror> Item": needs that influence already and adds a random mod
   of it with conqueror-exalt weights.
9. "Add a Prefix, Remove a Random Suffix" (and the reverse): removes first, respects metamods.
10. Hinekora's Lock: a declined preview removes the lock, and a new lock gives a new independent
    preview.
11. Max-links beastcraft gives a 6-link on body armours (sockets are not simulated).
12. Fracturing Orb never picks crafted mods.
13. Veiled Exalted Orb: removal respects metamods; the veiled affix goes to a uniformly chosen open
    side. Unveil shows 3 offers drawn by weight. Foulborn Exalted Orb = "add a random mod like an
    Exalted Orb" (optionally restricted to one side).
14. Locked Harvest reforge does **not** add mods to the locked side (your rule). Sanavixx's boots
    method implies it does; switchable with `rule reforge_fills_locked_side on`.

### The four open questions from the brief
1. *Does Dominance skip metamod-protected mods?* **Yes.** PoEDB's Orb of Dominance page
   (community wiki text): "modifiers that can't be changed (e.g. suffixes on an item with the
   'Suffixes can't be changed' mod) will not be affected". Implemented.
2. *How does Awakener pick influenced mods?* PoEDB's Awakener's Orb page: "If an item has two
   or more signature mods, one will be selected at random". Uniform is still an assumption (#5).
3. *Does a beastcraft metamod take an affix slot?* The bench metamod does: RePoE lists
   "Suffixes Cannot Be Changed" as a **prefix**. Sanavixx's "make sure 1 open prefix" before the
   beastcraft lock suggests the beast version does too. The model assumes it (#7).
4. *Is the curse the only caster prefix on a Hunter/Warlord Astral Plate?* **Yes**, at ilvl 86
   in RePoE 3.30.0.2.2 data (tested in `tests/test_poecraft.py`), so the locked Caster reforge
   always gives it.

## Data and price sources (fetched 2026-11-18)
* **Game data**: RePoE export [repoe-fork/repoe-fork.github.io](https://github.com/repoe-fork/repoe-fork.github.io),
  commit `e802a20`, game version **3.30.0.2.2**, pulled via git. All equipment classes, jewels,
  flasks and tinctures (cluster jewels not yet). The older
  lvlvllvlvllvlvl/RePoE repo is deprecated and points there. Built into `data/gamedata.json`
  by `tools/build_data.py` (armour classes only).
* **Mechanics**: [PoEDB](https://poedb.tw/us/) pages Orb_of_Dominance, Awakeners_Orb,
  Hunters_Exalted_Orb, Horticrafting, Bestiary_Recipe, Hinekoras_Lock, Veiled_Exalted_Orb.
  These embed the community wiki text. **poewiki.net returned HTTP 403** from this environment,
  so PoE Wiki was read only through PoEDB's copies.
* **Prices** (from 2026-11-18 evening): poe.ninja's PoE1 API, fetched by `python tools/fetch_prices.py`
  or the web app's Prices tab. Currency, omens, essences and fossils come from the exchange
  endpoint; beasts and base types (with item level and influence) from the stash endpoint. The
  stash *currency* endpoint is not used (it said Divine 127.5c while the exchange said 226.2c).
  Earlier prices came from [outof.games] (poe.ninja data shown on a web page).
* **Prices (before)**: [outof.games](https://outof.games/realms/pathofexile/economy/currency/) (poe.ninja
  data) for currency, beasts, omens, essences and base types, **Covens** league. The
  poe.ninja API itself returned 404 on the old endpoint. Saved in `data/prices.json` with the
  date. Estimated entries (influenced bases, Omen of Blanching, "any rare beast") are listed
  under `estimates` in that file. **Check them.** The prices you gave are in
  `data/prices_user_mid_nov_2026.json`.
* **Strategy**: [Sanavixx crafting guides](https://sanavixx.com/crafting/), Endgame Chest section.
* **Language inspiration**: [Kalandralang](https://github.com/doomeer/kalandralang), MIT licence.
  Syntax ideas only, no code reused.
* Craft of Exile was not used (see [docs/known-discrepancies.md](docs/known-discrepancies.md)).

## Layout
```
poecraft/   data.py (mods, names, pools)  item.py  mechanics.py (ALL rules)  lang.py (parser)
            engine.py (MC + exact)  analysis.py (stats, compare)  cli.py  prices.py
recipes/    *.craft, lib/*.craft (sub-recipes), tests/*.craft (small exact cases)
data/       gamedata.json  prices.json  aliases.json  prices_user_mid_nov_2026.json
docs/       language.md  known-discrepancies.md  blind-test-boots.md  for-claude.md
tools/      build_data.py
```

## How the exact engine works
Primitive actions are enumerated exhaustively. Every random choice is branched with its
probability, and outcomes are merged by item state. Loops become absorbing Markov chains:
transient states are found by running the body, then `(I − Q)X = R` gives the absorption
probabilities, the expected count of every currency, and the second moment of the chaos cost.
Two reductions keep this tractable:
* **i.i.d. loops**: `until X { alt }` or `{ chaos }`, where each attempt rerolls everything,
  are solved in closed form as a geometric number of trials. Monte Carlo uses this too.
* **lumping**: retry states whose body starts with the same deterministic reset (a scour or
  a rebuy) are merged.

If enumeration exceeds its budget (paths, states or 90 s), `auto` mode falls back to Monte Carlo
and says so.
