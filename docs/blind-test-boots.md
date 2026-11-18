# Blind test: Hunter + Redeemer speed boots

Target: rare boots (any base, ilvl 75+) with Hunter + Redeemer influence and
* Elevated **Tailwind** (Hunter suffix: "You have Tailwind if you have dealt a Critical Strike
  Recently" / elevated adds "(10–25)% increased Effect of Tailwind on you"),
* Elevated **Onslaught** (Redeemer suffix: "(8–10)% chance to gain Onslaught for 4 seconds on Kill" /
  elevated adds "(3–10)% increased Attack, Cast and Movement Speed while you have Onslaught"),
* **Elusive** (Redeemer prefix: "(5–10)% chance to gain Elusive on Critical Strike"),
* **Movement speed** (prefix).

Part 1 (below, up to "Sanavixx comparison") was written **before** reading the
"Endgame Speed Boots" section on sanavixx.com. The section was never opened before this commit;
the only part of that page ever extracted was the "Endgame Chest" section.

## Facts from the data (RePoE 3.30.0.2.2, `poecraft mods "Two-Toned Boots" --influence hunter,redeemer`)
* Tailwind has a single tier (ilvl 75, weight 100). The first Dominance upgrade makes it Elevated.
* Onslaught has T2 (ilvl 75) and T1 (ilvl 80). T1 must be on the item before Dominance can make
  it Elevated, so the Redeemer base needs **ilvl 80+**.
* **Elusive is the only `critical`-tagged prefix** on Hunter/Redeemer boots. With suffixes
  locked, *Harvest reforge Critical* (150 Primal lifeforce ≈ 3c) is therefore **guaranteed** to
  roll Elusive. This is the same trick as Caster → "additional Curse" on the chest.
* Movement speed is the only `speed`-tagged prefix, so *reforge Speed* guarantees it.
* The bench crafts movement speed (20–24%) for 4 chaos as a prefix.
* "Suffixes Cannot Be Changed" is a crafted *prefix*. Every locked reforge needs an open prefix
  and a new metamod (Wild Bristle Matron beastcraft ≈ 215c, cheaper than the 3.30 bench craft at
  4 div + 4 blessed).

## Candidate routes (all share the two elevated suffixes)
Common part (`recipes/lib/boots_parts.craft`):
1. Hunter boots (normal, ilvl 84): transmute → alt until Tailwind → regal → beastcraft
   "Add a Mod to a Hunter Item" (≈2–3c) if there is no second Hunter mod → Dominance until
   Tailwind is Elevated (≈50% per orb; on failure scour and start again).
2. Redeemer boots (normal, ilvl 84): same with T1 Onslaught and "Add a Mod to a Redeemer Item".
3. Awakener's Orb (target: Redeemer piece). If the result has no open prefix, use
   "Add a Suffix, Remove a Random Prefix", or an Annulment as a last resort.

| route | prefix step | file |
| --- | --- | --- |
| **A** | lock suffixes + reforge **Critical** (Elusive guaranteed). Bench-craft movement speed into the open prefix. If the reforge filled every prefix without movement speed, re-lock and reforge. | `recipes/boots/route_a_crit_reforge_bench_ms.craft` |
| B | lock + reforge **Speed** (movement speed guaranteed), then "Add a Mod to a Redeemer Item" hoping for Elusive. On a miss, re-lock and re-reforge. | `recipes/boots/route_b_speed_reforge_add_elusive.craft` |
| C | like A but insists on a **natural** movement speed: re-lock + reforge Critical until movement speed rolls. | `recipes/boots/route_c_crit_reforge_natural_ms.craft` |

## Results (`poecraft compare recipes/boots/*.craft --runs 3000`, prices 2026-11-18, divine = 203c)
| route | success | expected cost | cost per success | std | p90 | notes |
| --- | --- | --- | --- | --- | --- | --- |
| **A crit reforge + bench MS** | **95.2%** | **9.30 div** | **9.77 div** | 3.7 div | 14.2 div | natural MS anyway in 8%; MS ≥30% in 1.5% |
| B speed reforge + add Elusive | 45.9% | 14.0 div | 30.5 div | 6.5 div | 22.7 div | Elusive is ~9% of Redeemer prefix weight; each miss costs a Matron |
| C crit reforge, natural MS only | 68.6% | 22.1 div | 32.2 div | 14.1 div | 40.9 div | each retry costs a Matron, and full prefixes force risky annuls |

**Pick: Route A, ≈ 9.3 div expected (≈ 9.8 div per success).** Where the money goes: ~4
Dominance (6.4 div), one Awakener (0.7 div), one Wild Bristle Matron (1.1 div), ~1,600
alterations (1 div). The bases (≈15c with the estimated base prices) and all the other
beastcrafts are negligible.

Risks: 4.8% of runs fail. In every one of them, the Awakener result or a reforge left 3 prefixes
**and** 3 suffixes, and the Annulment used to make room hit an elevated suffix. Hinekora's preview
before the Awakener, or simply accepting the item, would avoid most of these.

Mechanics this needed (all in `poecraft/mechanics.py`): Harvest reforge **Critical** and
**Speed** (PoEDB Horticrafting costs), beastcraft "Add a Mod to a Redeemer/Hunter Item" (PoEDB
Bestiary_Recipe; behaviour UNVERIFIED), "Add a Suffix, Remove a Random Prefix" (UNVERIFIED
order), and the crafted-vs-natural mod distinction in `has`. **Veiled Exalted Orb was not
needed.** Its current text (PoEDB: "Removes a random modifier and adds a random Veiled
modifier to a rare item") makes it a poor fit here, and it is still unimplemented.

Same trick found for the chest: the Awakener rerolls every non-kept mod, so the Warlord piece's
"no other suffixes + lock + scour → magic" step only pays off with Hinekora's preview.
Dropping it (`recipes/endgame_chest_lean.craft`) cuts the chest from 12.7 to **11.0 div**.

## Sanavixx comparison (written after reading their "Endgame Boots (Speed Variation)" section)
Their method, as a recipe: `recipes/boots/sanavixx_endgame_speed_boots.craft`.
1. Hunter boots: alt/aug until a magic item with only Tailwind (annul a prefix if one rolls).
2. **Bench** "Suffixes Cannot Be Changed", then a beastcraft **Imprint**.
3. Regal, Harvest reforge **Physical** to gain a 2nd Hunter mod, then Dominance. If Tailwind is
   lost: restore the imprint, re-imprint, repeat.
4. Redeemer boots: **spam Harvest reforge Critical** (guaranteed Elusive = the 2nd influenced
   mod) until T1 Onslaught, then Dominance.
5. Awaken the Redeemer boots onto the Hunter boots, yolo annul if prefixes are full.
6. Fill the suffixes (bench "Prefixes Cannot Be Changed" + Foulborn Exalted Orb), beastcraft
   suffix lock.
7. Reforge Critical for Elusive, yolo annul if prefixes are full, beastcraft suffix lock.
8. **Veiled Exalted Orb**: with suffixes locked it removes the metamod or Elusive. If the metamod
   goes, **unveil a "Chosen" movement speed** (25–30% MS + a bonus line); if Elusive goes, redo 7.

Same prices, 3000 runs each:

| route | success | expected cost | cost per success | p90 | movement speed you get |
| --- | --- | --- | --- | --- | --- |
| **A (mine, blind)** | 95.2% | **9.3 div** | **9.8 div** | 14.2 div | bench 20–24% (natural roll in 8%) |
| D hybrid (A + their Veiled finish) | 91.5% | 13.1 div | 14.4 div | 18.1 div | Chosen 25–30% in 45.5%, otherwise bench |
| **Sanavixx** | 77.4% | 24.0 div | 31.0 div | 35.5 div | Chosen 25–30% in 75% of runs |
| B (mine) | 45.9% | 14.0 div | 30.5 div | 22.7 div | natural |
| C (mine) | 68.6% | 22.1 div | 32.2 div | 40.9 div | natural |

**Honest verdict.**
* For the item as specified (the four mods), **Route A is about 2.5× cheaper** per success and
  much safer. Sanavixx's route is more expensive mainly at current 3.30 prices:
  - the bench "Suffixes Cannot Be Changed" in step 2 is 4 div + 4 blessed in 3.30 (~4.7 div);
    a Wild Bristle Matron beastcraft (~1 div) does the same job;
  - the Imprint safety net costs a Craicic Croaker (~1 div) per Dominance attempt, which is more
    than redoing the alt spam (~0.5 div);
  - Veiled Exalted Orbs (~3 div each, ~1.5 used) and Foulborn exalts;
  - the "yolo annuls" without a lock cost 23% of runs an elevated mod.
* **Their item is better**: in 75% of runs it has a Chosen 25–30% MS with a bonus line, where
  mine has a 20–24% bench MS. If that matters, the hybrid **Route D** gets the Chosen MS 45% of
  the time for 13 div. It is cheaper than theirs because it keeps the cheap pieces (alt + "Add a
  Mod to a X Item" beastcraft) and the beastcraft lock. It doesn't fill suffixes first (their
  Foulborn step), so the veiled mod is sometimes a suffix.
* **Tricks they had that I missed**: (1) Veiled Exalt with locked suffixes as a *targeted prefix
  removal + premium MS unveil*. (2) Filling the suffixes first so the veiled mod must be a prefix.
  (3) Using the guaranteed-Elusive Critical reforge to *create the second influenced mod* on the
  Redeemer piece (I only used it at the end). It costs ~1.5 div of lifeforce at 2.3% T1
  Onslaught per reforge, roughly the same as my alt spam, and it needs no extra beastcraft.
* **Tricks I had that they missed (at current prices)**: beastcraft metamod instead of the 3.30
  bench lock; the 2–3c "Add a Mod to a <Conqueror> Item" beastcraft instead of a reforge for
  the second influenced mod; a 4c bench MS when a cheaper item is acceptable.
* **Mechanics question raised.** Their step 3 (suffixes locked, reforge Physical "to get the
  second Hunter mod") only works if a locked reforge can **add new mods into open slots on the
  locked side**: every physical-tagged mod on Hunter boots is a Hunter *suffix*. Step 6 (fill
  suffixes before the locked reforge) points the same way. Your ground-truth rule says the
  protected side is "left untouched". I kept that as the default and added a switch
  (`rule reforge_fills_locked_side on`), which their recipe turns on. **Please test in game**:
  on a rare with 1 suffix + suffix lock, does a reforge ever add a suffix?
  (docs/known-discrepancies.md #5)

### Mechanics added for this comparison
* Veiled Exalted Orb and unveil (RePoE `unveiled` mods; 3 offers by weight). UNVERIFIED:
  removal respects metamods; the side is uniform among open sides; offer count and weights.
* Foulborn Exalted Orb: modelled as "add a random suffix" (UNVERIFIED; not on the PoEDB pages
  fetched; price 640c from outof.games).
* Harvest reforge Physical, beastcraft Imprint/restore (already present), `rule` switches.
