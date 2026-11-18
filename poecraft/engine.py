"""Recipe execution: shared primitive actions, Monte Carlo runner and exact engine.

Exact engine
------------
Works on probability distributions over hashable states. Primitive actions are
enumerated exhaustively (every random choice is branched with its probability).
Loops are solved as absorbing Markov chains: transient states are discovered by
running the loop body from each state, then expected currency counts and the
second moment of the chaos cost are obtained from linear systems
(I - Q) X = R. If the number of enumerated paths or chain states exceeds a budget,
``ExactTooLarge`` is raised and the caller falls back to Monte Carlo.
"""
from __future__ import annotations

import math
from collections import defaultdict
from dataclasses import dataclass, field, replace

from . import mechanics as M
from .data import CONQUERORS, INFLUENCES, DataError, GameData, default_data
from .item import MULTIMOD, PREFIX_LOCK, SUFFIX_LOCK, Item, Roll
from .lang import Act, Foresee, If, Loop, Program, RecipeError, Seq
from .prices import UNIT_ALIASES, Prices

ACTION_COST = {
    "transmute": "transmute", "alteration": "alt", "augment": "aug", "regal": "regal", "alchemy": "alch",
    "chaos": "chaos", "exalt": "exalt", "annul": "annul", "scour": "scour", "dominance": "dominance",
    "fracture": "fracture", "veiled_exalt": "veiled_exalt", "awaken": "awakener",
}
LOST = "_lost_chaos"  # pseudo-currency: chaos value of discarded bought bases


class ExactTooLarge(Exception):
    pass


@dataclass(frozen=True)
class State:
    item: Item | None = None
    aside: tuple = ()  # sorted ((name, Item, value), ...)
    imprint: Item | None = None
    status: str = "run"  # run | fail | ok
    msg: str = ""
    value: float = 0.0  # chaos paid to buy the current item (for "lost base" risk)

    def get_aside(self, name):
        for n, it, v in self.aside:
            if n == name:
                return it, v
        return None, 0.0

    def without_aside(self, name):
        return tuple(x for x in self.aside if x[0] != name)


class Engine:
    def __init__(self, gd: GameData = None, prices: Prices = None):
        self.gd = gd or default_data()
        self.prices = prices or Prices.load()
        M.bind(self.gd)

    # ------------------------------------------------------------ helpers
    def _err(self, node, msg):
        return RecipeError(msg, node.line, node.file)

    def _cls(self, item):
        return self.gd.item_class(item.base)

    def _resolve(self, ref, item, line=None, file=None):
        try:
            return self.gd.resolve_mod(ref, self._cls(item) if item else None)
        except DataError as e:
            raise RecipeError(str(e), line, file)

    # ------------------------------------------------------------ conditions
    def cond(self, c, st: State, node=None):
        k = c[0]
        if k == "and":
            return self.cond(c[1], st, node) and self.cond(c[2], st, node)
        if k == "or":
            return self.cond(c[1], st, node) or self.cond(c[2], st, node)
        if k == "not":
            return not self.cond(c[1], st, node)
        if k == "const":
            return c[1]
        item = st.item
        if item is None:
            raise RecipeError("condition checked but there is no current item (missing 'buy'?)",
                              node.line if node else None, node.file if node else None)
        if k == "has":
            _, ref, op, n, elev, line = c
            fam = self._resolve(ref, item, line, node.file if node else None)
            tiers = [self.gd.tier(r.id) for r in item.mods if self.gd.family_key(r.id) == fam]
            if not tiers:
                return False
            t = min(tiers)
            if elev:
                return t == 0
            if op is None:
                return True
            return _cmp(op, t, n)
        if k == "is":
            return item.rarity == c[1]
        if k == "influenced":
            return c[1] in item.influences
        if k == "cmp":
            return _cmp(c[1], self.value(c[2], item, node), self.value(c[3], item, node))
        raise ValueError(c)

    def value(self, v, item, node):
        k = v[0]
        if k == "num":
            return v[1]
        if k == "tier":
            fam = self._resolve(v[1], item, v[2], node.file if node else None)
            tiers = [self.gd.tier(r.id) for r in item.mods if self.gd.family_key(r.id) == fam]
            return min(tiers) if tiers else 99
        if k == "count_infl":
            return sum(1 for r in item.influenced_mods(self.gd) if self.gd.mods[r.id].influence == v[1])
        name = v[1]
        gd = self.gd
        if name == "prefix_count":
            return item.n_prefix(gd)
        if name == "suffix_count":
            return item.n_suffix(gd)
        if name == "mod_count":
            return len(item.mods)
        if name == "open_prefix":
            return item.open_slots(gd, "prefix")
        if name == "open_suffix":
            return item.open_slots(gd, "suffix")
        if name == "influenced_count":
            return len(item.influenced_mods(gd))
        if name == "elevated_count":
            return sum(1 for r in item.mods if gd.mods[r.id].elevated)
        if name == "crafted_count":
            return len(item.crafted_mods())
        if name == "links":
            return item.links
        raise ValueError(v)

    # ------------------------------------------------------------ primitives
    def primitive(self, a: Act, st: State, ch):
        """Execute one primitive action. Returns (new_state, cost_dict)."""
        op = a.op
        try:
            return self._primitive(a, st, ch)
        except M.CraftError as e:
            raise self._err(a, f"{op}: {e}")

    def _need_item(self, a, st):
        if st.item is None:
            raise self._err(a, f"'{a.op}' needs a current item; start with 'buy' (or 'take <name>').")
        return st.item

    def _primitive(self, a: Act, st: State, ch):
        op, args = a.op, a.args
        gd = self.gd
        if op == "buy":
            return self._buy(a, st)
        if op == "pay":
            return st, {UNIT_ALIASES.get(k, k): v for k, v in args["cost"].items()}
        if op == "fail":
            return replace(st, status="fail", msg=args["msg"] or f"fail at line {a.line}"), {}
        if op == "succeed":
            return replace(st, status="ok", msg=args["msg"]), {}
        if op == "show":
            return st, {}
        if op == "set_aside":
            item = self._need_item(a, st)
            if st.get_aside(args["name"])[0] is not None:
                raise self._err(a, f"an item named '{args['name']}' is already set aside")
            aside = tuple(sorted(st.aside + ((args["name"], item, st.value),), key=lambda x: x[0]))
            return replace(st, item=None, aside=aside, value=0.0), {}
        if op == "take":
            it, v = st.get_aside(args["name"])
            if it is None:
                raise self._err(a, f"no set-aside item named '{args['name']}' (set_aside it first)")
            cost = {}
            if st.item is not None:
                raise self._err(a, "'take' needs the current item to be set aside first (set_aside <name>)")
            return replace(st, item=it, aside=st.without_aside(args["name"]), value=v), cost
        item = self._need_item(a, st)
        if op in ("transmute", "alteration", "augment", "regal", "alchemy", "chaos", "exalt", "annul",
                  "dominance", "fracture", "veiled_exalt"):
            fn = {"transmute": M.transmute, "alteration": M.alteration, "augment": M.augment, "regal": M.regal,
                  "alchemy": M.alchemy, "chaos": M.chaos, "exalt": M.exalt, "annul": M.annul,
                  "dominance": M.dominance, "fracture": M.fracture, "veiled_exalt": M.veiled_exalt}[op]
            return replace(st, item=fn(item, ch)), {ACTION_COST[op]: 1}
        if op == "scour":
            return replace(st, item=M.scour(item)), {"scour": 1}
        if op == "remove_crafted":
            return replace(st, item=M.remove_crafted(item)), {}
        if op == "conqueror_exalt":
            inf = args["influence"]
            return replace(st, item=M.conqueror_exalt(item, ch, inf)), {f"{inf}_exalt": 1}
        if op == "awaken":
            donor, dv = st.get_aside(args["donor"])
            if donor is None:
                raise self._err(a, f"awaken: no set-aside item named '{args['donor']}'. "
                                   f"Use 'set_aside {args['donor']}' on the donor item first.")
            new = M.awaken(item, donor, ch)
            return replace(st, item=new, aside=st.without_aside(args["donor"]), value=st.value + dv), {"awakener": 1}
        if op == "harvest_reforge":
            tag = args["tag"]
            res = M.harvest_reforge(item, ch, tag)
            key, n = M.HARVEST_REFORGE[M.HARVEST_TAG.get(tag, tag)]
            return replace(st, item=res), {key: n}
        if op == "essence":
            eid = self._essence(a, args["name"])
            return replace(st, item=M.essence(item, ch, eid)), {f"essence:{gd.essences[eid]['name']}": 1}
        if op == "craft":
            mod_id, cost = self._bench(a, item, args["what"])
            return replace(st, item=M.bench_craft(item, mod_id)), cost
        if op == "beastcraft":
            return self._beast(a, st, item, ch)
        if op == "omen":
            if args["what"] != "blanching":
                raise self._err(a, f"unknown omen '{args['what']}' (supported: blanching)")
            return replace(st, item=M.omen_blanching(item)), {"omen_blanching": 1, "chromatic": 1}
        raise self._err(a, f"unknown action {op}")

    def _buy(self, a, st):
        gd = self.gd
        args = a.args
        try:
            base = gd.resolve_base(args["base"])
        except DataError as e:
            raise self._err(a, str(e))
        for inf in args["influences"]:
            if inf not in INFLUENCES:
                raise self._err(a, f"unknown influence '{inf}' (use one of {', '.join(INFLUENCES)})")
        item = Item(base=base, ilvl=args["ilvl"], rarity=args["rarity"], influences=frozenset(args["influences"]),
                    tag=args.get("tag"))
        mods = []
        for w in args["with"]:
            try:
                fam_key = gd.resolve_mod(w["ref"], gd.item_class(base))
            except DataError as e:
                raise RecipeError(str(e), w["line"], a.file)
            fam = gd.families[fam_key]
            if w["elevated"]:
                if not fam.elevated:
                    raise RecipeError(f'"{w["ref"]}" has no elevated version', w["line"], a.file)
                mid = fam.elevated
            elif w["tier"] is not None:
                if not 1 <= w["tier"] <= len(fam.tiers):
                    raise RecipeError(f'"{w["ref"]}" has tiers 1..{len(fam.tiers)} (asked for tier {w["tier"]}); '
                                      f'levels: ' + ", ".join(f"T{i+1}=ilvl{gd.mods[m].level}" for i, m in enumerate(fam.tiers)),
                                      w["line"], a.file)
                mid = fam.tiers[w["tier"] - 1]
            else:
                ok = [m for m in fam.tiers if gd.mods[m].level <= item.ilvl]
                if not ok:
                    raise RecipeError(f'"{w["ref"]}" needs item level {gd.mods[fam.tiers[-1]].level}+', w["line"], a.file)
                mid = ok[0]
            m = gd.mods[mid]
            if m.influence and m.influence not in item.influences:
                raise RecipeError(f'"{w["ref"]}" is a {m.influence.title()} mod; add "influence {m.influence}" to the buy',
                                  w["line"], a.file)
            if not m.elevated and m.level > item.ilvl:
                raise RecipeError(f'"{m.short()}" (tier {gd.tier(mid)}) needs item level {m.level}, item is {item.ilvl}',
                                  w["line"], a.file)
            if any(g in {g2 for r in mods for g2 in gd.mods[r.id].groups} for g in m.groups):
                raise RecipeError(f'two mods of the same group ({m.group}) on one item', w["line"], a.file)
            mods.append(Roll(mid, fractured=w["fractured"]))
        item = item.with_mods(tuple(mods))
        mx = item.max_affixes()
        if item.n_prefix(gd) > mx or item.n_suffix(gd) > mx:
            raise self._err(a, f"a {item.rarity} item can have at most {mx} prefix(es) and {mx} suffix(es)")
        cost = {UNIT_ALIASES.get(k, k): v for k, v in args["cost"].items()}
        lost = {}
        if st.item is not None and st.value:
            lost = {LOST: st.value}
        value = self.prices.chaos(cost)
        cost.update(lost)
        return replace(st, item=item, value=value, imprint=None if st.item is not None else st.imprint), cost

    def _essence(self, a, name):
        gd = self.gd
        low = name.lower()
        hits = [eid for eid, e in gd.essences.items() if e["name"].lower() == low]
        if not hits:
            hits = [eid for eid, e in gd.essences.items() if low in e["name"].lower()]
        if len(hits) != 1:
            opts = sorted(gd.essences[e]["name"] for e in hits)[:12]
            raise self._err(a, f'essence "{name}" ' + ("not found" if not hits else f"is ambiguous: {', '.join(opts)}"))
        return hits[0]

    BENCH_ALIASES = {"lock_suffixes": SUFFIX_LOCK, "suffixes_cannot_be_changed": SUFFIX_LOCK,
                     "lock_prefixes": PREFIX_LOCK, "prefixes_cannot_be_changed": PREFIX_LOCK,
                     "multimod": MULTIMOD}

    def _bench(self, a, item, what):
        gd = self.gd
        cls = gd.item_class(item.base)
        opts = [b for b in gd.bench if cls in b["classes"]]
        if what in self.BENCH_ALIASES:
            mid = self.BENCH_ALIASES[what]
            for b in opts:
                if b["mod"] == mid:
                    return mid, dict(b["cost"])
            raise self._err(a, f"bench craft {what} not available on {cls}")
        low = what.lower()
        hits = [b for b in opts if low in gd.mods[b["mod"]].text.lower() or low == b["mod"].lower()]
        if not hits:
            raise self._err(a, f'no bench craft matching "{what}" for {cls}. Builtins: {", ".join(self.BENCH_ALIASES)}')
        best = max(hits, key=lambda b: (gd.mods[b["mod"]].level, b["mod"]))
        fams = {gd.mods[b["mod"]].group for b in hits}
        if len(fams) > 1:
            raise self._err(a, f'bench craft "{what}" is ambiguous: ' + "; ".join(sorted({gd.mods[b["mod"]].short() for b in hits}))[:400])
        return best["mod"], dict(best["cost"])

    def _beast(self, a, st, item, ch):
        what = a.args["what"]
        gd = self.gd
        if what in ("lock_suffixes", "lock_prefixes", "multimod"):
            return replace(st, item=M.beast_metamod(item, what)), M.beast_cost("metamod")
        if what == "imprint":
            if item.rarity != "magic":
                raise self._err(a, "beastcraft imprint needs a magic item")
            return replace(st, imprint=item), M.beast_cost("imprint")
        if what == "restore_imprint":
            if st.imprint is None:
                raise self._err(a, "no imprint to restore (use 'beastcraft imprint' first)")
            return replace(st, item=st.imprint, imprint=None), {}
        if what == "hinekoras_lock":
            return replace(st, item=M.hinekoras_lock(item)), M.beast_cost("hinekoras_lock")
        if what == "max_links":
            return replace(st, item=M.max_links(item)), M.beast_cost("max_links")
        if what == "add_mod":
            inf = a.args["influence"]
            if inf not in ("hunter", "warlord", "redeemer"):
                raise self._err(a, "beastcraft add_mod supports hunter, warlord, redeemer")
            return replace(st, item=M.beast_add_influence_mod(item, ch, inf)), M.beast_cost(f"add_{inf}_mod")
        if what in ("add_prefix_remove_suffix", "add_suffix_remove_prefix"):
            side = "prefix" if what == "add_prefix_remove_suffix" else "suffix"
            return replace(st, item=M.beast_swap_side(item, ch, side)), M.beast_cost(what)
        raise self._err(a, f"unknown beastcraft '{what}'. Known: lock_suffixes, lock_prefixes, multimod, imprint, "
                           f"restore_imprint, hinekoras_lock, max_links, add_mod <influence>, "
                           f"add_prefix_remove_suffix, add_suffix_remove_prefix")

    # ------------------------------------------------------------ Monte Carlo
    def run_mc(self, prog: Program, seed=None, max_steps=200_000, trace=None):
        ch = M.RandomChooser(seed)
        costs = defaultdict(float)
        steps = [0]

        def run(node, st):
            if st.status != "run":
                return st
            if isinstance(node, Seq):
                for s in node.body:
                    st = run(s, st)
                    if st.status != "run":
                        break
                return st
            steps[0] += 1
            if steps[0] > max_steps:
                raise RecipeError(f"run exceeded {max_steps} steps: a loop probably never ends "
                                  f"(check its condition, or add 'limit N')", node.line, node.file)
            if isinstance(node, Act):
                st2, c = self.primitive(node, st, ch)
                for k, v in c.items():
                    costs[k] += v
                if trace is not None and node.op == "show":
                    trace.append((node.args.get("msg") or f"line {node.line}") + "\n" +
                                 (st.item.describe(self.gd) if st.item else "(no item)"))
                return st2
            if isinstance(node, If):
                if self.cond(node.cond, st, node):
                    return run(node.then, st)
                return run(node.other, st) if node.other else st
            if isinstance(node, Loop):
                n = 0
                if node.check_first and self._loop_exit(node, st):
                    return st
                while True:
                    st = run(node.body, st)
                    n += 1
                    if st.status != "run" or self._loop_exit(node, st):
                        return st
                    if node.limit is not None and n >= node.limit:
                        return replace(st, status="fail", msg=f"gave up after {n} attempts (line {node.line})")
            if isinstance(node, Foresee):
                if st.item is None or not st.item.preview_lock:
                    raise self._err(node, "'foresee' needs an item with Hinekora's Lock "
                                          "(beastcraft hinekoras_lock on a magic item first)")
                st2, c = self.primitive(node.action, st, ch)
                if self.cond(node.cond, st2, node):
                    for k, v in c.items():
                        costs[k] += v
                    return st2
                st = replace(st, item=replace(st.item, preview_lock=False))
                return run(node.other, st) if node.other else st
            raise TypeError(node)

        st = run(prog.body, State())
        return self._finish(prog, st, dict(costs), steps[0])

    def _loop_exit(self, node, st):
        c = self.cond(node.cond, st, node)
        return (not c) if node.negate else c

    def _finish(self, prog, st, costs, steps=0):
        ok = st.status != "fail"
        if ok and prog.goal is not None:
            if st.item is None:
                ok = False
            else:
                ok = self.cond(prog.goal, st)
        if not ok and st.value:
            costs[LOST] = costs.get(LOST, 0) + st.value
        return RunResult(ok, costs, st.item, st.msg, steps)

    # ------------------------------------------------------------ exact
    def run_exact(self, prog: Program, max_paths=200_000, max_states=4000):
        self._budget = [max_paths, max_states]
        self._cache = {}
        out = self._exact(prog.body, State())
        # terminal accounting
        res = ExactResult()
        for st, P, S1, S2 in out.values():
            ok = st.status != "fail"
            if ok and prog.goal is not None:
                ok = st.item is not None and self.cond(prog.goal, st)
            lost = 0.0 if ok else st.value
            if lost:
                S1 = dict(S1)
                S1[LOST] = S1.get(LOST, 0) + P * lost
            res.add(st, P, S1, S2, ok)
        return res

    def _chaos(self, vec):
        return sum(self.prices[k] * v for k, v in vec.items())

    def _exact(self, node, st):
        """Distribution after executing node from state st: {key: (state, P, S1, S2)}."""
        if st.status != "run":
            return {st: (st, 1.0, {}, 0.0)}
        key = (id(node), st)
        if key in self._cache:
            return self._cache[key]
        if isinstance(node, Seq):
            dist = {st: (st, 1.0, {}, 0.0)}
            for s in node.body:
                dist = self._compose(dist, s)
            out = dist
        elif isinstance(node, Act):
            out = self._exact_primitive(node, st)
        elif isinstance(node, If):
            if self.cond(node.cond, st, node):
                out = self._exact(node.then, st)
            else:
                out = self._exact(node.other, st) if node.other else {st: (st, 1.0, {}, 0.0)}
        elif isinstance(node, Loop):
            out = self._exact_loop(node, st)
        elif isinstance(node, Foresee):
            if st.item is None or not st.item.preview_lock:
                raise self._err(node, "'foresee' needs an item with Hinekora's Lock")
            outs = self._exact_primitive(node.action, st)
            out = {}
            unlocked = replace(st, item=replace(st.item, preview_lock=False))
            other = self._exact(node.other, unlocked) if node.other else {unlocked: (unlocked, 1.0, {}, 0.0)}
            for s2, P, S1, S2 in outs.values():
                if self.cond(node.cond, s2, node):
                    _acc(out, s2, P, S1, S2)
                else:
                    for s3, P3, S13, S23 in other.values():
                        _acc(out, s3, P * P3, {k: P * v for k, v in S13.items()}, P * S23)
        else:
            raise TypeError(node)
        self._cache[key] = out
        return out

    def _compose(self, dist, node):
        out = {}
        for s, P, X1, X2 in dist.values():
            x1c = self._chaos(X1)
            for t, Q, Y1, Y2 in self._exact(node, s).values():
                S1 = {k: v * Q for k, v in X1.items()}
                for k, v in Y1.items():
                    S1[k] = S1.get(k, 0.0) + P * v
                y1c = self._chaos(Y1)
                _acc(out, t, P * Q, S1, X2 * Q + 2 * x1c * y1c + P * Y2)
        return out

    def _exact_primitive(self, a, st):
        outcomes = {}

        def fn(ch):
            return self.primitive(a, st, ch)

        for (s2, cost_items), p in _enumerate(fn, self._budget).items():
            cost = dict(cost_items)
            c = self._chaos(cost)
            _acc(outcomes, s2, p, {k: p * v for k, v in cost.items()}, p * c * c)
        return outcomes

    def _exact_loop(self, node, s0):
        import numpy as np
        if node.check_first and self._loop_exit(node, s0):
            return {s0: (s0, 1.0, {}, 0.0)}
        limit = node.limit
        idx = {}
        order = []
        trans = []  # per transient: list of (kind, target, Q, Y1, Y2)

        def tkey(s, k):
            return (s, k if limit is not None else 0)

        start = tkey(s0, 0)
        idx[start] = 0
        order.append(start)
        i = 0
        absorbed = {}
        while i < len(order):
            s, k = order[i]
            rows = []
            for t, Q, Y1, Y2 in self._exact(node.body, s).values():
                if t.status != "run" or self._loop_exit(node, t):
                    rows.append(("a", t, Q, Y1, Y2))
                elif limit is not None and k + 1 >= limit:
                    tf = replace(t, status="fail", msg=f"gave up after {limit} attempts (line {node.line})")
                    rows.append(("a", tf, Q, Y1, Y2))
                else:
                    tk = tkey(t, k + 1)
                    if tk not in idx:
                        idx[tk] = len(order)
                        order.append(tk)
                        if len(order) > self._budget[1]:
                            raise ExactTooLarge(f"loop at line {node.line} has more than {self._budget[1]} states")
                    rows.append(("t", tk, Q, Y1, Y2))
                for r in rows[-1:]:
                    if r[0] == "a" and r[1] not in absorbed:
                        absorbed[r[1]] = len(absorbed)
            trans.append(rows)
            i += 1
        n, m = len(order), len(absorbed)
        curr = sorted({c for rows in trans for r in rows for c in r[3]})
        ci = {c: j for j, c in enumerate(curr)}
        A_ = np.eye(n)
        RP = np.zeros((n, m))
        RY1 = np.zeros((len(curr), n, m))
        RY2 = np.zeros((n, m))
        QY1 = np.zeros((len(curr), n, n))
        QY1c = np.zeros((n, n))
        QY2 = np.zeros((n, n))
        RY1c = np.zeros((n, m))
        for x, rows in enumerate(trans):
            for kind, tgt, Q, Y1, Y2 in rows:
                y1c = self._chaos(Y1)
                if kind == "a":
                    u = absorbed[tgt]
                    RP[x, u] += Q
                    for c, v in Y1.items():
                        RY1[ci[c], x, u] += v
                    RY2[x, u] += Y2
                    RY1c[x, u] += y1c
                else:
                    t = idx[tgt]
                    A_[x, t] -= Q
                    for c, v in Y1.items():
                        QY1[ci[c], x, t] += v
                    QY1c[x, t] += y1c
                    QY2[x, t] += Y2
        try:
            Ainv_RP = np.linalg.solve(A_, RP)
        except np.linalg.LinAlgError:
            raise self._err(node, "this loop can never finish from some state (its exit condition is unreachable); "
                                  "add 'limit N' or fix the condition")
        if np.any(Ainv_RP.sum(axis=1) < 1 - 1e-6) or not np.all(np.isfinite(Ainv_RP)):
            raise self._err(node, "this loop can never finish from some reachable state; add 'limit N' or fix the condition")
        B1 = np.zeros((len(curr), n, m))
        for j in range(len(curr)):
            B1[j] = np.linalg.solve(A_, RY1[j] + QY1[j] @ Ainv_RP)
        B1c = np.linalg.solve(A_, RY1c + QY1c @ Ainv_RP)
        B2 = np.linalg.solve(A_, RY2 + QY2 @ Ainv_RP + 2 * QY1c @ B1c)
        out = {}
        for tgt, u in absorbed.items():
            P = Ainv_RP[0, u]
            if P <= 0:
                continue
            S1 = {c: float(B1[ci[c], 0, u]) for c in curr if B1[ci[c], 0, u]}
            _acc(out, tgt, float(P), S1, float(B2[0, u]))
        return out


def _acc(d, s, P, S1, S2):
    if s in d:
        _, P0, A, B = d[s]
        A = dict(A)
        for k, v in S1.items():
            A[k] = A.get(k, 0.0) + v
        d[s] = (s, P0 + P, A, B + S2)
    else:
        d[s] = (s, P, dict(S1), S2)


def _cmp(op, a, b):
    return {"<=": a <= b, ">=": a >= b, "<": a < b, ">": a > b, "=": a == b, "==": a == b, "!=": a != b}[op]


class _Replay:
    __slots__ = ("prefix", "trail")

    def __init__(self, prefix):
        self.prefix = prefix
        self.trail = []

    def weighted(self, options, weights):
        pos = len(self.trail)
        i = self.prefix[pos][0] if pos < len(self.prefix) else 0
        tot = float(sum(weights))
        probs = [w / tot for w in weights]
        self.trail.append((i, probs))
        return options[i]

    def uniform(self, options):
        return self.weighted(options, [1] * len(options))


def _enumerate(fn, budget):
    """Enumerate all outcomes of fn(chooser): {(state, cost_items): prob}."""
    res = defaultdict(float)
    prefix = []
    while True:
        ch = _Replay(prefix)
        st, cost = fn(ch)
        p = 1.0
        for i, probs in ch.trail:
            p *= probs[i]
        res[(st, tuple(sorted(cost.items())))] += p
        budget[0] -= 1
        if budget[0] < 0:
            raise ExactTooLarge("too many random outcomes to enumerate exactly")
        trail = ch.trail
        while trail and trail[-1][0] == len(trail[-1][1]) - 1:
            trail.pop()
        if not trail:
            break
        i, probs = trail[-1]
        trail[-1] = (i + 1, probs)
        prefix = trail
    return res


# ---------------------------------------------------------------- results
@dataclass
class RunResult:
    success: bool
    costs: dict
    item: Item | None
    msg: str
    steps: int


@dataclass
class ExactResult:
    p_success: float = 0.0
    mean_counts: dict = field(default_factory=dict)  # currency -> E[count]
    second_moment: float = 0.0
    outcomes: list = field(default_factory=list)

    def add(self, st, P, S1, S2, ok):
        if ok:
            self.p_success += P
        for k, v in S1.items():
            self.mean_counts[k] = self.mean_counts.get(k, 0.0) + v
        self.second_moment += S2
        self.outcomes.append((st, P, ok))
