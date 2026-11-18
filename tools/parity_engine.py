"""Cross-check the Rust engine against the Python reference on every recipe in recipes/.

For each recipe both engines run (exact where small enough, otherwise Monte Carlo):
* both exact      -> means, std and success must match to 1e-6 relative;
* otherwise       -> z-test on the mean cost and success probability using the standard
                     errors of whichever sides are Monte Carlo (|z| <= 4 passes).

    python tools/parity_engine.py [--runs N] [recipe ...]
"""
import argparse
import glob
import json
import math
import os
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.path.join(ROOT, "target", "release", "poecraft.exe" if os.name == "nt" else "poecraft")


def run_json(cmd):
    out = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", cwd=ROOT)
    if out.returncode != 0:
        raise RuntimeError(out.stderr.strip() or out.stdout.strip())
    return json.loads(out.stdout)


def side(rep):
    """(mean, se_mean, p, n_for_p or None if exact, std, label)"""
    if rep.get("exact"):
        e = rep["exact"]
        return e["mean_chaos"], 0.0, e["p_success"], None, e["std_chaos"], "exact"
    m = rep["mc"]
    se = m.get("se_mean") or m["std_chaos"] / math.sqrt(max(m["runs"], 1))
    return m["mean_chaos"], se, m["p_success"], m["runs"], m["std_chaos"], f"mc{m['runs']}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("recipes", nargs="*")
    ap.add_argument("--runs", type=int, default=3000)
    a = ap.parse_args()
    recipes = a.recipes or sorted(p for p in glob.glob(os.path.join(ROOT, "recipes", "**", "*.craft"), recursive=True)
                                  if os.sep + "lib" + os.sep not in p)
    bad = 0
    for path in recipes:
        rel = os.path.relpath(path, ROOT)
        t0 = time.time()
        try:
            py = run_json([sys.executable, "-m", "poecraft", "run", rel, "--json", "--runs", str(a.runs)])
        except RuntimeError as e:
            print(f"SKIP {rel}: python error: {e}")
            continue
        t1 = time.time()
        rs = run_json([EXE, "run", rel, "--json", "--runs", str(a.runs * 4)])
        t2 = time.time()
        pm, pse, pp, pn, pstd, pl = side(py)
        rm, rse, rp, rn, rstd, rl = side(rs)
        if pn is None and rn is None:
            ok = (abs(pm - rm) <= 1e-6 * max(1, abs(pm)) and abs(pp - rp) <= 1e-9
                  and abs(pstd - rstd) <= 1e-6 * max(1, pstd))
            detail = f"mean {pm:.4f} vs {rm:.4f}, std {pstd:.3f} vs {rstd:.3f}, p {pp:.6f} vs {rp:.6f}"
        else:
            zc = abs(pm - rm) / max(math.hypot(pse, rse), 1e-9)
            var_p = sum(p * (1 - p) / n for p, n in ((pp, pn), (rp, rn)) if n)
            zp = abs(pp - rp) / max(math.sqrt(var_p), 1e-9)
            ok = zc <= 4 and zp <= 4
            detail = f"mean {pm:,.1f} vs {rm:,.1f} (z={zc:.1f}), p {pp:.4f} vs {rp:.4f} (z={zp:.1f})"
        bad += not ok
        print(f"{'ok ' if ok else 'BAD'} {rel:55} py[{pl}] {t1 - t0:5.1f}s  rust[{rl}] {t2 - t1:5.2f}s  {detail}")
    print("ENGINE PARITY OK" if not bad else f"{bad} MISMATCHES")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
