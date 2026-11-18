"""Command line interface. Run ``python -m poecraft --help``."""
from __future__ import annotations

import argparse
import json
import sys

from .analysis import analyse, compare, compare_table
from .data import DataError, default_data
from .engine import Engine
from .lang import RecipeError, parse_file
from .prices import Prices


def _engine(a):
    prices = Prices.load(*(a.prices or []))
    return Engine(default_data(), prices)


def cmd_run(a):
    eng = _engine(a)
    prog = parse_file(a.recipe)
    rep = analyse(eng, prog, runs=a.runs, mode=a.mode, seed=a.seed)
    if a.json:
        print(json.dumps(rep.to_dict(), indent=1, default=str))
    else:
        print(rep.text(eng.prices))
        if a.show_item and rep.example_item:
            print("  example finished item:\n    " + rep.example_item.replace("\n", "\n    "))


def cmd_compare(a):
    eng = _engine(a)
    reps = compare(eng, a.recipes, runs=a.runs, mode=a.mode, sort=a.sort)
    if a.json:
        print(json.dumps([r.to_dict() for r in reps], indent=1, default=str))
        return
    print(compare_table(reps))
    if a.verbose:
        for r in reps:
            print()
            print(r.text(eng.prices))


def cmd_sample(a):
    eng = _engine(a)
    prog = parse_file(a.recipe)
    trace = []
    r = eng.run_mc(prog, seed=a.seed, trace=trace)
    for t in trace:
        print("--", t)
    print("== result:", "SUCCESS" if r.success else f"FAILED {r.msg}")
    if r.item:
        print(r.item.describe(eng.gd))
    print("cost:", f"{eng.prices.chaos(r.costs):,.0f} chaos", {k: round(v, 3) for k, v in r.costs.items()})


def cmd_check(a):
    eng = _engine(a)
    prog = parse_file(a.recipe)
    for s in range(a.runs):
        eng.run_mc(prog, seed=s)
    print(f"OK: {a.recipe} parsed and ran {a.runs} time(s) without errors")


def cmd_mods(a):
    gd = default_data()
    base = gd.resolve_base(a.base)
    infl = frozenset(x for x in (a.influence or "").split(",") if x)
    pool = gd.spawn_pool(base, infl, a.ilvl)
    tot = {"prefix": 0, "suffix": 0}
    for mid, w in pool:
        tot[gd.mods[mid].gen] += w
    rows = []
    for mid, w in pool:
        m = gd.mods[mid]
        if a.side and m.gen != a.side:
            continue
        if a.search and a.search.lower() not in m.text.lower() and a.search.lower() not in mid.lower():
            continue
        if a.tag and a.tag not in m.tags:
            continue
        rows.append((m.gen, m.influence or "", m.group, -m.level, mid, w, m))
    rows.sort()
    print(f"{gd.bases[base]['name']} ilvl {a.ilvl} influences {sorted(infl) or 'none'}: "
          f"total prefix weight {tot['prefix']}, suffix weight {tot['suffix']}")
    for gen, inf, grp, _l, mid, w, m in rows:
        t = gd.tier(mid)
        print(f"  {gen[:3]} {inf[:8]:8} T{t:<2} w={w:<5} {100 * w / tot[gen]:5.2f}%  lvl{m.level:<3} {m.short()[:70]}  "
              f"[{mid}] tags={','.join(m.tags)}")
    if a.elevated:
        print("elevated versions:")
        for key, fam in gd.families.items():
            if fam.elevated and (not infl or key[2] in infl) and gd._can_spawn_on_class(fam.elevated, gd.item_class(base)):
                print(f"  {key[2]:8} {key[0]:6} {gd.mods[fam.elevated].short()[:90]}")


def cmd_prices(a):
    p = Prices.load(*(a.prices or []))
    print(json.dumps({k: v for k, v in p.meta.items()}, indent=1))
    for k in sorted(p.table):
        if not a.grep or a.grep.lower() in k.lower():
            print(f"  {k:45} {p.table[k]}")


def main(argv=None):
    ap = argparse.ArgumentParser(prog="poecraft", description="Path of Exile 1 crafting simulator / calculator")
    ap.add_argument("--prices", action="append", help="extra price JSON file(s) merged over data/prices.json")
    sub = ap.add_subparsers(dest="cmd", required=True)

    r = sub.add_parser("run", help="evaluate one recipe (exact if small, else Monte Carlo)")
    r.add_argument("recipe")
    r.add_argument("--runs", type=int, default=2000)
    r.add_argument("--mode", choices=["auto", "exact", "mc", "both"], default="auto")
    r.add_argument("--seed", type=int, default=0)
    r.add_argument("--json", action="store_true")
    r.add_argument("--show-item", action="store_true")
    r.set_defaults(fn=cmd_run)

    c = sub.add_parser("compare", help="rank several recipes against the same prices")
    c.add_argument("recipes", nargs="+")
    c.add_argument("--runs", type=int, default=2000)
    c.add_argument("--mode", choices=["auto", "exact", "mc", "both"], default="auto")
    c.add_argument("--sort", choices=["cost_per_success", "mean", "risk", "std"], default="cost_per_success")
    c.add_argument("--json", action="store_true")
    c.add_argument("-v", "--verbose", action="store_true")
    c.set_defaults(fn=cmd_compare)

    s = sub.add_parser("sample", help="run a recipe once and print the item (and 'show' statements)")
    s.add_argument("recipe")
    s.add_argument("--seed", type=int, default=None)
    s.set_defaults(fn=cmd_sample)

    k = sub.add_parser("check", help="parse a recipe and run it a few times to catch errors")
    k.add_argument("recipe")
    k.add_argument("--runs", type=int, default=20)
    k.set_defaults(fn=cmd_check)

    m = sub.add_parser("mods", help="list the mods that can roll on a base, with weights")
    m.add_argument("base")
    m.add_argument("--influence", help="comma separated, e.g. hunter,warlord")
    m.add_argument("--ilvl", type=int, default=86)
    m.add_argument("--side", choices=["prefix", "suffix"])
    m.add_argument("--search")
    m.add_argument("--tag", help="only mods with this tag (e.g. caster, critical, speed)")
    m.add_argument("--elevated", action="store_true", help="also list elevated mods")
    m.set_defaults(fn=cmd_mods)

    p = sub.add_parser("prices", help="show the price table")
    p.add_argument("--grep")
    p.set_defaults(fn=cmd_prices)

    a = ap.parse_args(argv)
    try:
        a.fn(a)
    except (RecipeError, DataError) as e:
        print(f"error: {e}", file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main()
