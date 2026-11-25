"""Game data: bases, mods, mod families/tiers, readable mod names, mod pools.

All data comes from ``data/gamedata.json`` which is built from RePoE by
``tools/build_data.py`` (see that file for the pinned version).

Terminology
-----------
* **mod**: one RePoE mod id, e.g. ``AdditionalCritWithAttacksInfluence2_``.
* **family**: all tiers of "the same" mod: same affix side, same mod group and the
  same influence (or none). Hunter attack crit and Elder attack crit share a group
  but are different families.
* **tier**: 1 = best rollable tier (highest required level). Elevated mods are
  their own tier 0 (``elevated``) and never roll naturally (spawn weight 0).
"""
from __future__ import annotations

import json
import os
import re
from dataclasses import dataclass, field
from functools import lru_cache

DATA_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "data")

# Influence names <-> RePoE influence tag suffix.
# Source: RePoE item_classes.json "influence_tags" (order crusader, elder, basilisk,
# eyrie, shaper, adjudicator) cross-checked with mod names: "Hunter's" mods spawn on
# *_basilisk, "Warlord's"/"of the Conquest" on *_adjudicator, "Redeemer's" on *_eyrie.
INFLUENCE_TAG_SUFFIX = {
    "shaper": "shaper",
    "elder": "elder",
    "crusader": "crusader",
    "redeemer": "eyrie",
    "hunter": "basilisk",
    "warlord": "adjudicator",
}
INFLUENCES = tuple(INFLUENCE_TAG_SUFFIX)
CONQUERORS = ("crusader", "redeemer", "hunter", "warlord")

# Influence tags per item class come from RePoE item_classes.json (see GameData.influence_tag).


class DataError(Exception):
    pass


@dataclass(frozen=True)
class Mod:
    id: str
    name: str
    gen: str  # "prefix" | "suffix"
    group: str
    groups: tuple
    type: str
    tags: tuple
    level: int
    spawn: tuple
    genw: tuple
    text: str
    essence_only: bool
    influence: str | None  # influence this mod belongs to (from its spawn tags)
    elevated: bool
    crafted: bool = False
    domain: str = "item"

    @property
    def family_key(self):
        # Mod "type" separates different mods sharing a group (e.g. +1 support gems vs
        # +2 cold gems); elevated versions carry the same type + "Maven" suffix.
        # Crafted (bench) mods form their own families so `has` can tell them apart.
        return (self.gen, self.group, "crafted" if self.crafted else self.influence,
                re.sub(r"(Influence)?Maven(V\d+)?_*$", "", self.type))

    def short(self):
        return self.text.replace("\n", " / ")


@dataclass
class Family:
    key: tuple
    tiers: list = field(default_factory=list)  # mod ids, tier 1 first (rollable only)
    elevated: str | None = None

    def tier_of(self, mod_id):
        if mod_id == self.elevated:
            return 0
        return self.tiers.index(mod_id) + 1


class GameData:
    def __init__(self, path=None):
        path = path or os.path.join(DATA_DIR, "gamedata.json")
        with open(path, encoding="utf-8") as f:
            raw = json.load(f)
        self.source = raw["source"]
        self.classes = raw["classes"]
        self.bases = raw["bases"]
        self.essences = raw["essences"]
        self.bench = raw["bench"]
        bench_mods = {b["mod"] for b in raw["bench"]}
        infl_tags = {}
        for cls, c in self.classes.items():
            for t in c["influence_tags"]:
                for inf, suf in INFLUENCE_TAG_SUFFIX.items():
                    if t.endswith("_" + suf):
                        infl_tags[t] = inf
        self.influence_tag_to_name = infl_tags
        self.mods: dict[str, Mod] = {}
        for mid, m in raw["mods"].items():
            influence = None
            for tag, _w in m["spawn"]:
                if tag in infl_tags:
                    influence = infl_tags[tag]
                    break
            self.mods[mid] = Mod(
                id=mid, name=m["name"], gen=m["gen"], group=m["groups"][0] if m["groups"] else mid,
                groups=tuple(m["groups"]), type=m["type"], tags=tuple(m["tags"]), level=m["level"],
                spawn=tuple(tuple(x) for x in m["spawn"]), genw=tuple(tuple(x) for x in m["genw"]),
                text=m["text"], essence_only=m["essence_only"], influence=influence,
                elevated="Elevated" in m["name"], crafted=mid in bench_mods, domain=m.get("domain", "item"),
            )
        self._build_families()
        self.aliases = self._load_aliases()

    # ------------------------------------------------------------------ families
    def _build_families(self):
        self._elevated_family_override = {}
        self._fk_cache = {}
        self._tier_cache = {}
        fams: dict[tuple, Family] = {}
        for m in self.mods.values():
            if m.crafted or m.domain == "veiled" or m.gen not in ("prefix", "suffix"):
                continue
            if m.type.startswith("OLD"):
                # legacy versions of elevated mods (e.g. OLDAdditionalCurseOnEnemiesMaven)
                continue
            fam = fams.setdefault(m.family_key, Family(m.family_key))
            if m.elevated:
                fam.elevated = m.id
            elif any(w > 0 for _t, w in m.spawn) or m.essence_only or m.domain == "unveiled":
                fam.tiers.append(m.id)
        # Some elevated mods have a "type" that does not match their base tiers
        # (e.g. ReducedPhysicalReflectTaken vs PhysicalDamageCannotBeReflected...).
        # Attach such orphans to the single family with the same side/group/influence
        # that has tiers but no elevated version.
        for key, fam in list(fams.items()):
            if fam.tiers or not fam.elevated:
                continue
            matches = [f for k, f in fams.items() if k[:3] == key[:3] and f.tiers and not f.elevated]
            if len(matches) == 1:
                matches[0].elevated = fam.elevated
                self._elevated_family_override[fam.elevated] = matches[0].key
                del fams[key]
        for fam in fams.values():
            fam.tiers.sort(key=lambda i: (-self.mods[i].level, i))
        self.families = fams

    def family_key(self, mod_id) -> tuple:
        k = self._fk_cache.get(mod_id)
        if k is None:
            k = self._elevated_family_override.get(mod_id) or self.mods[mod_id].family_key
            self._fk_cache[mod_id] = k
        return k

    def family(self, mod_id) -> Family:
        return self.families[self.family_key(mod_id)]

    def tier(self, mod_id, base_id=None) -> int:
        """1 = best natural tier, 0 = elevated.

        With ``base_id`` the tier is ranked among the family's mods that can roll on that
        base (any influence of its class). Without it, the rank is across ALL item classes,
        which is wrong once several classes are loaded (e.g. a ring-only tier would sit
        between two body-armour tiers), so callers should always pass the base."""
        key = (mod_id, base_id)
        t = self._tier_cache.get(key)
        if t is None:
            m = self.mods[mod_id]
            fk = self.family_key(mod_id)
            if m.crafted or fk not in self.families:
                t = 1
            elif mod_id == self.families[fk].elevated:
                t = 0
            elif base_id is not None and mod_id in self.tiers_for(fk, base_id):
                t = self.tiers_for(fk, base_id).index(mod_id) + 1
            else:
                t = self.families[fk].tier_of(mod_id)
            self._tier_cache[key] = t
        return t

    def weight_on(self, mod_id, base_id) -> int:
        """Spawn weight of a mod on a base, counting every influence its class can have
        (first matching spawn tag wins: RePoE docs/mods.md)."""
        b = self.bases[base_id]
        tags = set(b["tags"]) | set(self.classes[b["item_class"]]["influence_tags"])
        for tag, w in self.mods[mod_id].spawn:
            if tag in tags:
                return w
        return 0

    @lru_cache(maxsize=None)
    def tiers_for(self, fam_key, base_id) -> tuple:
        """The family's natural tiers that can appear on this base, best first."""
        fam = self.families[fam_key]
        cls = self.bases[base_id]["item_class"]
        ess = {e["mods"].get(cls) for e in self.essences.values()}
        dom = self.bases[base_id].get("domain", "item")
        out = [m for m in fam.tiers
               if (self.mods[m].domain in (dom, "unveiled") and self.weight_on(m, base_id) > 0)
               or (self.mods[m].essence_only and m in ess)]
        return tuple(out)

    # ------------------------------------------------------------------ bases
    @lru_cache(maxsize=None)
    def resolve_base(self, name: str) -> str:
        if name in self.bases:
            return name
        hits = [bid for bid, b in self.bases.items() if b["name"].lower() == name.lower()]
        if not hits:
            close = sorted({b["name"] for b in self.bases.values() if name.lower() in b["name"].lower()})
            hint = f" Did you mean one of: {', '.join(close[:10])}?" if close else ""
            raise DataError(f'Unknown base "{name}".{hint} (Supported classes: {", ".join(self.classes)})')
        # several ids can share a name (e.g. Two-Toned Boots variants): pick the
        # first in sorted order unless the caller passes the exact id.
        return sorted(hits)[0]

    def item_class(self, base_id):
        return self.bases[base_id]["item_class"]

    def influence_tag(self, base_id, influence):
        cls = self.item_class(base_id)
        suffix = "_" + INFLUENCE_TAG_SUFFIX[influence]
        for t in self.classes[cls]["influence_tags"]:
            if t.endswith(suffix):
                return t
        raise DataError(f"{cls} items cannot have {influence.title()} influence")

    # ------------------------------------------------------------------ names
    def _load_aliases(self):
        p = os.path.join(DATA_DIR, "aliases.json")
        if not os.path.exists(p):
            return {}
        with open(p, encoding="utf-8") as f:
            return {k.lower(): v for k, v in json.load(f).items() if not k.startswith("_")}

    @lru_cache(maxsize=None)
    def resolve_mod(self, ref: str, item_class: str | None = None) -> tuple:
        """Resolve a readable mod reference to a family key.

        Accepted forms (case-insensitive):
          * an alias from data/aliases.json, e.g. "Hunter: additional curse"
          * a RePoE mod id, e.g. "AdditionalCurseOnEnemiesInfluence1"
          * "<Influence>: <text fragment>", e.g. "Warlord: total mana cost"
          * "prefix: <text>" / "suffix: <text>" to disambiguate sides
          * a plain text fragment, e.g. "increased maximum Life"
        Returns the family key (gen, group, influence). Raises DataError with the
        candidate list when the reference is ambiguous or unknown.
        """
        r = ref.strip()
        low = r.lower()
        if low in self.aliases:
            r = self.aliases[low]
            low = r.lower()
        if r in self.mods:
            return self.family_key(r)
        influence = None
        gen = None
        text = low
        m = re.match(r"^\s*(\w+)\s*:\s*(.+)$", low)
        if m and m.group(1) in INFLUENCES + ("normal", "none"):
            influence = None if m.group(1) in ("normal", "none") else m.group(1)
            text = m.group(2)
            explicit_none = m.group(1) in ("normal", "none")
        else:
            explicit_none = False
        m2 = re.match(r"^\s*(prefix|suffix)\s*:\s*(.+)$", text)
        if m2:
            gen, text = m2.group(1), m2.group(2)
        cands = {}
        for fam_key, fam in self.families.items():
            fgen, _grp, finf, _t = fam_key
            if gen and fgen != gen:
                continue
            if influence and finf != influence:
                continue
            if explicit_none and finf is not None:
                continue
            ids = fam.tiers + ([fam.elevated] if fam.elevated else [])
            if item_class is not None and not any(self._can_spawn_on_class(i, item_class) for i in ids):
                continue
            for i in ids:
                if text in self.mods[i].text.lower() or text == i.lower():
                    cands[fam_key] = self.mods[i]
                    break
        if len(cands) == 1:
            return next(iter(cands))
        where = f" on {item_class}" if item_class else ""
        if not cands:
            raise DataError(
                f'No mod matches "{ref}"{where}. Use the in-game text (e.g. "Hunter: additional Curse"), '
                f'prefix it with an influence ("Warlord: ...") or side ("suffix: ..."), or run '
                f'`poecraft mods <base> --search "<text>"` to list mods.')
        lines = []
        for k, m in sorted(cands.items(), key=lambda kv: str(kv[0])):
            lines.append(f'    - {(k[2] or "normal").title()} {k[0]}: "{m.short()}"  (e.g. id {m.id})')
        raise DataError(
            f'Mod reference "{ref}"{where} is ambiguous; it matches {len(cands)} mod families:\n' + "\n".join(lines[:15])
            + ("\n    ..." if len(lines) > 15 else "")
            + '\n  Disambiguate with an influence ("Hunter: ...") or side ("prefix: ...") or more text.')

    def _can_spawn_on_class(self, mod_id, item_class):
        m = self.mods[mod_id]
        tags = self.class_tags(item_class) | set(self.classes[item_class]["influence_tags"])
        return any(tag in tags for tag, _w in m.spawn)

    @lru_cache(maxsize=None)
    def class_tags(self, item_class):
        s = set()
        for b in self.bases.values():
            if b["item_class"] == item_class:
                s.update(b["tags"])
        s.discard("default")
        return frozenset(s)

    def family_label(self, fam_key):
        gen, _g, inf, _t = fam_key
        fam = self.families.get(fam_key)
        if fam is None:  # bench-crafted mods have no family entry
            ex = max((m for m in self.mods.values() if m.family_key == fam_key), key=lambda m: (m.level, m.id)).id
            return f'{(inf or "normal").title()} {gen} "{self.mods[ex].short()}"'
        ex = fam.tiers[0] if fam.tiers else fam.elevated
        return f'{(inf or "normal").title()} {gen} "{self.mods[ex].short()}"'

    @lru_cache(maxsize=1024)
    def unveil_pool(self, base_id: str, influences: frozenset, ilvl: int, gen: str):
        """Unveiled mods (domain 'unveiled' in RePoE) that can appear when unveiling a
        veiled affix of side ``gen`` on this base."""
        tags = set(self.bases[base_id]["tags"])
        for inf in influences:
            tags.add(self.influence_tag(base_id, inf))
        out = []
        for mid, m in self.mods.items():
            if m.domain != "unveiled" or m.gen != gen or m.level > ilvl:
                continue
            w = 0
            for tag, tw in m.spawn:
                if tag in tags:
                    w = tw
                    break
            if w > 0:
                out.append((mid, w))
        out.sort()
        return tuple(out)

    # ------------------------------------------------------------------ pools
    @lru_cache(maxsize=4096)
    def spawn_pool(self, base_id: str, influences: frozenset, ilvl: int):
        """All (mod_id, weight) that can naturally roll on this base, ignoring the
        current mods. Weight = first matching spawn tag (RePoE semantics), times
        generation-weight multipliers (percent).
        Source for "weight / total weight of eligible mods": PoE Wiki "Modifier"
        article and RePoE docs/mods.md (spawn_weights: first matching tag wins)."""
        tags = set(self.bases[base_id]["tags"])
        for inf in influences:
            tags.add(self.influence_tag(base_id, inf))
        # A mod can only roll on a base of the same RePoE domain (RePoE docs/mods.md).
        domain = self.bases[base_id].get("domain", "item")
        out = []
        for mid, m in self.mods.items():
            if m.crafted or m.essence_only or m.elevated or m.level > ilvl or m.domain != domain                     or m.gen not in ("prefix", "suffix"):
                continue
            w = 0
            for tag, tw in m.spawn:
                if tag in tags:
                    w = tw
                    break
            if w <= 0:
                continue
            for tag, gw in m.genw:
                if tag in tags:
                    w = w * gw // 100
                    break
            if w > 0:
                out.append((mid, w))
        out.sort()
        return tuple(out)


_DEFAULT = None


def default_data() -> GameData:
    global _DEFAULT
    if _DEFAULT is None:
        _DEFAULT = GameData()
    return _DEFAULT
