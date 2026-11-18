"""Fetch current PoE1 prices from poe.ninja into data/prices.json (spec §4.G).

    python tools/fetch_prices.py [--league Covens] [--dry-run]

Sources (poe.ninja, checked 2026-11-18):
* exchange items (currency, omens, essences, fossils, ...):
  /poe1/api/economy/exchange/current/overview?league=L&type=T   -> lines[].primaryValue (chaos)
* beasts and base types (stash listings):
  /poe1/api/economy/stash/current/item/overview?league=L&type=Beast|BaseType -> lines[].chaosValue
  BaseType lines carry levelRequired (= item level) and variant (= influences).
The stash *currency* endpoint is NOT used: it reported Divine = 127.5c while the exchange said
226.2c on 2026-11-18.

Keys already in prices.json that poe.ninja does not cover (e.g. any_rare_beast, omen_blanching
when unlisted) are kept and stay listed under "estimates". Requests identify the client only as
"(ej)".
"""
import argparse
import datetime as dt
import json
import os
import sys
import urllib.parse
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BASE = "https://poe.ninja/poe1/api"
UA = {"User-Agent": "poecraft/0.1 (ej)"}

# our price key -> poe.ninja display name
CURRENCY = {
    "chaos": "Chaos Orb", "divine": "Divine Orb", "exalt": "Exalted Orb", "annul": "Orb of Annulment",
    "alt": "Orb of Alteration", "aug": "Orb of Augmentation", "regal": "Regal Orb", "transmute": "Orb of Transmutation",
    "alch": "Orb of Alchemy", "scour": "Orb of Scouring", "blessed": "Blessed Orb", "sacred": "Sacred Orb",
    "awakener": "Awakener's Orb", "dominance": "Orb of Dominance", "hunter_exalt": "Hunter's Exalted Orb",
    "warlord_exalt": "Warlord's Exalted Orb", "redeemer_exalt": "Redeemer's Exalted Orb",
    "crusader_exalt": "Crusader's Exalted Orb", "fracture": "Fracturing Orb", "veiled_exalt": "Veiled Exalted Orb",
    "veiled_chaos": "Veiled Chaos Orb", "foulborn_exalt": "Foulborn Exalted Orb", "eldritch_chaos": "Eldritch Chaos Orb",
    "chromatic": "Chromatic Orb", "jeweller": "Jeweller's Orb", "fusing": "Orb of Fusing", "regret": "Orb of Regret",
    "chance": "Orb of Chance", "vaal": "Vaal Orb", "armourer_scrap": "Armourer's Scrap", "glassblower": "Glassblower's Bauble",
    "gcp": "Gemcutter's Prism", "lifeforce_wild": "Wild Crystallised Lifeforce", "lifeforce_vivid": "Vivid Crystallised Lifeforce",
    "lifeforce_primal": "Primal Crystallised Lifeforce", "lifeforce_sacred": "Sacred Crystallised Lifeforce",
    "hinekoras_lock_orb": "Hinekora's Lock", "omen_blanching": "Omen of Blanching",
}
EXCHANGE_TYPES = ["Currency", "Omen", "Essence", "Fossil", "Fragment"]

# recipe base keys: (base name or None for any base of the class, item class, influences, min ilvl)
BASE_KEYS = {
    "base:astral_plate_warlord_86": ("Astral Plate", None, "Warlord", 86),
    "base:astral_plate_hunter_86": ("Astral Plate", None, "Hunter", 86),
    "base:astral_plate_hunter_84": ("Astral Plate", None, "Hunter", 84),
    "base:boots_hunter_75": (None, "Boots", "Hunter", 84),
    "base:boots_redeemer_80": (None, "Boots", "Redeemer", 84),
}


def get(path, **q):
    url = f"{BASE}/{path}?{urllib.parse.urlencode(q)}"
    with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=60) as r:
        return json.load(r)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--league")
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()
    league = a.league or get("data/index-state")["economyLeagues"][0]["name"]
    print(f"league: {league}", file=sys.stderr)

    by_name = {}
    for t in EXCHANGE_TYPES:
        try:
            x = get("economy/exchange/current/overview", league=league, type=t)
        except Exception as e:  # a type may not exist in every league
            print(f"  skip exchange {t}: {e}", file=sys.stderr)
            continue
        names = {i["id"]: i["name"] for i in x.get("items", [])}
        for line in x.get("lines", []):
            nm = names.get(line["id"])
            if nm and line.get("primaryValue") is not None:
                by_name[nm] = float(line["primaryValue"])
        print(f"  exchange {t}: {len(x.get('lines', []))} items", file=sys.stderr)

    beasts = get("economy/stash/current/item/overview", league=league, type="Beast")["lines"]
    bases = get("economy/stash/current/item/overview", league=league, type="BaseType")["lines"]
    print(f"  beasts {len(beasts)}, base types {len(bases)}", file=sys.stderr)

    path = os.path.join(ROOT, "data", "prices.json")
    old = json.load(open(path, encoding="utf-8"))
    prices = dict(old["prices"])
    estimates = dict(old.get("estimates", {}))
    found = 0
    for key, name in CURRENCY.items():
        if name in by_name:
            prices[key] = round(by_name[name], 4)
            estimates.pop(key, None)
            found += 1
    for b in beasts:
        prices[f"beast:{b['name']}"] = b["chaosValue"]
    for nm, v in by_name.items():
        if nm.startswith("Omen of "):
            prices[f"omen:{nm[len('Omen of '):]}"] = v
        if "Essence" in nm:
            prices[f"essence:{nm}"] = v
    # bases: generic keys "base:<Name>/<Influences or none>/<ilvl>" and the recipe keys
    for b in bases:
        var = b.get("variant") or "none"
        prices[f"base:{b['name']}/{var}/{b.get('levelRequired')}"] = b["chaosValue"]
    for key, (name, cls, infl, ilvl) in BASE_KEYS.items():
        # poe.ninja "itemType" is the item class
        cands = [b["chaosValue"] for b in bases
                 if (b["name"] == name if name else b.get("itemType") == cls)
                 and (b.get("variant") or "") == infl and (b.get("levelRequired") or 0) >= ilvl]
        if cands:
            prices[key] = min(cands)
            estimates[key] = f"cheapest poe.ninja listing: {name or 'any ' + cls}, {infl}, ilvl {ilvl}+ (stash data, {len(cands)} variants)"
    out = {
        "_doc": old.get("_doc", ""),
        "source": f"poe.ninja PoE1 API (exchange overview for currency/omens/essences/fossils; stash item overview for beasts and base types), league {league}",
        "league": league,
        "fetched": dt.date.today().isoformat(),
        "estimates": estimates,
        "prices": dict(sorted(prices.items())),
    }
    print(f"  {found}/{len(CURRENCY)} currency keys found; divine = {prices.get('divine')}c", file=sys.stderr)
    missing = [k for k, n in CURRENCY.items() if n not in by_name]
    if missing:
        print(f"  not on poe.ninja (kept old values): {', '.join(missing)}", file=sys.stderr)
    if a.dry_run:
        print(json.dumps({k: prices[k] for k in list(CURRENCY) + list(BASE_KEYS) if k in prices}, indent=1))
        return
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(out, f, indent=1, ensure_ascii=False)
    print(f"wrote {path}", file=sys.stderr)


if __name__ == "__main__":
    main()
