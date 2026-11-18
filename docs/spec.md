# poecraft web app — specification (draft 1, 2026-11-18)

A personal, local, all-in-one tool to **design an item, test it in Path of Building, and work
out the cheapest way to craft it**. It runs on the owner's PC only. Hosting for others is out of
scope for now.

## 1. Principles
1. **Correct before clever.** Every crafting rule lives in one place and cites a source (PoEDB,
   PoE Wiki, game data, or "in-game testing (owner)"). Anything unverified is flagged in the UI
   next to every result that depends on it (§4.I).
2. **Ask, don't guess.** When a mechanic is uncertain, it goes on the open-questions list (§7)
   for the owner to test in game or in Craft of Exile. Craft of Exile is wrong about reforges
   combined with metamods, so those cases are tested in game only.
3. **Same features everywhere.** Every web action has a JSON API call and a CLI command, so
   Claude can drive the tool the same way the UI does.
4. **Never damage PoB data.** PoB is only read, except for the opt-in "Import into PoB" (§4.F),
   which always writes a backup first.
5. **Legal items only.** The item builder and every mechanic refuse to produce an item the game
   could not produce.

## 2. Scope
- **All item classes from the start**, using RePoE's full data: armour, weapons, jewellery,
  quivers, shields, flasks and jewels. Class-specific systems are listed in §5. Where one isn't
  implemented, the class is still buildable and testable but its crafting actions are greyed out
  with the reason shown.
- **Local only:** a Rust server on localhost serves the web UI and talks to the local PoB.
- **Out of scope:** recombinators (nerfed), multi-user hosting, and accounts.

## 3. Architecture
```
browser UI (static HTML/JS, served locally)
        |  JSON over HTTP (localhost)
Rust server (axum)
  ├─ core crates: data · item · mechanics · engine (Monte Carlo + exact) · lang (recipes)
  │               planner (reverse engineering) · pricing · trade (search links) · pobitem (item text)
  ├─ CLI (same crates)        └─ optional Python bindings (pyo3) for scripting
  └─ PoB worker pool (N processes, 1 per core) ── pob-headless harness ── installed PoB (lua51.dll)
```
- **The Rust core replaces the current Python engine.** The Python version stays as the
  reference until the Rust results match it: a cross-check runs every recipe in both and
  compares within Monte Carlo error, and the exact engine gives precise targets.
- **WebAssembly is not planned.** All computation runs on the server. Revisit only if it becomes
  essentially free.
- **PoB workers reuse `pob-headless`.** Each worker loads a build once and then scores items.
  There are two scoring paths:
  - the tooltip fast path, ~24 ms per item, used for screening;
  - the full recalculation, ~205 ms per item, used to confirm results.
  Results are cached on disk, keyed by build file hash, PoB version, config preset, slot and the
  item's relevant mods. Numbers from the two paths are never mixed in one comparison (they
  differ by ~2–3% DPS).
- **Data:** RePoE (pinned commit and game version), prices (fetched plus overrides), the rules
  file, and the PoB trade stat-ID map (§4.H).

## 4. Features
Phase numbers refer to §6.

### A. Item builder (phase 2)
- Choose the item class, base, item level, influences (incl. Eldritch implicits later),
  quality, sockets/links/colours and fractured mods.
- Add mods from the legal pool only. The builder enforces item level, tags, influence, mod
  groups, prefix/suffix limits, crafted-mod limits (with multimod) and elevated mods.
- Each mod has a tier and a value roll (needed for PoB and the trade site).
- "Paste item" parses in-game or PoB item text into the builder.
- **Acceptance:** for 20 real items pasted from the game, the parse and re-export round-trip
  unchanged, and illegal edits are refused with the reason.

### B. Mod pool viewer and trick detection (phase 2)
- For the current item: every mod that can roll, with its weight, percentage, tier, tags and
  group, split by prefix/suffix and by what currently blocks it.
- **Trick hints**, computed from the pool rather than hard-coded:
  - "Elusive is the only `critical` prefix → a locked Critical reforge guarantees it";
  - "this mod is also a bench craft";
  - "this mod has an elevated version";
  - "a Veiled/unveil version exists".
- **Acceptance:** it reproduces the Caster→curse (chest) and Critical→Elusive (boots) findings.

### C. Simulator and strategy comparison (phase 1 core, phase 3 UI)
- Run a recipe with the exact engine where it's small enough, otherwise Monte Carlo, and
  cross-check when both run.
- Output: success %, expected cost, cost per success, standard deviation, p50/p90/p99, risk of
  losing the base, tracked outcomes, and currency used.
- **Comparison view:** several recipes against the same prices and rules, ranked by expected
  cost, risk or spread, with cost-distribution charts.
- **Acceptance:** the Rust engine matches the Python results for every recipe in `recipes/`.

### D. Reverse engineering (phase 5, then ongoing)
- **Input:** a target spec built in the item builder. Each mod is marked:
  - **vital** — must be on the item, with a minimum tier or elevated;
  - **important** — scored by its probability, with a weight;
  - **bonus** — anything else useful, valued at the end.
  Chest example: the three influenced mods are vital, the third-suffix resistance is important,
  good prefixes are bonus.
- **Output:** the cheapest few routes as editable recipes, each with expected cost to reach all
  vital mods, P(each important mod), expected bonus value, and risk. If the target can't be
  made, it says so and shows the closest achievable item.
- **Method (grows over time):**
  1. Single-step ranking: for each needed mod, the cheapest method by exact cost (alts, chaos,
     essences, every reforge tag, locked reforges, "add influenced mod" beastcraft, bench).
  2. Trick detection from §4.B.
  3. Pattern templates: elevate on separate pieces then merge; lock a side and reforge; fill
     with bench, Foulborn or Veiled; partial locks (§5).
  4. Assemble and order the steps, then run the candidates through the simulator and comparison.
  5. For small sub-problems, the exact engine computes the best stopping and retry policy
     ("reroll again or stop here?").
- **Acceptance:** for the chest and the boots, the planner's best route costs no more than the
  best hand-written recipe.

### E. Test in PoB (phase 4)
- Choose one or more builds (from the PoB Builds folder or a pobb.in link), the slot (with a
  ring 1/2 choice), and a config preset. The default preset is the owner's standard scoring:
  Pinnacle boss, resists assumed capped, flasks up and down, no cooldown skills in EHP.
- **Output per build:** DPS, EHP and max hit, each as the change against the currently equipped
  item, with flasks up and down. Multi-skill builds report the main skill and FullDPS.
- **Value per divine:** for simulated outcomes, the expected stat gain per divine spent, and a
  `sell` or stop condition can be based on build value. Scoring is limited by distinct items,
  irrelevant-mod collapsing, a mod-contribution estimate and a call budget.
- **Acceptance:** for the owner's current chest, the scores match the full-path harness exactly.

### F. Copy item / Import into PoB (phase 4)
- **Copy item:** puts the item in PoB/game item-text format on the clipboard.
- **Import into PoB:** adds the item to the chosen builds' item lists, **not equipped**.
  - Writes a timestamped backup first.
  - Warns if PoB may have the build open, since PoB overwrites the file on save.
  - Opt-in, with a confirmation.
- **Acceptance:** the imported item appears in PoB's item list, and the build otherwise loads
  with identical stats.

### G. Prices (phase 3)
- League selection; a fetch button (poe.ninja API, falling back to outof.games) for currency,
  beasts, omens, essences, lifeforce and base types. Requests identify the client only as `(ej)`.
- Per-key overrides kept in a local file. Estimated prices are flagged everywhere they're used,
  and every price shows its fetch date.

### H. Profit / sell mode and trade links (phase 3)
- **"Check price":** builds a pathofexile.com trade search for the item or spec and opens it in
  the browser. The owner reads the listings and types an expected sell price, which feeds the
  `sell` statement (profit, margin, P(loss)).
- **"Buy vs craft":** for a pasted listing, its crafting cost against the asking price.
- **How the link is built:** the trade site accepts a JSON query in the URL
  (`/trade/search/<League>?q=<json>`). Stat filters need trade stat IDs, which come from
  `https://www.pathofexile.com/api/trade/data/stats` (fetched once per league and mapped to our
  mods by text). Resistances use the site's pseudo stats.
  - **To verify:** that the `?q=` form still works.
  - **To verify:** that the text-to-ID mapping covers elevated and hybrid mods.
  - **Expected difficulty:** moderate. The mapping is the work; the link itself is easy.

### I. Rules panel and confirmation workflow (phase 1)
- Every rule has an id, a description, a source and a status: **verified (source)**,
  **owner-confirmed**, **unverified**, **disputed** (sources disagree). Switches like
  `reforge_fills_locked_side` are toggles.
- Every result lists the unverified or disputed rules it depends on.
- Each open question (§7) carries a short in-game or Craft of Exile test. The owner marks it
  confirmed or refuted, and that updates the rule.

### J. Recipe editor (phase 3)
- Text editor for the recipe language with inline errors (file:line plus a suggestion), and
  buttons for run, compare and sample.

### K. Base advice (phase 5)
- Which bases are interchangeable for the odds ("any Hunter body armour, ilvl 84+"), what the
  kept item's base and quality contribute (armour, implicit), and the cheapest qualifying base
  from the price data.

### L. Emulator (last phase)
- Manual crafting in a **workspace** of several items: set-aside items, imprint, Hinekora
  preview. Every crafting action is available.
- Undo/redo over the whole workspace; a seeded RNG.
- Odds panel: the outcome distribution of each action before you click.
- "Save session as recipe", which can then be simulated.

## 5. Mechanics coverage
Already in the Python engine: basic currency, conqueror exalts, Dominance, Awakener, Harvest
reforges (respecting and consuming metamods), essences, bench including metamods, beastcrafts
(metamod on magic only, imprint, Hinekora + foresee, max links, add influenced mod, swap side),
Veiled Exalted + unveil, Foulborn (approximation), Fracturing.

To add:
- **Partial locks:** "1 Suffix Modifier cannot be changed" (1 div) and "2 Suffix Modifiers
  cannot be changed" (2 div), plus the prefix versions. These are cheaper than the full lock
  when the item has fewer than 3 suffixes, and can enable gambles. Exact behaviour is open
  question Q1.
- **Mod values:** rolls, Divine Orb, Blessed Orb and Sacred Orb.
- **Sockets, links, colours:** Jeweller's, Fusing, Chromatic, Omen of Blanching and other omens.
- **Fossils and resonators; catalysts** (jewellery quality).
- **Eldritch implicits:** embers, ichors, Eldritch Chaos/Exalted/Annulment.
- **Other systems:** synthesised items, cluster jewels, flasks and tinctures, Harvest
  "more/less likely" reforges, Aisling (veiled mods), and any other current-league currency.

## 6. Phases
1. **Rust core** with data for all classes, the rules registry (§4.I), the Monte Carlo and exact
   engines, and recipe parsing. Acceptance: matches the Python results on every recipe; partial
   locks added.
2. **Item builder, mod pool viewer, mod values, item text in and out**, plus the minimal server
   and UI shell.
3. **Simulator UI, comparison view, recipe editor, prices, trade links and profit mode.**
4. **PoB testing, value per divine, Copy item, Import into PoB.**
5. **Reverse engineering v1 and base advice.**
6. **Emulator.**
7. **Mechanics backlog from §5**, interleaved as needed.

## 7. Open questions for the owner
These are areas where I'm not fully confident. Each needs an in-game test; Craft of Exile works
except where it's known to be wrong.

| # | Question | Why it matters |
|---|---|---|
| Q1 | **Partial suffix locks** ("1/2 Suffix Modifier(s) cannot be changed"): can they only be crafted when the item has ≤ N suffixes, or anytime? Which suffixes do they protect (all existing ones, or N chosen at random)? Do they take a prefix slot like the full lock? Does a reforge consume them? | The gamble strategies, and the cost of every locked reforge on items with fewer than 3 suffixes |
| Q2 | Can a locked reforge **add** new mods into open slots on the locked side? (Sanavixx's boots method implies yes; your rule says the protected side is untouched.) | Whether "fill the suffixes first" matters, and the Sanavixx boots route |
| Q3 | How many new mods does a locked reforge roll? (Model: totals 4/5/6 with weights 8/3/1, at least one new mod.) | The 0–2 extra-prefix odds and how often prefixes end up full |
| Q4 | Rare mod count when rerolling: 4/5/6 = 8/3/1? | Every reroll and Awakener fill |
| Q5 | Foulborn Exalted Orb: what exactly does it do? | Filling open prefixes |
| Q6 | Hinekora's Lock: after declining a preview, does a new lock give a different outcome? | Preview farming (third-suffix strategy) |
| Q7 | Awakener: is the kept influenced mod picked uniformly when an item has several? Is the rest filled to 4–6 mods? | Merge outcomes |
| Q8 | Dominance: target and victim uniform? Do upgrades below T1 ignore item level? | Elevation odds |
| Q9 | Veiled Exalted Orb: which side gets the veiled affix? How many unveil options are offered? | The Veiled movement-speed finish |
| Q10 | "Add a Mod to a <Conqueror> Item" beastcraft: does it require that influence already? Are its weights the same as the conqueror exalt? | The cheap way to get a second influenced mod |
| Q11 | Metamod beastcraft: do you choose which metamod, or is it random? | Cost of locks on magic items |
