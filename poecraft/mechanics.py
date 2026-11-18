"""Every crafting rule lives in this file.

Each rule cites its source:
  * PoEDB = https://poedb.tw/us/<page> (fetched 2026-11-18; PoEDB pages embed the
    community-wiki text, which is how PoE Wiki content was read: poewiki.net itself
    returned HTTP 403 to this environment)
  * RePoE = game data export, see tools/build_data.py
  * "in-game testing (user)" = ground truth supplied by the project owner
  * UNVERIFIED = our best guess; listed in README "Unverified rules".

Rules take a *chooser* ``ch`` for every random decision, so the same code is used
for Monte Carlo sampling (random chooser) and exact enumeration (enumerating
chooser, see exact.py).
"""
from __future__ import annotations

import random
from dataclasses import replace as _dc_replace

from .data import CONQUERORS, GameData
from .item import METAMODS, MULTIMOD, PREFIX_LOCK, SUFFIX_LOCK, Item, Roll


def replace(obj, **kw):
    return obj.evolve(**kw) if isinstance(obj, Item) else _dc_replace(obj, **kw)


class CraftError(Exception):
    """An action that the game would not allow on the current item."""


# ---------------------------------------------------------------- choosers
class RandomChooser:
    def __init__(self, seed=None):
        self.rng = random.Random(seed)

    def weighted(self, options, weights):
        return self.rng.choices(options, weights)[0]

    def uniform(self, options):
        return options[self.rng.randrange(len(options))]


# ---------------------------------------------------------------- rule switches
# Rules whose in-game behaviour is disputed. Recipes can flip them with `rule <name> on|off`.
DEFAULT_RULES = {
    # Can a Harvest reforge ADD new mods into open slots on the side protected by a
    # "Cannot Be Changed" metamod? Default off = in-game testing (user): "the protected side is
    # left untouched". Sanavixx "Endgame Speed Boots" step 3 (reforge Physical with suffixes
    # locked to gain a Hunter *suffix*) and step 6 (fill suffixes before the locked reforge)
    # only make sense if this is ON. See docs/known-discrepancies.md #5.
    "reforge_fills_locked_side": False,
}
RULES = dict(DEFAULT_RULES)


# ---------------------------------------------------------------- constants
# Number of affixes on a freshly rolled rare (Chaos/Alchemy/reforge/Awakener fill).
# UNVERIFIED: commonly cited community figures (as used by Craft of Exile):
# 4 mods 8/12, 5 mods 3/12, 6 mods 1/12. Not confirmed on PoEDB/PoE Wiki.
RARE_MOD_COUNT = {4: 8, 5: 3, 6: 1}
# UNVERIFIED: magic items roll 1 or 2 affixes with equal chance.
MAGIC_MOD_COUNT = {1: 1, 2: 1}

ARMOUR_CLASSES = {"Body Armour", "Boots", "Gloves", "Helmet"}

# Harvest reforge costs. Source: PoEDB Horticrafting (https://poedb.tw/us/Horticrafting)
# "Reforge a Rare item with random modifiers, including a X modifier".
HARVEST_REFORGE = {
    "fire": ("lifeforce_wild", 50), "cold": ("lifeforce_vivid", 50), "lightning": ("lifeforce_primal", 50),
    "physical": ("lifeforce_vivid", 50), "life": ("lifeforce_wild", 75), "defences": ("lifeforce_primal", 75),
    "chaos": ("lifeforce_vivid", 100), "attack": ("lifeforce_wild", 75), "caster": ("lifeforce_primal", 75),
    "speed": ("lifeforce_vivid", 150), "critical": ("lifeforce_primal", 150), "minion": ("lifeforce_primal", 200),
    "elemental": ("lifeforce_wild", 200), "attribute": ("lifeforce_vivid", 200), "mana": ("lifeforce_primal", 200),
}
# RePoE tag names differ from the in-game craft names for two tags.
HARVEST_TAG = {"defences": "defences", "defence": "defences"}

# Beastcrafts: recipe -> beasts consumed. Source: PoEDB Bestiary Recipe
# (https://poedb.tw/us/Bestiary_Recipe), fetched 2026-11-18. "any_rare_beast" is the
# generic "Any Rare Creature" ingredient (priced separately, default ~0).
BEASTCRAFTS = {
    "metamod": (["Wild Bristle Matron"], 3, "Add a crafted Meta-modifier to a non-Unique Item"),
    "imprint": (["Craicic Croaker"], 3, "Create an Imprint: Of a Magic Item"),
    "hinekoras_lock": (["Craicic Croaker", "Black Mórrigan"], 2, "Apply a Hinekora's Lock: To a Magic Item"),
    "max_links": (["Craicic Sand Spitter", "Black Mórrigan"], 2, "Modify an Item: to Have Maximum Possible Links"),
    "max_sockets": (["Craicic Shield Crab", "Black Mórrigan"], 2, "Modify an Item: to Have Maximum Possible Number of Sockets"),
    "six_link_rare": (["Saqawine Vulture"], 3, "Create an Item: Fully-linked Six-socket Rare"),
    "add_prefix_remove_suffix": (["Farric Wolf Alpha"], 3, "Add a Prefix, Remove a Random Suffix (rare only)"),
    "add_suffix_remove_prefix": (["Farric Lynx Alpha"], 3, "Add a Suffix, Remove a Random Prefix (rare only)"),
    "add_hunter_mod": (["Craicic Watcher", "Craicic Maw"], 2, "Add a Mod to a Hunter Item"),
    "add_warlord_mod": (["Fenumal Scrabbler", "Craicic Maw"], 2, "Add a Mod to a Warlord Item"),
    "add_redeemer_mod": (["Fenumal Queen", "Craicic Maw"], 2, "Add a Mod to a Redeemer Item"),
}
# Crusader/Shaper/Elder "add a mod" beastcrafts exist too but their beasts were not
# extracted; add them here when needed.


def beast_cost(name):
    beasts, any_rare, _ = BEASTCRAFTS[name]
    cost = {f"beast:{b}": 1 for b in beasts}
    if any_rare:
        cost["any_rare_beast"] = any_rare
    return cost


# ---------------------------------------------------------------- pool helpers
def _removable(item: Item, r: Roll):
    """Can a 'remove/reroll' effect touch this mod?
    Fractured mods: never (PoEDB Fracturing Orb). Mods on a side protected by
    'Prefixes/Suffixes Cannot Be Changed' are protected (PoEDB Orb of Dominance
    community text: "modifiers that can't be changed (e.g. suffixes on an item with
    the 'Suffixes can't be changed' mod) will not be affected"; the same metamod
    protects against Scour/Annul/Harvest reforge per the bench craft's purpose).
    """
    if r.fractured:
        return False
    gen = _gd.mods[r.id].gen
    if item.locked_side(gen) and r.id not in METAMODS:
        return False
    return True


_gd: GameData = None  # set by bind()


def bind(gd: GameData):
    global _gd
    if gd is not _gd:
        _POOL_CACHE.clear()
    _gd = gd


def pool(item: Item, sides=("prefix", "suffix"), influence_only=None, tag=None, exclude_groups=()):
    """Mods that can be added to ``item`` right now with their weights.

    Rule (user DATA RULES + PoE Wiki "Modifier"): chance = weight / total weight of
    all mods that can currently roll, respecting ilvl, item tags incl. influence
    tags, open prefix/suffix slots and mod-group exclusion (a mod cannot roll if a
    mod of the same group is already on the item)."""
    gd = _gd
    present = frozenset(item.groups(gd) | set(exclude_groups))
    open_ = {g: item.open_slots(gd, g) > 0 for g in ("prefix", "suffix")}
    key = (item.base, item.influences, item.ilvl, present, open_["prefix"], open_["suffix"], sides,
           influence_only, tag)
    hit = _POOL_CACHE.get(key)
    if hit is not None:
        return hit
    out_m, out_w = [], []
    for mid, w in gd.spawn_pool(item.base, item.influences, item.ilvl):
        m = gd.mods[mid]
        if m.gen not in sides or not open_[m.gen]:
            continue
        if influence_only and m.influence != influence_only:
            continue
        if tag and tag not in m.tags:
            continue
        if any(g in present for g in m.groups):
            continue
        out_m.append(mid)
        out_w.append(w)
    if len(_POOL_CACHE) > 200_000:
        _POOL_CACHE.clear()
    _POOL_CACHE[key] = (out_m, out_w)
    return out_m, out_w


_POOL_CACHE = {}


def add_random_mod(item: Item, ch, **kw) -> Item:
    mods, weights = pool(item, **kw)
    if not mods:
        return None
    mid = ch.weighted(mods, weights)
    return item.with_mods(item.mods + (Roll(mid),))


def fill_random(item: Item, ch, n_total: int, **kw) -> Item:
    """Add random mods until the item has n_total mods (or nothing can roll)."""
    while len(item.mods) < n_total:
        nxt = add_random_mod(item, ch, **kw)
        if nxt is None:
            break
        item = nxt
    return item


def _count(ch, table):
    keys = sorted(table)
    return ch.weighted(keys, [table[k] for k in keys])


def _require(cond, msg):
    if not cond:
        raise CraftError(msg)


# ---------------------------------------------------------------- basic currency
# Source for the basic currency rules: PoEDB item descriptions of each orb.
def transmute(item, ch):
    _require(item.rarity == "normal", f"Orb of Transmutation needs a normal item (item is {item.rarity}).")
    it = replace(item, rarity="magic", preview_lock=False)
    return fill_random(it, ch, len(item.mods) + _count(ch, MAGIC_MOD_COUNT))


def alteration(item, ch):
    _require(item.rarity == "magic", f"Orb of Alteration needs a magic item (item is {item.rarity}).")
    keep = tuple(r for r in item.mods if r.fractured)
    it = item.with_mods(keep)
    return fill_random(it, ch, len(keep) + _count(ch, MAGIC_MOD_COUNT))


def augment(item, ch):
    _require(item.rarity == "magic", "Orb of Augmentation needs a magic item.")
    _require(len(item.mods) < 2, "Orb of Augmentation needs a magic item with an open affix.")
    return add_random_mod(item, ch) or item


def regal(item, ch):
    _require(item.rarity == "magic", f"Regal Orb needs a magic item (item is {item.rarity}).")
    it = replace(item, rarity="rare")
    return add_random_mod(it, ch) or it.with_mods(it.mods)


def alchemy(item, ch):
    _require(item.rarity == "normal", "Orb of Alchemy needs a normal item.")
    it = replace(item, rarity="rare", preview_lock=False)
    return fill_random(it, ch, len(item.mods) + _count(ch, RARE_MOD_COUNT))


def chaos(item, ch):
    """Chaos Orb: reroll a rare item's mods. Keeps fractured mods.
    UNVERIFIED: also keeps the side protected by a 'cannot be changed' metamod
    (commonly stated; not confirmed on PoEDB)."""
    _require(item.rarity == "rare", f"Chaos Orb needs a rare item (item is {item.rarity}).")
    keep = tuple(r for r in item.mods if not _removable(item, r))
    it = item.with_mods(keep)
    return fill_random(it, ch, max(len(keep) + 1, _count(ch, RARE_MOD_COUNT)))


def exalt(item, ch):
    _require(item.rarity == "rare", "Exalted Orb needs a rare item.")
    _require(len(item.mods) < 6 and (item.open_slots(_gd, "prefix") or item.open_slots(_gd, "suffix")),
             "Exalted Orb needs a rare item with an open prefix or suffix.")
    nxt = add_random_mod(item, ch)
    _require(nxt is not None, "No modifier can roll on this item (all eligible groups taken).")
    return nxt


def annul(item, ch):
    """Orb of Annulment: removes a random modifier. Cannot remove fractured mods or
    mods protected by a metamod (PoEDB 'Prefixes Cannot Be Changed')."""
    _require(item.rarity in ("magic", "rare"), "Orb of Annulment needs a magic or rare item.")
    cands = [r for r in item.mods if _removable(item, r)]
    _require(cands, "Orb of Annulment: no removable modifier.")
    victim = ch.uniform(cands)
    return item.with_mods(tuple(r for r in item.mods if r != victim))


def scour(item, ch=None):
    """Orb of Scouring: removes all removable modifiers. Rarity afterwards: normal if
    nothing is left, magic if at most 1 prefix and 1 suffix remain, else rare.
    The metamod itself is on the *other* side, so it is scoured away too.
    Source: Sanavixx Endgame Chest step 2 ('craft Suffixes Cannot Be Changed, then
    scour: result should be a magic item with only [the suffix]'); PoEDB Orb of Scouring."""
    keep = tuple(r for r in item.mods if not _removable(item, r) and r.id not in METAMODS)
    it = item.with_mods(keep)
    np_, ns = it.n_prefix(_gd), it.n_suffix(_gd)
    rarity = "normal" if not keep else ("magic" if np_ <= 1 and ns <= 1 else "rare")
    return replace(it, rarity=rarity)


# ---------------------------------------------------------------- influence
def conqueror_exalt(item, ch, influence):
    """Hunter's/Warlord's/Redeemer's/Crusader's Exalted Orb.
    Source: PoEDB Hunters_Exalted_Orb: 'Adds Hunter influence and a new Hunter
    modifier to a rare item ... a high-level rare item with no influence'."""
    _require(influence in CONQUERORS, f"Unknown conqueror influence {influence!r}.")
    _require(item.rarity == "rare", f"{influence.title()}'s Exalted Orb needs a rare item.")
    _require(not item.influences, f"{influence.title()}'s Exalted Orb needs an item with no influence "
                                  f"(item has {', '.join(sorted(item.influences))}).")
    it = replace(item, influences=frozenset([influence]))
    nxt = add_random_mod(it, ch, influence_only=influence)
    _require(nxt is not None, f"No {influence.title()} modifier can roll (check open prefix/suffix slots).")
    return nxt


def dominance(item, ch):
    """Orb of Dominance.
    Source: PoEDB Orb_of_Dominance: 'Removes one Influenced Modifier from an item with
    at least two Influenced Modifiers and upgrades another Influenced Modifier.
    Upgrading a modifier of the highest tier transforms the modifier into an Elevated
    Modifier. Attempting to upgrade an Elevated Modifier rerolls its values. Can be
    used on Body Armours, Boots, Gloves and Helmets.' Community text on the same page:
    'used on Rare items', 'randomly upgrades one ... removes one other', 'doesn't
    distinguish between influence types', 'modifiers that can't be changed (e.g.
    suffixes on an item with the Suffixes can't be changed mod) will not be affected',
    'A tier 3 mod will be replaced with its tier 2 version', 'There is no item level
    requirement for a mod to become an elevated tier.'
    UNVERIFIED: (a) target and victim are chosen uniformly and independently of
    weights; (b) tier upgrades below T1 also ignore item level; (c) protected
    influenced mods still count toward the 'at least two' requirement but are never
    chosen, and at least two *changeable* influenced mods are needed.
    """
    gd = _gd
    _require(gd.item_class(item.base) in ARMOUR_CLASSES, "Orb of Dominance only works on body armours, boots, gloves and helmets.")
    _require(item.rarity == "rare", "Orb of Dominance needs a rare item.")
    infl = item.influenced_mods(gd)
    _require(len(infl) >= 2, f"Orb of Dominance needs at least two influenced modifiers (item has {len(infl)}).")
    elig = [r for r in infl if _removable(item, r)]
    _require(len(elig) >= 2, "Orb of Dominance: fewer than two influenced modifiers can be changed "
                             "(fractured or protected by a 'cannot be changed' metamod).")
    target = ch.uniform(elig)
    victim = ch.uniform([r for r in elig if r != target])
    fam = gd.family(target.id)
    t = fam.tier_of(target.id)
    if t == 0:
        new_id = target.id  # value reroll only
    elif t == 1:
        new_id = fam.elevated or target.id
    else:
        new_id = fam.tiers[t - 2]
    mods = [r for r in item.mods if r not in (target, victim)] + [replace(target, id=new_id)]
    return item.with_mods(tuple(mods))


def awaken(target: Item, donor: Item, ch):
    """Awakener's Orb: destroy ``donor``, apply its influence to ``target``.
    Source: PoEDB Awakeners_Orb: 'Destroys an item, applying its influence to another
    of the same item class. The second item is reforged as a rare item with both
    influence types and new modifiers'; community text: 'The two items must be of the
    same item type, with different influences. The target item's base type, item
    level, sockets, and links are retained. One signature mod ... from each item is
    guaranteed to merge on the new item, except ... the same mod group ... one will be
    discarded at random. If an item has two or more signature mods, one will be
    selected at random'; PoEDB Orb_of_Dominance: 'Elevated mods can be transferred to
    another item with an Awakener's Orb.'
    UNVERIFIED: the random pick among several influenced mods is uniform; the rest
    of the item is filled with random mods to the usual rare mod count (4-6)."""
    gd = _gd
    _require(gd.item_class(target.base) == gd.item_class(donor.base),
             "Awakener's Orb: both items must be of the same item class.")
    _require(target.influences and donor.influences, "Awakener's Orb: both items must be influenced.")
    _require(not (target.influences & donor.influences), "Awakener's Orb: the items must have different influences.")
    kept = []
    for it in (donor, target):
        infl = it.influenced_mods(gd)
        if infl:
            kept.append(ch.uniform(infl))
    if len(kept) == 2 and set(gd.mods[kept[0].id].groups) & set(gd.mods[kept[1].id].groups):
        kept = [ch.uniform(kept)]
    res = replace(target, rarity="rare", influences=target.influences | donor.influences, preview_lock=False)
    res = res.with_mods(tuple(Roll(r.id) for r in kept))
    return fill_random(res, ch, max(len(kept), _count(ch, RARE_MOD_COUNT)))


# ---------------------------------------------------------------- harvest
def harvest_reforge(item, ch, tag):
    """Harvest 'Reforge a Rare item with random modifiers, including a <Tag> modifier'.
    Cost source: PoEDB Horticrafting. Behaviour with metamods: **in-game testing
    (user)**: the reforge RESPECTS 'Prefixes/Suffixes Cannot Be Changed' (protected
    side untouched, other side rerolled) and the metamod is CONSUMED by the reforge.
    (Craft of Exile models this wrongly; see docs/known-discrepancies.md.)
    The guaranteed mod is drawn from the mods with that tag (RePoE implicit_tags)
    that can roll on a changeable side.
    UNVERIFIED: total mod count follows RARE_MOD_COUNT with kept mods counting toward
    it (at least one new mod); the guaranteed tagged mod is always a new mod."""
    tag = HARVEST_TAG.get(tag, tag)
    _require(tag in HARVEST_REFORGE, f"Unknown Harvest reforge tag {tag!r}. Known: {', '.join(sorted(HARVEST_REFORGE))}.")
    _require(item.rarity == "rare", "Harvest reforge needs a rare item.")
    keep = tuple(r for r in item.mods if not _removable(item, r) and r.id not in METAMODS)
    base = item.with_mods(keep)
    if RULES["reforge_fills_locked_side"]:
        sides = ("prefix", "suffix")
    else:
        sides = tuple(g for g in ("prefix", "suffix") if not item.locked_side(g))
    mods, weights = pool(base, sides=sides, tag=tag)
    _require(mods, f"Harvest reforge {tag}: no {tag} modifier can roll on the changeable side(s) of this item.")
    first = ch.weighted(mods, weights)
    it = base.with_mods(keep + (Roll(first),))
    n = max(len(it.mods), _count(ch, RARE_MOD_COUNT))
    return fill_random(it, ch, n, sides=sides)


# ---------------------------------------------------------------- essence
def essence(item, ch, essence_id):
    """Essence: on a normal item -> rare with the essence mod + random mods; on a rare
    item -> reroll keeping the essence mod (Source: PoEDB Essence pages). Keeps
    fractured mods. Item level restriction from RePoE essences.json."""
    gd = _gd
    e = gd.essences[essence_id]
    cls = gd.item_class(item.base)
    _require(cls in e["mods"], f'{e["name"]} cannot be used on {cls}.')
    _require(item.rarity in ("normal", "rare"), f'{e["name"]} needs a normal or rare item.')
    lim = e.get("item_level_restriction")
    if lim:
        _require(item.ilvl <= lim, f'{e["name"]} only works on items up to item level {lim}.')
    keep = tuple(r for r in item.mods if r.fractured)
    mid = e["mods"][cls]
    it = replace(item, rarity="rare").with_mods(keep + (Roll(mid),))
    return fill_random(it, ch, max(len(it.mods), _count(ch, RARE_MOD_COUNT)))


# ---------------------------------------------------------------- bench
def bench_craft(item, mod_id):
    """Crafting bench: add a crafted mod. One crafted mod per item unless 'Can have
    up to 3 Crafted Modifiers' is present (PoEDB Crafting Bench). Needs an open slot
    on the mod's side. Metamods are crafted mods and take an affix slot:
    'Suffixes Cannot Be Changed' is a PREFIX, 'Prefixes Cannot Be Changed' a SUFFIX
    (RePoE mods.json generation_type of DexMaster/StrMaster...CannotChange*)."""
    gd = _gd
    m = gd.mods[mod_id]
    _require(item.rarity in ("magic", "rare"), "The crafting bench needs a magic or rare item.")
    _require(item.open_slots(gd, m.gen) > 0, f"Bench craft '{m.short()}' needs an open {m.gen}.")
    crafted = item.crafted_mods()
    limit = 3 if item.has_mod(MULTIMOD) else 1
    if mod_id == MULTIMOD:
        limit = 3
    _require(len(crafted) < limit, "Item already has a crafted modifier (craft 'multimod' first to allow up to 3).")
    _require(not (set(m.groups) & item.groups(gd)), f"Item already has a modifier of group {m.group}.")
    return item.with_mods(item.mods + (Roll(mod_id, crafted=True),))


def remove_crafted(item):
    return item.with_mods(tuple(r for r in item.mods if not r.crafted))


# ---------------------------------------------------------------- beastcraft
def beast_metamod(item, which):
    """Beastcraft 'Add a crafted Meta-modifier to a non-Unique Item' (Wild Bristle Matron).
    In-game (user): only works on MAGIC items, not rares (Sanavixx's chest/boots guides
    use it on rares; see docs/known-discrepancies.md #7).
    UNVERIFIED: the player chooses which metamod; it is the same crafted mod as the
    bench version, so 'Suffixes Cannot Be Changed' needs an open PREFIX."""
    _require(item.rarity == "magic", "the metamod beastcraft only works on magic items (in-game, user); "
                                     "on a rare use the bench craft: craft lock_suffixes (4 divine + 4 blessed)")
    mod = {"lock_suffixes": SUFFIX_LOCK, "lock_prefixes": PREFIX_LOCK, "multimod": MULTIMOD}[which]
    return bench_craft(item, mod)


def beast_add_influence_mod(item, ch, influence):
    """Beastcraft 'Modify Mods on an Item: Add a Mod to a <Conqueror> Item'.
    UNVERIFIED: needs a rare item that already has the influence; adds one random mod
    of that influence (weights as for the conqueror exalt)."""
    _require(item.rarity == "rare", "This beastcraft needs a rare item.")
    _require(influence in item.influences, f"Item has no {influence.title()} influence.")
    nxt = add_random_mod(item, ch, influence_only=influence)
    _require(nxt is not None, f"No {influence.title()} modifier can be added (open slot?).")
    return nxt


def beast_swap_side(item, ch, add_side):
    """Beastcraft 'Add a Prefix, Remove a Random Suffix' / 'Add a Suffix, Remove a Random
    Prefix' (rare only). UNVERIFIED: removal happens first and respects metamods."""
    _require(item.rarity == "rare", "This beastcraft needs a rare item.")
    rem_side = "suffix" if add_side == "prefix" else "prefix"
    cands = [r for r in item.side(_gd, rem_side) if _removable(item, r)]
    _require(cands, f"No removable {rem_side}.")
    victim = ch.uniform(cands)
    it = item.with_mods(tuple(r for r in item.mods if r != victim))
    nxt = add_random_mod(it, ch, sides=(add_side,))
    _require(nxt is not None, f"No {add_side} can roll.")
    return nxt


def hinekoras_lock(item):
    """Beastcraft 'Apply a Hinekora's Lock: To a Magic Item'. Hinekora's Lock (PoEDB):
    'Allows an item to foresee the result of the next Currency item used on it.
    Modifying the item in any way removes the ability to foresee.'"""
    _require(item.rarity == "magic", "The Hinekora's Lock beastcraft needs a magic item.")
    return replace(item, preview_lock=True)


def max_links(item):
    """Beastcraft 'Modify an Item: to Have Maximum Possible Links'. Sockets/links are
    not otherwise simulated; body armours are assumed to have 6 sockets."""
    n = 6 if _gd.item_class(item.base) == "Body Armour" else 4
    return replace(item, links=n)


def omen_blanching(item):
    """Omen of Blanching for white sockets. STUB: only the cost is modelled
    (UNVERIFIED mechanics, price must be set manually)."""
    return replace(item, white_sockets=max(item.white_sockets, 2))


def fracture(item, ch):
    """Fracturing Orb: fractures a random modifier of a rare item with at least 4
    modifiers (PoEDB Fracturing Orb). UNVERIFIED: crafted mods are not eligible."""
    _require(item.rarity == "rare" and len(item.mods) >= 4, "Fracturing Orb needs a rare item with 4+ modifiers.")
    _require(not any(r.fractured for r in item.mods), "Item already has a fractured modifier.")
    cands = [r for r in item.mods if not r.crafted]
    t = ch.uniform(cands)
    return item.with_mods(tuple(r for r in item.mods if r != t) + (replace(t, fractured=True),))


def veiled_exalt(item, ch):
    """Veiled Exalted Orb. PoEDB (current text): 'Removes a random modifier and adds a
    random Veiled modifier to a rare item'.
    UNVERIFIED: the removal respects metamods/fractured mods like an Annulment (Sanavixx
    "Endgame Speed Boots" step 8 relies on this); the veiled affix goes to a uniformly
    chosen side with an open slot after the removal."""
    _require(item.rarity == "rare", "Veiled Exalted Orb needs a rare item.")
    cands = [r for r in item.mods if _removable(item, r)]
    _require(cands, "Veiled Exalted Orb: no removable modifier.")
    victim = ch.uniform(cands)
    it = item.with_mods(tuple(r for r in item.mods if r != victim))
    sides = [g for g in ("prefix", "suffix") if it.open_slots(_gd, g) > 0]
    side = ch.uniform(sides)
    return it.with_mods(it.mods + (Roll("VeiledPrefix" if side == "prefix" else "VeiledSuffix"),))


UNVEIL_OPTIONS = 3


def unveil(item, ch, preferences):
    """Unveil (Jun): the veiled affix becomes one of UNVEIL_OPTIONS offered unveiled mods.
    UNVERIFIED: options are drawn by RePoE unveiled spawn weight without replacement,
    skipping mod groups already on the item; the player takes the first mod in
    ``preferences`` (list of family keys) that is offered, otherwise the first offer."""
    gd = _gd
    veiled = [r for r in item.mods if r.id in ("VeiledPrefix", "VeiledSuffix")]
    _require(veiled, "unveil: the item has no veiled modifier.")
    v = veiled[0]
    gen = "prefix" if v.id == "VeiledPrefix" else "suffix"
    rest = item.with_mods(tuple(r for r in item.mods if r != v))
    present = rest.groups(gd)
    pool_ = [(m, w) for m, w in gd.unveil_pool(item.base, item.influences, item.ilvl, gen)
             if not (set(gd.mods[m].groups) & present)]
    _require(pool_, "unveil: no unveiled modifier can appear.")
    offers = []
    while pool_ and len(offers) < UNVEIL_OPTIONS:
        m = ch.weighted([p[0] for p in pool_], [p[1] for p in pool_])
        offers.append(m)
        pool_ = [p for p in pool_ if not (set(gd.mods[p[0]].groups) & set(gd.mods[m].groups))]
    choice = offers[0]
    for fam in preferences:
        hit = [m for m in offers if gd.family_key(m) == fam]
        if hit:
            choice = hit[0]
            break
    return rest.with_mods(rest.mods + (Roll(choice, crafted=False),))


def foulborn_exalt(item, ch, side=None):
    """Foulborn Exalted Orb (Covens-era currency). Its exact effect was not on the PoEDB
    pages fetched. UNVERIFIED model: like an Exalted Orb, adds one random mod to an open
    side (used by the user to fill an open prefix when suffixes are full, and by
    Sanavixx to fill suffixes). ``side`` restricts it to one side, standing in for a
    "Cannot Be Changed" craft on the other side (Sanavixx boots step 6)."""
    _require(item.rarity == "rare", "Foulborn Exalted Orb needs a rare item.")
    nxt = add_random_mod(item, ch, sides=(side,) if side else ("prefix", "suffix"))
    _require(nxt is not None, f"Foulborn Exalted Orb: no open {side or 'affix'}.")
    return nxt
