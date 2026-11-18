"""Build the compact game-data file ``data/gamedata.json`` from a RePoE export.

Source: https://github.com/repoe-fork/repoe-fork.github.io (the hosted RePoE
export; the older lvlvllvlvllvlvl/RePoE repo is deprecated and points there).

Pinned version used for the committed data file:
    repoe-fork.github.io commit 585b4b8433079e514bbd6095c7a5af0480c9d824
    exported game version 3.30.0.2.2 (fetched 2026-11-18)

How to refresh (GitHub is reachable from the shell even when other sites are not):

    git clone --depth 1 --filter=blob:none --no-checkout \
        https://github.com/repoe-fork/repoe-fork.github.io /tmp/repoe
    cd /tmp/repoe && mkdir -p /tmp/repoe_data && for f in mods base_items \
        essences crafting_bench_options item_classes; do \
        git show HEAD:data/$f.json > /tmp/repoe_data/$f.json; done
    python tools/build_data.py /tmp/repoe_data --version "$(git show HEAD:version.txt)" \
        --commit "$(git rev-parse HEAD)"

Only the item classes the simulator supports are kept (armour pieces), which keeps
the file small enough to commit.
"""
from __future__ import annotations

import argparse
import datetime as _dt
import json
import os
import sys

# Every equipment class with influence tags, plus jewels, flasks and tinctures.
# Cluster jewels (affliction_jewel) use a separate notable system and are not included yet.
CLASSES = [
    "Body Armour", "Boots", "Gloves", "Helmet", "Shield", "Quiver",
    "Amulet", "Ring", "Belt",
    "Claw", "Dagger", "Rune Dagger", "Wand", "One Hand Sword", "Thrusting One Hand Sword",
    "One Hand Axe", "One Hand Mace", "Sceptre", "Bow", "Staff", "Warstaff",
    "Two Hand Sword", "Two Hand Axe", "Two Hand Mace",
    "Jewel", "AbyssJewel",
    "LifeFlask", "ManaFlask", "HybridFlask", "UtilityFlask", "Tincture",
]
# A mod can only roll on a base of the same RePoE domain (mods.json "domain" vs
# base_items.json "domain"). Source: RePoE docs/mods.md.
DOMAINS = {"item", "flask", "jewel", "abyss_jewel", "tincture"}

# RePoE cost item ids -> our price keys.
CURRENCY_IDS = {
    "Metadata/Items/Currency/CurrencyModValues": "divine",
    "Metadata/Items/Currency/CurrencyRerollImplicit": "blessed",
    "Metadata/Items/Currency/CurrencyAddModToRare": "exalt",
    "Metadata/Items/Currency/CurrencyRerollRare": "chaos",
    "Metadata/Items/Currency/CurrencyRerollMagic": "alt",
    "Metadata/Items/Currency/CurrencyAddModToMagic": "aug",
    "Metadata/Items/Currency/CurrencyUpgradeToRare": "alch",
    "Metadata/Items/Currency/CurrencyUpgradeMagicToRare": "regal",
    "Metadata/Items/Currency/CurrencyUpgradeToMagic": "transmute",
    "Metadata/Items/Currency/CurrencyRemoveMod": "annul",
    "Metadata/Items/Currency/CurrencyConvertToNormal": "scour",
    "Metadata/Items/Currency/CurrencyRerollDefences": "armourer_scrap",
    "Metadata/Items/Currency/CurrencyArmourQuality": "armourer_scrap",
    "Metadata/Items/Currency/CurrencyRerollSocketColours": "chromatic",
    "Metadata/Items/Currency/CurrencyRerollSocketNumbers": "jeweller",
    "Metadata/Items/Currency/CurrencyRerollSocketLinks": "fusing",
    "Metadata/Items/Currency/CurrencyPassiveRefund": "regret",
    "Metadata/Items/Currency/CurrencyUpgradeRandomly": "chance",
    "Metadata/Items/Currency/CurrencyCorrupt": "vaal",
    "Metadata/Items/Currency/CurrencyDuplicate": "mirror",
}


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("repoe_dir")
    ap.add_argument("--version", default="unknown")
    ap.add_argument("--commit", default="unknown")
    ap.add_argument("--out", default=os.path.join(os.path.dirname(__file__), "..", "data", "gamedata.json"))
    a = ap.parse_args(argv)

    def load(name):
        with open(os.path.join(a.repoe_dir, name + ".json"), encoding="utf-8") as f:
            return json.load(f)

    mods = load("mods")
    bases = load("base_items")
    essences = load("essences")
    bench = load("crafting_bench_options")
    item_classes = load("item_classes")

    out_classes = {}
    relevant_tags = set()
    for cls in CLASSES:
        tags = item_classes.get(cls, {}).get("influence_tags") or []
        out_classes[cls] = {"influence_tags": tags}
        relevant_tags.update(tags)

    out_bases = {}
    for bid, b in bases.items():
        if b.get("item_class") not in CLASSES or b.get("domain") not in DOMAINS:
            continue
        if b.get("release_state") != "released":
            continue
        out_bases[bid] = {
            "name": b["name"],
            "item_class": b["item_class"],
            "tags": b["tags"],
            "drop_level": b["drop_level"],
            "implicits": b.get("implicits", []),
            "domain": b["domain"],
            "properties": {k: v for k, v in (b.get("properties") or {}).items() if v is not None},
            "requirements": b.get("requirements"),
        }
        relevant_tags.update(b["tags"])
    # "default" matches every base (RePoE: first matching spawn tag wins), so mods that spawn
    # only via "default" must be kept. Dropping it lost e.g. LifeRegeneration1-7 (bug until 2026-11-18).
    relevant_tags.add("default")

    keep = {}

    def slim(mid, m):
        return {
            "name": m["name"],
            "gen": m["generation_type"],
            "groups": m["groups"],
            "type": m["type"],
            "tags": m["implicit_tags"],
            "level": m["required_level"],
            "spawn": [[s["tag"], s["weight"]] for s in m["spawn_weights"]],
            "genw": [[s["tag"], s["weight"]] for s in m.get("generation_weights", [])],
            "text": m.get("text") or "",
            "essence_only": m.get("is_essence_only", False),
            "adds_tags": m.get("adds_tags", []),
            "domain": m["domain"],
            "stats": [[st["id"], st["min"], st["max"]] for st in m.get("stats", [])],
        }

    for mid, m in mods.items():
        if m["domain"] not in DOMAINS or m["generation_type"] not in ("prefix", "suffix"):
            continue
        tags_here = {s["tag"] for s in m["spawn_weights"]}
        spawnable = any(s["weight"] > 0 and s["tag"] in relevant_tags for s in m["spawn_weights"])
        elevated = "Elevated" in m["name"] and (tags_here & {t for c in out_classes.values() for t in c["influence_tags"]})
        if spawnable or elevated:
            keep[mid] = slim(mid, m)

    # Unveiled mods (Betrayal/Jun "Veiled" crafting) and the veiled placeholders.
    for mid, m in mods.items():
        if m["domain"] == "unveiled" and m["generation_type"] in ("prefix", "suffix"):
            if any(sw["weight"] > 0 and sw["tag"] in relevant_tags for sw in m["spawn_weights"]):
                keep[mid] = slim(mid, m)
        if mid in ("VeiledPrefix", "VeiledSuffix"):
            keep[mid] = slim(mid, m)

    for b in out_bases.values():
        for mid in b["implicits"]:
            if mid in mods and mid not in keep:
                keep[mid] = slim(mid, mods[mid])

    out_ess = {}
    for eid, e in essences.items():
        em = {c: e["mods"][c] for c in CLASSES if c in e.get("mods", {})}
        if not em:
            continue
        out_ess[eid] = {"name": e["name"], "level": e["level"], "item_level_restriction": e.get("item_level_restriction"),
                        "mods": em}
        for mid in em.values():
            if mid in mods and mid not in keep:
                keep[mid] = slim(mid, mods[mid])

    out_bench = []
    for o in bench:
        mid = (o.get("actions") or {}).get("add_explicit_mod")
        if not mid:
            continue
        classes = [c for c in o.get("item_classes", []) if c in CLASSES]
        if not classes:
            continue
        cost = {}
        for cid, n in o["cost"].items():
            cost[CURRENCY_IDS.get(cid, cid)] = n
        out_bench.append({"mod": mid, "classes": classes, "cost": cost})
        if mid in mods and mid not in keep:
            keep[mid] = slim(mid, mods[mid])
            keep[mid]["gen"] = mods[mid]["generation_type"]

    data = {
        "source": {
            "repo": "https://github.com/repoe-fork/repoe-fork.github.io",
            "commit": a.commit,
            "game_version": a.version,
            "built": _dt.date.today().isoformat(),
        },
        "classes": out_classes,
        "bases": out_bases,
        "mods": keep,
        "essences": out_ess,
        "bench": out_bench,
    }
    with open(a.out, "w", encoding="utf-8") as f:
        json.dump(data, f, separators=(",", ":"), ensure_ascii=False, sort_keys=True)
    print(f"wrote {a.out}: {len(out_bases)} bases, {len(keep)} mods, {len(out_ess)} essences, {len(out_bench)} bench options",
          file=sys.stderr)


if __name__ == "__main__":
    main()
