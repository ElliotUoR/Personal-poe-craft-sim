"""Cross-check the Rust data layer against the Python reference.

For a spread of bases, influence sets and item levels, compares the mod pool (ids, weights,
per-base tiers) from `poecraft pool` (Rust) with poecraft.data (Python). Also compares
mod-name resolution for the aliases and some ambiguous references.

    python tools/parity_data.py [path/to/poecraft.exe]
"""
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from poecraft.data import DataError, default_data  # noqa: E402

EXE = sys.argv[1] if len(sys.argv) > 1 else os.path.join("target", "release", "poecraft.exe")
gd = default_data()

CASES = [
    ("Astral Plate", "", 86), ("Astral Plate", "hunter,warlord", 86), ("Royal Plate", "warlord", 84),
    ("Two-Toned Boots", "hunter,redeemer", 84), ("Titan Greaves", "crusader", 75),
    ("Two-Stone Ring", "hunter", 86), ("Onyx Amulet", "shaper", 84), ("Stygian Vise", "elder", 86),
    ("Imbued Wand", "", 84), ("Vaal Axe", "warlord", 86), ("Thicket Bow", "", 85),
    ("Cobalt Jewel", "", 84), ("Searching Eye Jewel", "", 84), ("Divine Life Flask", "", 84),
    ("Gold Flask", "", 85), ("Prismatic Tincture", "", 82), ("Spike-Point Arrow Quiver", "redeemer", 86),
    ("Archon Kite Shield", "", 86), ("Hubris Circlet", "elder", 86), ("Sorcerer Gloves", "hunter", 82),
]


def rust_pool(base, infl, ilvl):
    args = [EXE, "pool", base, "--ilvl", str(ilvl)] + (["--influence", infl] if infl else [])
    out = subprocess.run(args, capture_output=True, text=True, encoding="utf-8", check=True).stdout
    return {line.split("\t")[0]: (int(line.split("\t")[1]), int(line.split("\t")[2]))
            for line in out.splitlines() if line.strip()}


def py_pool(base, infl, ilvl):
    b = gd.resolve_base(base)
    return {m: (w, gd.tier(m, b)) for m, w in gd.spawn_pool(b, frozenset(x for x in infl.split(",") if x), ilvl)}


bad = 0
for base, infl, ilvl in CASES:
    try:
        p = py_pool(base, infl, ilvl)
    except DataError as e:
        print(f"SKIP {base}: {e}")
        continue
    r = rust_pool(base, infl, ilvl)
    if p != r:
        bad += 1
        only_p = sorted(set(p) - set(r))[:5]
        only_r = sorted(set(r) - set(p))[:5]
        diff = [(k, p[k], r[k]) for k in sorted(set(p) & set(r)) if p[k] != r[k]][:5]
        print(f"MISMATCH {base} {infl} {ilvl}: only py {only_p} only rust {only_r} diff {diff}")
    else:
        print(f"ok  {base:26} {infl or '-':18} ilvl {ilvl}: {len(p)} mods")

REFS = list(gd.aliases) + ["increased maximum life", "Warlord: total mana cost", "suffix: to Fire Resistance",
                           "Hunter: Attacks have", "nonsense mod text"]
for ref in REFS:
    for cls in ("Body Armour", "Boots", "Ring"):
        try:
            pl = gd.family_label(gd.resolve_mod(ref, cls))
        except DataError as e:
            pl = "ERROR " + str(e).split("\n")[0][:60]
        out = subprocess.run([EXE, "resolve", ref, "--class", cls], capture_output=True, text=True,
                             encoding="utf-8").stdout.strip().split("\n")[0]
        rl = out if not out.startswith("ERROR") else "ERROR " + out[6:][:60]
        if pl != rl:
            bad += 1
            print(f"RESOLVE MISMATCH {ref!r} {cls}: py={pl!r} rust={rl!r}")
print("PARITY OK" if not bad else f"{bad} MISMATCHES")
sys.exit(1 if bad else 0)
