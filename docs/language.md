# Craft recipe language reference

Recipes are plain-text `.craft` files. The design is inspired by
[Kalandralang](https://github.com/doomeer/kalandralang) (MIT licence; only syntax ideas were
borrowed, no code). It changes these things:

* **Readable mod names** (`has "Hunter: additional curse" tier <= 1`) instead of RePoE ids.
* **Current mechanics**: Dominance/elevated mods, Awakener, beastcrafts (metamod, Hinekora's
  Lock preview, add-influenced-mod, six-link), Harvest reforges that respect metamods.
* **Sub-recipes** with parameters (`def elevate_hunter(mod) { ... }`) and `include` files.
* **Goals and tracked outcomes** (`goal`, `track`) so results report success probability.
* **`compare`** command ranking several recipes on cost, risk and variance.
* Errors carry `file:line`, say what is wrong and usually suggest a fix.

## Lexical rules

* `#` starts a comment until the end of the line.
* Strings use double quotes: `"Astral Plate"`. Numbers: `86`, `2.5`.
* Blocks use braces `{ ... }`. Newlines are not significant.
* Statements are keywords; sub-recipe calls need parentheses: `warlord_piece()`.

## State

While a recipe runs there is a **current item**, a set of **set-aside items** (by name), one
**imprint**, and the **costs** spent so far. Every random event is drawn with the
rules in `poecraft/mechanics.py`.

## Statements

### Buying
```
buy "<base>" [normal|magic|rare] [ilvl N] [influence <i>[, <i>]]
    [with [fractured] "<mod>" [tier N | elevated]]...
    [for <amount> <currency> | for price "<price key>"]
```
* Only the listed mods are put on the item. Without `tier`, the best tier the ilvl allows.
* Influences: `shaper elder crusader redeemer hunter warlord`.
* `for 2 divine`, `for 50 chaos`, or `for price "base:astral_plate_warlord_86"` (looked up in the
  price file). The price counts as the item's value for the "bases lost" risk metric.
* Buying again while holding a bought item counts that item as *lost*.

### Currency
| statement | effect |
| --- | --- |
| `transmute` `alt` `aug` `regal` `alch` `chaos` `exalt` `annul` `scour` | basic orbs |
| `dominance` | Orb of Dominance |
| `hunter_exalt` `warlord_exalt` `redeemer_exalt` `crusader_exalt` | conqueror exalts |
| `awaken <name>` | Awakener's Orb: destroy set-aside item `<name>`, current item is the target |
| `fracture` | Fracturing Orb |
| `essence "<name>"` | e.g. `essence "Deafening Essence of Greed"` |
| `veiled_exalt` | Veiled Exalted Orb: removes a random (unprotected) mod, adds a veiled affix |
| `unveil "<mod>"[, "<mod>"...]` | unveil: 3 offers by weight; takes the first listed mod that is offered |
| `foulborn_exalt` | UNVERIFIED stand-in: adds a random suffix |

### Crafting bench
```
craft lock_suffixes     # "Suffixes Cannot Be Changed" (a crafted PREFIX), 4 div + 4 blessed in 3.30
craft lock_prefixes     # "Prefixes Cannot Be Changed" (a crafted SUFFIX)
craft multimod          # "Can have up to 3 Crafted Modifiers"
craft "to maximum Life" # any bench mod by text (best tier)
remove_crafted
```

### Beastcrafts
```
beastcraft lock_suffixes | lock_prefixes | multimod   # "Add a crafted Meta-modifier" (Wild Bristle Matron)
beastcraft imprint                                     # magic items only
beastcraft restore_imprint
beastcraft hinekoras_lock                              # magic items only; enables `foresee`
beastcraft max_links
beastcraft add_mod hunter|warlord|redeemer             # "Add a Mod to a <X> Item"
beastcraft add_prefix_remove_suffix | add_suffix_remove_prefix
```
Each beastcraft costs the specific captured beasts (see `poecraft/mechanics.py:BEASTCRAFTS`).

### Harvest
```
harvest reforge caster   # also: attack life defences chaos fire cold lightning physical speed critical
                         #       minion elemental attribute mana
```
Respects `Prefixes/Suffixes Cannot Be Changed` and consumes the metamod (in-game testing).

### Other
```
omen blanching          # stub: cost only
pay 3 divine            # any extra cost
set_aside <name>        # park the current item under a name (then buy/take another)
take <name>             # make a set-aside item current again (current slot must be empty)
show "label"            # print the item in `poecraft sample`
fail "reason"           # stop: this run is a failure
succeed                 # stop: this run is finished
```

### Control flow
```
if <cond> { ... } else if <cond> { ... } else { ... }
repeat [limit N] { ... } until <cond>     # body runs at least once
repeat [limit N] { ... } while <cond>
until <cond> [limit N] { ... }            # checks first; body may run 0 times
while <cond> [limit N] { ... }
foresee <action> accept if <cond> [otherwise { ... }]
```
* `limit N` stops after N iterations and simply continues after the loop. Whether the run
  succeeded is decided by `goal`.
* `foresee` needs Hinekora's Lock on the current item. The action's outcome is drawn. If
  `<cond>` holds for that outcome it is applied and paid. Otherwise nothing is spent, the lock is
  removed and the `otherwise` block runs.

### Sub-recipes and includes
```
include "lib/elevate.craft"          # path relative to this file; may only contain defs

def elevate_hunter(mod) {            # parameters are strings, used where a mod name goes
    repeat { ... } until has mod elevated and influenced_count == 1
}
elevate_hunter("Hunter: attack crit")
```
Calls are expanded inline, and recursion is rejected.

### Rule switches (top level)
```
rule reforge_fills_locked_side on    # see docs/known-discrepancies.md #5
```

### Goal and tracked outcomes (top level)
```
goal has "Warlord: mana cost" elevated and has "Hunter: additional curse"
track "life prefix" has "flat life" tier <= 2
```
`goal` decides success. Without it a run succeeds unless it hit `fail`. `track` reports the
probability of an extra condition at the end of a run.

## Conditions
```
has "<mod>"                    # any tier
has "<mod>" tier <= 2          # tier 1 = best; elevated counts as tier 0
has "<mod>" elevated
is magic | is rare | is normal
influenced hunter
prefix_count, suffix_count, mod_count, open_prefix, open_suffix,
influenced_count, elevated_count, crafted_count, links       # numbers; compare with < <= = == != >= >
tier "<mod>"                   # number (99 if absent)
count(hunter)                  # number of Hunter mods
not ..., ... and ..., ... or ..., ( ... )
```
A bare metric means "> 0", e.g. `open_prefix`.

## Mod names
A mod reference resolves to a *family* (all tiers of one mod for one influence):
1. an alias from `data/aliases.json` (e.g. `"Hunter: attack crit"`, `"flat life"`);
2. a RePoE mod id (`"AdditionalCurseOnEnemiesInfluence1"`);
3. `"<Influence>: <text>"`, e.g. `"Warlord: total mana cost"`;
4. `"prefix: <text>"` / `"suffix: <text>"`;
5. any fragment of the in-game text, e.g. `"to maximum Life"`.

If the name is ambiguous the error lists every match. To see which mods can roll on a base, and
their weights, run `poecraft mods "<base>" --influence hunter,warlord --search "<text>"`.

## Semantics of results
* **success probability**: share of runs where `goal` holds (and no `fail`).
* **expected cost**: everything spent until the recipe stops, in chaos and divines.
* **cost per success** = expected cost / success probability. This is the expected cost if you
  restart from scratch after a failure.
* **bases lost**: expected value of bought bases thrown away (rebuys, final failure).
  **P(lose base)** is the share of runs that threw one away (Monte Carlo only).
* **percentiles** (Monte Carlo only): p50/p90/p99 of total cost.
