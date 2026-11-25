# Known discrepancies between in-game behaviour, tools and guides

Each entry names its source. The simulator follows the "in-game" column.

| # | Topic | In-game (ground truth) | Tool / guide that differs | Source |
| --- | --- | --- | --- | --- |
| 1 | Harvest reforge + "Prefixes/Suffixes Cannot Be Changed" | The reforge **respects** the metamod: the protected side stays, only the other side is rerolled. | Craft of Exile treats the reforge as breaking the metamod and rerolling the whole item. **Do not use Craft of Exile to check any step that combines reforges and metamods.** | in-game testing (user), Nov 2026 |
| 2 | Is the metamod kept after a Harvest reforge? | No. The metamod is **consumed**, so every retry costs one reforge plus one new lock. | Guides often say only "reforge until happy" and leave out the per-retry lock cost. | in-game testing (user); also implied by Sanavixx "Endgame Chest" step 7 ("repeat step 6") |
| 3 | Cost of the "Suffixes Cannot Be Changed" bench craft | RePoE 3.30.0.2.2 bench data: **4 Divine + 4 Blessed** for armour (`DexMasterItemGenerationCannotChangeSuffixesB`). New "1/2 Suffix Modifier(s) cannot be changed" crafts cost 1/2 Divine. | Older guides and tools quote 2 Divine. | RePoE crafting_bench_options.json (repoe-fork, 2026-11-18) |
| 4 | Kalandralang | Outdated for current mechanics: no elevated-mod beastcrafts, old Harvest reforge rules, recombinators. | — | github.com/doomeer/kalandralang README (read 2026-11-18) |
| 7 | Where the metamod beastcraft ("Add a crafted Meta-modifier", Wild Bristle Matron) can be used | **Magic items only**, so a rare needs the bench craft (4 div + 4 blessed in 3.30) | Sanavixx "Endgame Chest" step 6 ("Use Beastcraft Multimod to apply Suffixes Cannot Be Changed onto chest", a rare after the Awakener) and the boots guide (steps 6–7) use it on rares; PoEDB's recipe text says "non-Unique Item" | in-game (user), 2026-11-18 |
| 5 | Can a locked Harvest reforge add NEW mods to open slots on the locked side? | User ground truth: "the protected side is left untouched" (simulator default: no). | Sanavixx "Endgame Boots (Speed Variation)" step 3 (suffixes locked + reforge Physical to gain a Hunter mod; all physical mods on Hunter boots are suffixes) and step 6 (fill suffixes before the locked reforge) only make sense if **yes**. Switch: `rule reforge_fills_locked_side on`. **Resolved 2026-11-18: owner confirmed YES in game; default switched on.** | sanavixx.com/crafting (read 2026-11-18) vs in-game testing (user) |
| 6 | Bench "Suffixes Cannot Be Changed" in guides | 3.30 cost is 4 div + 4 blessed (see #3); the beastcraft metamod (Wild Bristle Matron, ~1 div) is cheaper | Sanavixx boots step 2 uses the bench craft | RePoE 3.30 + outof.games prices 2026-11-18 |

| 8 | Partial locks "1/2 Suffix (Prefix) Modifier(s) cannot be changed" (1 / 2 divine) | Can always be crafted; protect exactly N suffixes; take a slot on the other side like the full lock; consumed by a reforge. Open: which N are protected when the item has more | ? (not modelled by other tools we checked) | in-game (owner), 2026-11-18 |
| 9 | Does "increased bonuses gained from Equipped Boots" (Wickerman, e.g. Greater Smouldering Leg 12%) scale Replica Stampede's "Your Movement Speed is 165% of its base value"? | **Unknown, needs an in-game check** (character sheet movement speed with the Wickerman leg attached vs detached) | Path of Building 2.69.0 does **not**: CalcSetup scales boot mods with `MergeMod(mod, true)`, which keeps only BASE/INC/MORE and drops the OVERRIDE mod this line parses to, so PoB shows 165% either way. If the game scales the value it would be 165 x 1.12 = 184.8%. | PoB source (Modules/CalcSetup.lua, Classes/ModList.lua), checked 2026-11-25 with pob_bridge/inspect_boots_ms.lua on SanaviCovens |

## Open questions (not yet confirmed in game)
Also listed as UNVERIFIED in README and in code comments:
* How many new mods a reforge with a locked side rolls. The model gives 1–2 extra prefixes
  with 2 kept suffixes and 0–2 with 3 kept suffixes; Sanavixx says "0–2 extra prefixes".
* Whether a second Hinekora's Lock gives a *new* preview of the same action.

## Craft of Exile and locked reforges (2026-11-18)
The owner's Craft of Exile run of a suffix-locked reforge gave 17/41.5/37.5% for 1/2/3 prefixes.
That is what an UNLOCKED whole-item reforge gives in our model (22/46/31% with a Life tag,
13/45/42% with Defence), while a real locked reforge with 3 kept suffixes gives 67/25/8% under
the chaos-style mod-count model. It is the known CoE bug (#1): CoE breaks the lock and rerolls
everything. Do not use CoE numbers for locked reforges.

## Consequences found 2026-11-18
* With the locked side able to fill (Q2), the Caster reforge on the Endgame Chest guarantees the
  curse only when all 3 suffixes are full: with 2 suffixes the guaranteed caster mod is the Hunter
  spell-crit suffix about 2/3 of the time. `recipes/lib/endgame_chest_parts.craft` has
  `curse_reforge_safe` (fill the 3rd suffix with an Exalted Orb, then one guaranteed retry).

## Craft of Exile ignores metamods on Chaos Orbs too (2026-11-18)
On beta.craftofexile.com (3.29 data) an imported Royal Plate with three T1 resistance suffixes and
"Suffix Modifiers cannot be changed" lost all three suffixes to a single Chaos Orb. PoE Wiki
(Meta-crafting_mod) and the owner's in-game test both say locked mods survive rerolls. So CoE
cannot be used for any locked reroll, Harvest or Chaos.
