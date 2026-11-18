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

## Sanavixx comparison
(to be added after reading the "Endgame Speed Boots" section; see below)
