"""Immutable, hashable item state (hashable so the exact engine can use it as a
Markov-chain state)."""
from __future__ import annotations

from dataclasses import dataclass, field, replace

from .data import GameData

# Bench/beast metamod ids for armour pieces (RePoE crafting_bench_options.json, the
# "B" variants are the Body Armour/Gloves/Boots/Helmet versions).
SUFFIX_LOCK = "DexMasterItemGenerationCannotChangeSuffixesB"   # a crafted PREFIX
PREFIX_LOCK = "StrMasterItemGenerationCannotChangePrefixesB"   # a crafted SUFFIX
MULTIMOD = "StrIntMasterItemGenerationCanHaveMultipleCraftedMods"  # crafted suffix
METAMODS = {SUFFIX_LOCK, PREFIX_LOCK, MULTIMOD}


@dataclass(frozen=True, order=True)
class Roll:
    id: str
    fractured: bool = False
    crafted: bool = False


@dataclass(frozen=True)
class Item:
    base: str
    ilvl: int
    rarity: str = "normal"  # normal | magic | rare
    influences: frozenset = frozenset()
    mods: tuple = ()  # sorted tuple[Roll]
    preview_lock: bool = False  # Hinekora's Lock applied
    links: int = 0  # 6 after the max-links beastcraft; 0 = not tracked
    white_sockets: int = 0  # Omen of Blanching stub
    tag: str | None = field(default=None, compare=True)  # optional user label (e.g. "warlord piece")

    # -------------------------------------------------------------- helpers
    def with_mods(self, mods, **kw):
        return self.evolve(mods=tuple(sorted(mods)), preview_lock=False, **kw)

    def evolve(self, **kw):
        """Fast dataclasses.replace (this is the hot path of Monte Carlo runs)."""
        d = {"base": self.base, "ilvl": self.ilvl, "rarity": self.rarity, "influences": self.influences,
             "mods": self.mods, "preview_lock": self.preview_lock, "links": self.links,
             "white_sockets": self.white_sockets, "tag": self.tag}
        d.update(kw)
        return Item(**d)

    def mod_ids(self):
        return [r.id for r in self.mods]

    def side(self, gd: GameData, gen):
        return [r for r in self.mods if gd.mods[r.id].gen == gen]

    def n_prefix(self, gd):
        return len(self.side(gd, "prefix"))

    def n_suffix(self, gd):
        return len(self.side(gd, "suffix"))

    def max_affixes(self):
        return {"normal": 0, "magic": 1, "rare": 3}[self.rarity]

    def open_slots(self, gd, gen):
        return self.max_affixes() - len(self.side(gd, gen))

    def has_mod(self, mod_id):
        return any(r.id == mod_id for r in self.mods)

    def suffixes_locked(self):
        return self.has_mod(SUFFIX_LOCK)

    def prefixes_locked(self):
        return self.has_mod(PREFIX_LOCK)

    def locked_side(self, gen):
        return self.suffixes_locked() if gen == "suffix" else self.prefixes_locked()

    def influenced_mods(self, gd):
        return [r for r in self.mods if gd.mods[r.id].influence is not None and not r.crafted]

    def crafted_mods(self):
        return [r for r in self.mods if r.crafted]

    def groups(self, gd):
        s = set()
        for r in self.mods:
            s.update(gd.mods[r.id].groups)
        return s

    def describe(self, gd: GameData):
        b = gd.bases[self.base]
        inf = "/".join(sorted(i.title() for i in self.influences)) or "no influence"
        head = f'{self.rarity.title()} {b["name"]} (ilvl {self.ilvl}, {inf})'
        lines = [head]
        for gen in ("prefix", "suffix"):
            for r in self.side(gd, gen):
                m = gd.mods[r.id]
                t = gd.tier(r.id)
                tier = "crafted" if r.crafted else ("ELEVATED" if t == 0 else f"T{t}")
                fr = " [fractured]" if r.fractured else ""
                inf_s = f"{m.influence.title()} " if m.influence else ""
                lines.append(f"  {gen[0].upper()} {inf_s}{tier}{fr}: {m.short()}")
        if self.preview_lock:
            lines.append("  (Hinekora's Lock: next currency outcome can be foreseen)")
        if self.links:
            lines.append(f"  {self.links}-linked")
        return "\n".join(lines)
