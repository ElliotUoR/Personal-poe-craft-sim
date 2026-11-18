"""Statistics over recipe runs, exact/MC cross-check, and recipe comparison."""
from __future__ import annotations

import math
import statistics
from dataclasses import dataclass, field

from .engine import LOST, Engine, ExactTooLarge
from .lang import Program, parse_file


@dataclass
class Report:
    name: str
    method: str  # "exact" | "monte carlo" | "exact+mc"
    runs: int
    p_success: float
    mean_chaos: float
    std_chaos: float
    mean_counts: dict
    divine: float
    percentiles: dict = field(default_factory=dict)  # MC only
    mean_lost: float = 0.0
    p_lose_base: float | None = None  # MC only: P(at least one bought base discarded)
    exact: dict | None = None
    mc: dict | None = None
    agreement: str | None = None
    missing_prices: list = field(default_factory=list)
    example_item: str | None = None
    tracks: dict = field(default_factory=dict)

    @property
    def cost_per_success(self):
        return self.mean_chaos / self.p_success if self.p_success > 0 else math.inf

    def to_dict(self):
        d = dict(self.__dict__)
        d["cost_per_success"] = self.cost_per_success
        return d

    def text(self, prices=None):
        dv = self.divine
        L = [f"== {self.name}  [{self.method}{', %d runs' % self.runs if self.runs else ''}]",
             f"  success probability : {self.p_success:.2%}",
             f"  expected cost       : {self.mean_chaos:,.0f} chaos  ({self.mean_chaos / dv:.2f} divine)",
             f"  std dev             : {self.std_chaos:,.0f} chaos  ({self.std_chaos / dv:.2f} divine)",
             f"  cost per success    : {self.cost_per_success:,.0f} chaos  ({self.cost_per_success / dv:.2f} divine)"]
        if self.percentiles:
            L.append("  cost percentiles    : " + ", ".join(f"p{k} {v / dv:.1f}div" for k, v in self.percentiles.items()))
        if self.mean_lost:
            s = f"  bases lost          : {self.mean_lost:,.0f} chaos expected"
            if self.p_lose_base is not None:
                s += f"; P(lose at least one bought base) = {self.p_lose_base:.1%}"
            L.append(s)
        L.append("  expected currency used:")
        for k, v in sorted(self.mean_counts.items(), key=lambda kv: -kv[1] * (prices[kv[0]] if prices else 1)):
            if k.startswith("_") or v < 1e-9:
                continue
            c = f"  = {v * prices[k]:,.0f}c" if prices else ""
            L.append(f"    {v:10.3f} x {k}{c}")
        for lbl, pr in self.tracks.items():
            L.append(f"  P({lbl}) = {pr:.2%}")
        if self.agreement:
            L.append(f"  exact vs monte carlo: {self.agreement}")
        if self.missing_prices:
            L.append(f"  WARNING: no price for {', '.join(sorted(self.missing_prices))} (counted as 0)")
        return "\n".join(L)


def mc_stats(engine: Engine, prog: Program, runs=2000, seed=0):
    totals, succ, lost, lose_any = [], 0, [], 0
    counts = {}
    example = None
    tracks = {lbl: 0 for lbl, _c in prog.tracks}
    for i in range(runs):
        r = engine.run_mc(prog, seed=seed * 1_000_003 + i)
        c = engine.prices.chaos(r.costs)
        totals.append(c)
        succ += r.success
        lv = r.costs.get(LOST, 0.0)
        lost.append(lv)
        lose_any += lv > 0
        for k, v in r.costs.items():
            counts[k] = counts.get(k, 0.0) + v
        for k, v in r.tracks.items():
            tracks[k] += v
        if r.success and example is None and r.item is not None:
            example = r.item.describe(engine.gd)
    counts = {k: v / runs for k, v in counts.items()}
    s = sorted(totals)

    def pct(q):
        return s[min(len(s) - 1, int(q / 100 * len(s)))]

    return {
        "p_success": succ / runs,
        "mean_chaos": statistics.fmean(totals),
        "std_chaos": statistics.pstdev(totals) if runs > 1 else 0.0,
        "se_mean": (statistics.pstdev(totals) / math.sqrt(runs)) if runs > 1 else 0.0,
        "mean_counts": counts,
        "percentiles": {50: pct(50), 90: pct(90), 99: pct(99)},
        "mean_lost": statistics.fmean(lost),
        "p_lose_base": lose_any / runs,
        "example": example,
        "runs": runs,
        "tracks": {k: v / runs for k, v in tracks.items()},
    }


def exact_stats(engine: Engine, prog: Program, **budget):
    r = engine.run_exact(prog, **budget)
    mean = sum(engine.prices[k] * v for k, v in r.mean_counts.items())
    var = max(0.0, r.second_moment - mean * mean)
    return {"p_success": r.p_success, "mean_chaos": mean, "std_chaos": math.sqrt(var),
            "mean_counts": r.mean_counts, "mean_lost": r.mean_counts.get(LOST, 0.0), "tracks": r.tracks,
            "n_outcomes": len(r.outcomes)}


def analyse(engine: Engine, prog: Program, runs=2000, mode="auto", seed=0, name=None, exact_seconds=30):
    """mode: auto (exact if small enough, plus MC cross-check), exact, mc, both."""
    engine.prices.missing.clear()
    name = name or prog.name
    ex = mc = None
    exact_note = None
    if mode in ("auto", "exact", "both"):
        try:
            ex = exact_stats(engine, prog, max_seconds=exact_seconds)
        except ExactTooLarge as e:
            if mode in ("exact",):
                raise
            ex = None
            exact_note = f"exact engine skipped ({e}); using Monte Carlo"
    if mode in ("mc", "both", "auto") or ex is None:
        mc = mc_stats(engine, prog, runs=runs, seed=seed)
    agreement = exact_note
    if ex and mc:
        agreement = check_agreement(ex, mc)
    main = ex or mc
    rep = Report(
        name=name, method="exact" + ("+mc" if (ex and mc) else "") if ex else "monte carlo",
        runs=mc["runs"] if mc else 0,
        p_success=main["p_success"], mean_chaos=main["mean_chaos"], std_chaos=main["std_chaos"],
        mean_counts=main["mean_counts"], divine=engine.prices.divine,
        percentiles=mc["percentiles"] if mc else {}, mean_lost=main["mean_lost"],
        p_lose_base=mc["p_lose_base"] if mc else None, exact=ex, mc=mc, agreement=agreement,
        missing_prices=sorted(engine.prices.missing), example_item=mc["example"] if mc else None,
        tracks=main.get("tracks", {}))
    return rep


def check_agreement(ex, mc, z=4.0):
    """Exact mean must lie within z standard errors of the MC mean (and success
    probabilities within z binomial standard errors)."""
    n = mc["runs"]
    se = mc["se_mean"] or 1e-9
    zc = abs(ex["mean_chaos"] - mc["mean_chaos"]) / se
    p = ex["p_success"]
    sep = math.sqrt(max(p * (1 - p), 1e-12) / n)
    zp = abs(p - mc["p_success"]) / sep
    ok = zc <= z and zp <= z
    return (f"{'AGREE' if ok else 'DISAGREE'} (mean: exact {ex['mean_chaos']:,.1f} vs mc {mc['mean_chaos']:,.1f}, "
            f"z={zc:.1f}; success: exact {p:.3f} vs mc {mc['p_success']:.3f}, z={zp:.1f})")


def compare(engine: Engine, paths, runs=2000, mode="auto", sort="cost_per_success"):
    reps = []
    for p in paths:
        prog = parse_file(p)
        reps.append(analyse(engine, prog, runs=runs, mode=mode))
    key = {"cost_per_success": lambda r: r.cost_per_success, "mean": lambda r: r.mean_chaos,
           "risk": lambda r: (1 - r.p_success, r.mean_lost), "std": lambda r: r.std_chaos}[sort]
    reps.sort(key=key)
    return reps


def compare_table(reps):
    dv = reps[0].divine if reps else 1
    hdr = (f"{'#':>2} {'recipe':28} {'success':>8} {'E[cost]':>9} {'per success':>12} {'std':>8} "
           f"{'p90':>8} {'E[lost base]':>12} {'P(lose base)':>12}")
    L = [hdr, "-" * len(hdr)]
    for i, r in enumerate(reps, 1):
        p90 = f"{r.percentiles[90] / dv:.1f}" if r.percentiles else "-"
        pl = f"{r.p_lose_base:.1%}" if r.p_lose_base is not None else "-"
        L.append(f"{i:>2} {r.name[:28]:28} {r.p_success:>8.1%} {r.mean_chaos / dv:>8.2f}d {r.cost_per_success / dv:>11.2f}d "
                 f"{r.std_chaos / dv:>7.2f}d {p90:>7}d {r.mean_lost / dv:>11.2f}d {pl:>12}")
    L.append("(d = divine orbs; 'per success' = expected cost / success probability, i.e. the cost if you "
             "restart the whole recipe after a failure)")
    return "\n".join(L)
