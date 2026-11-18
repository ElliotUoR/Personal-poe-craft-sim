"""Price table: chaos value per price key.

Default file: data/prices.json. Override with --prices FILE (merged on top) or
data/prices.local.json (git-ignored, also merged on top if present).

Price keys:
  chaos, divine, exalt, annul, alt, aug, regal, transmute, alch, scour, blessed,
  awakener, dominance, hunter_exalt, warlord_exalt, redeemer_exalt, crusader_exalt,
  fracture, veiled_exalt, eldritch_chaos, chromatic, jeweller, fusing,
  lifeforce_wild / lifeforce_vivid / lifeforce_primal / lifeforce_sacred (per unit),
  beast:<Beast Name>, any_rare_beast, omen_blanching, essence:<Essence name>,
  and any "base:..." key a recipe uses with `buy ... for price "base:..."`.
"""
from __future__ import annotations

import json
import os

from .data import DATA_DIR

# Recipe-friendly aliases for `pay N <unit>` / `buy ... for N <unit>`.
UNIT_ALIASES = {"c": "chaos", "chaos": "chaos", "div": "divine", "divine": "divine", "divines": "divine",
                "ex": "exalt", "exalt": "exalt"}


class PriceError(Exception):
    pass


class Prices:
    def __init__(self, table: dict, meta=None):
        self.table = dict(table)
        self.meta = meta or {}
        self.missing = set()

    @classmethod
    def load(cls, *override_paths, include_local=True):
        path = os.path.join(DATA_DIR, "prices.json")
        with open(path, encoding="utf-8") as f:
            raw = json.load(f)
        table = dict(raw["prices"])
        meta = {k: v for k, v in raw.items() if k != "prices"}
        paths = list(override_paths)
        local = os.path.join(DATA_DIR, "prices.local.json")
        if include_local and os.path.exists(local):
            paths.insert(0, local)
        for p in paths:
            if not p:
                continue
            with open(p, encoding="utf-8") as f:
                o = json.load(f)
            table.update(o.get("prices", o))
            meta.setdefault("overrides", []).append(p)
        return cls(table, meta)

    def __getitem__(self, key):
        key = UNIT_ALIASES.get(key, key)
        if key.startswith("_"):
            return 0.0
        if key not in self.table:
            self.missing.add(key)
            return 0.0
        return float(self.table[key])

    def chaos(self, cost: dict) -> float:
        return sum(self[k] * n for k, n in cost.items())

    @property
    def divine(self):
        return self["divine"]
