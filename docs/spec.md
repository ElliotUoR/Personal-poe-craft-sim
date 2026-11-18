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
- **How the link is built (tested 2026-11-18 in the owner's logged-in browser):**
  - The trade page keeps the whole search in its URL: `/trade/search/<League>/<code>`, where
    `<code>` is the query JSON (the `query` object), gzip-compressed and base64url-encoded.
    A generated link opened in the normal trade page with every filter filled in.
  - Stat IDs come from `/api/trade/data/stats` (about 7,900 explicit, 300 pseudo, 290 crafted).
    The site's text differs from game data, e.g. "You can apply # additional Curses"
    (`explicit.stat_30642521`), "Socketed Attacks have +# to Total Mana Cost"
    (`explicit.stat_2264586521`, value −20), "Attacks have +#% to Critical Strike Chance"
    (`explicit.stat_2572042788`). The mapping matches our mods' stats and values to these
    templates rather than relying on exact text.
  - Elevated mods have no separate trade stat; they're found by value (elevated crit = min 1.6).
    The hybrid elevated line "Ignore Stuns while using Socketed Attack Skills" only exists on
    the site as a %-chance version, so links filter on the item's other line instead.
  - **Status filter:** `online` returns nothing in Covens; `securable` (instant buyout /
    merchant) and `available` work. Links default to `securable`.
  - Live check, elevated-crit Royal Plate with curse and −20 mana cost: 44 listed, cheapest
    18, 18, 19, 19, 19, 20 divine.

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

### M. Progress bars (owner request, 2026-11-18)
- Every long computation shows live progress: simulations (runs done / total, ETA), exact solves
  (spinner + elapsed), comparisons ("recipe k of n"), PoB scoring (items scored / total) and the
  reverse-engineering search.
- The engine exposes one shared progress object (`analysis::Progress`: phase, done, total,
  label). The CLI draws it on stderr (`--progress` forces it, `--quiet` hides it); the web app
  will stream the same object to the browser (server-sent events) and draw a bar.

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
Answers from the owner, 2026-11-18. The rule statuses are in `poecraft rules`.

| # | Question | Answer | Status |
|---|---|---|---|
| Q1 | Partial locks | Can always be crafted; protect exactly N; take a slot on the other side; consumed by a reforge. With more than N mods on the side, the protected ones are **random** and the freed slots **can** fill. | owner-confirmed |
| Q2 | Can a locked reforge add mods to the locked side? | **Yes.** Protected mods stay; open slots on that side can fill. Consequence: on the chest the Caster reforge only guarantees the curse when all 3 suffixes are full (2 suffixes: ~34%, the rest is the Hunter spell-crit suffix). See `curse_reforge_safe`. | owner-confirmed |
| Q3 | Mod count of a locked reforge | Owner expects the same odds as a Chaos Orb (as modelled). | **open** |
| Q4 | Rare mod count | Range 4-6 verified (PoEDB Orb_of_Alchemy wiki text: "four to six", "three to four on jewels"). The 8/3/1 odds remain the community figure. Our Astral Plate chaos prefix split 26.3/45.2/28.5% matches the owner's Craft of Exile run (26.1/44/28.5%), but CoE likely uses the same assumption. In-game counts would settle it. | **open (odds)** |
| Q5 | Foulborn Exalted Orb | Adds a mod; for each mod type with N eligible tiers only the top K can roll (K = ceil(N/2) if N<=6, else floor(N/2)+1), weights x N/K (PoE Wiki text). | verified |
| Q6 | Hinekora re-lock | New, different outcome. | owner-confirmed |
| Q7 | Awakener | Kept mod: uniformly random. Fill count: owner thinks 3-6; wiki text for Chaos/Alchemy says 4-6; modelled 4-6. | **open (fill count)** |
| Q8 | Dominance | Random target/victim; upgrades ignore item level. | owner-confirmed |
| Q9 | Veiled Exalted | Random side; 3 unveil options. | owner-confirmed |
| Q10 | "Add a Mod to a <Conqueror> Item" | Needs the influence; same odds as the conqueror exalt. Owner: "you can choose the tag for the mod" (to clarify). | owner-confirmed |
| Q11 | Metamod beastcraft | Owner suspects removal. It is still in the 3.30 game data (PoEDB Bestiary_Recipe and Wild_Bristle_Matron, 2026-11-18); availability in Covens unknown. | **open** |
