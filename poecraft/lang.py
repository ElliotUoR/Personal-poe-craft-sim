"""Craft recipe language: lexer, parser and sub-recipe expansion.

See docs/language.md for the full reference. Short example::

    def elevate(mod) {
        repeat { dominance } until has mod elevated or influenced_count < 2
    }
    buy "Astral Plate" ilvl 86 rare influence hunter
        with "Hunter: Attacks have" tier 1 with "Hunter: maximum Life" tier 2
        for 30 chaos
    elevate("Hunter: Attacks have")
    goal has "Hunter: Attacks have" elevated
"""
from __future__ import annotations

import os
import re
from dataclasses import dataclass, field


class RecipeError(Exception):
    def __init__(self, msg, line=None, file=None):
        self.line = line
        self.file = file
        loc = ""
        if file or line:
            loc = f"{os.path.basename(file) if file else '<recipe>'}:{line}: " if line else f"{file}: "
        super().__init__(loc + msg)


# ---------------------------------------------------------------- lexer
TOKEN_RE = re.compile(r"""
    (?P<ws>[ \t\r]+)|
    (?P<nl>\n)|
    (?P<comment>\#[^\n]*)|
    (?P<str>"(?:[^"\\\n]|\\.)*")|
    (?P<num>-?\d+(?:\.\d+)?)|
    (?P<op><=|>=|==|!=|<|>|=|\(|\)|\{|\}|,)|
    (?P<name>[A-Za-z_][A-Za-z0-9_']*)
""", re.X)


@dataclass
class Tok:
    kind: str
    val: object
    line: int


def lex(src, file=None):
    toks, line, pos = [], 1, 0
    while pos < len(src):
        m = TOKEN_RE.match(src, pos)
        if not m:
            raise RecipeError(f"unexpected character {src[pos]!r}", line, file)
        k = m.lastgroup
        v = m.group(k)
        if k == "nl":
            line += 1
        elif k == "str":
            toks.append(Tok("str", bytes(v[1:-1], "utf-8").decode("unicode_escape"), line))
        elif k == "num":
            toks.append(Tok("num", float(v) if "." in v else int(v), line))
        elif k in ("op", "name"):
            toks.append(Tok(k, v, line))
        pos = m.end()
    toks.append(Tok("eof", None, line))
    return toks


# ---------------------------------------------------------------- AST
@dataclass
class Node:
    line: int
    file: str | None = field(default=None, repr=False, compare=False)


@dataclass
class Act(Node):
    """A primitive action. op = action name, args = dict."""
    op: str = ""
    args: dict = field(default_factory=dict)


@dataclass
class Seq(Node):
    body: list = field(default_factory=list)


@dataclass
class Loop(Node):
    body: Seq = None
    cond: object = None
    check_first: bool = False  # until/while check before the first iteration
    negate: bool = False  # while: loop while cond holds
    limit: int | None = None


@dataclass
class If(Node):
    cond: object = None
    then: Seq = None
    other: Seq | None = None


@dataclass
class Foresee(Node):
    action: Act = None
    cond: object = None
    other: Seq | None = None


@dataclass
class Call(Node):
    name: str = ""
    args: list = field(default_factory=list)


@dataclass
class Def:
    name: str
    params: list
    body: Seq
    line: int
    file: str | None


@dataclass
class Program:
    body: Seq
    defs: dict
    goal: object = None
    name: str = ""
    tracks: list = field(default_factory=list)


# conditions: tuples
#   ("and", a, b) ("or", a, b) ("not", a) ("has", modref, tierop, n, elevated_flag, line)
#   ("cmp", op, lhs, rhs) where lhs/rhs = ("num", v) | ("metric", name) | ("tier", modref)
#   ("is", rarity) ("influenced", infl) ("bool", metric)

SIMPLE_ACTIONS = {
    "transmute": "transmute", "alt": "alteration", "alteration": "alteration", "aug": "augment",
    "augment": "augment", "regal": "regal", "alch": "alchemy", "alchemy": "alchemy", "chaos": "chaos",
    "exalt": "exalt", "annul": "annul", "scour": "scour", "dominance": "dominance",
    "orb_of_dominance": "dominance", "fracture": "fracture", "veiled_exalt": "veiled_exalt",
    "remove_crafted": "remove_crafted",
}
CONQ_ACTIONS = {"hunter_exalt": "hunter", "warlord_exalt": "warlord", "redeemer_exalt": "redeemer",
                "crusader_exalt": "crusader"}
METRICS = {"prefix_count", "suffix_count", "mod_count", "open_prefix", "open_suffix", "open_prefixes",
           "open_suffixes", "influenced_count", "elevated_count", "crafted_count", "links"}
KEYWORDS = {"track", "if", "else", "repeat", "until", "while", "foresee", "accept", "otherwise", "def", "include",
            "goal", "and", "or", "not", "has", "with", "for", "limit"}


class Parser:
    def __init__(self, src, file=None, defs=None, include_stack=()):
        self.file = file
        self.toks = lex(src, file)
        self.i = 0
        self.defs = defs if defs is not None else {}
        self.goal = None
        self.tracks = []
        self.include_stack = include_stack

    # -- token helpers
    def peek(self, k=0):
        return self.toks[self.i + k]

    def next(self):
        t = self.toks[self.i]
        self.i += 1
        return t

    def err(self, msg, tok=None):
        tok = tok or self.peek()
        return RecipeError(msg, tok.line, self.file)

    def accept(self, kind, val=None):
        t = self.peek()
        if t.kind == kind and (val is None or t.val == val):
            self.i += 1
            return t
        return None

    def expect(self, kind, val=None, what=None):
        t = self.accept(kind, val)
        if not t:
            got = self.peek()
            want = what or (repr(val) if val else kind)
            gotd = "end of file" if got.kind == "eof" else repr(got.val)
            raise self.err(f"expected {want}, got {gotd}")
        return t

    def name_is(self, *vals):
        t = self.peek()
        return t.kind == "name" and t.val in vals

    # -- program
    def parse_program(self):
        body = []
        while self.peek().kind != "eof":
            st = self.statement(top=True)
            if st is not None:
                body.append(st)
        return Program(Seq(1, self.file, body), self.defs, self.goal, tracks=self.tracks)

    def block(self):
        start = self.expect("op", "{", "'{'")
        body = []
        while not self.accept("op", "}"):
            if self.peek().kind == "eof":
                raise RecipeError("unclosed '{' (missing '}')", start.line, self.file)
            st = self.statement()
            if st is not None:
                body.append(st)
        return Seq(start.line, self.file, body)

    def string_or_param(self, what="a string"):
        t = self.peek()
        if t.kind == "str":
            self.next()
            return t.val
        if t.kind == "name" and t.val not in KEYWORDS:
            self.next()
            return ("param", t.val, t.line)
        raise self.err(f"expected {what}")

    def statement(self, top=False):
        t = self.peek()
        line = t.line
        if t.kind != "name":
            raise self.err(f"expected a statement, got {t.val!r}")
        w = t.val
        if w == "def":
            if not top:
                raise self.err("'def' is only allowed at the top level of a file")
            self.next()
            name = self.expect("name", what="sub-recipe name").val
            self.expect("op", "(")
            params = []
            if not self.accept("op", ")"):
                while True:
                    params.append(self.expect("name", what="parameter name").val)
                    if self.accept("op", ")"):
                        break
                    self.expect("op", ",", "',' or ')'")
            body = self.block()
            if name in self.defs:
                raise RecipeError(f"sub-recipe '{name}' defined twice", line, self.file)
            self.defs[name] = Def(name, params, body, line, self.file)
            return None
        if w == "include":
            self.next()
            path = self.expect("str", what="file name").val
            base = os.path.dirname(self.file) if self.file else os.getcwd()
            full = os.path.normpath(os.path.join(base, path))
            if not os.path.exists(full):
                raise RecipeError(f"include file not found: {path} (looked for {full})", line, self.file)
            if full in self.include_stack:
                return None
            with open(full, encoding="utf-8") as f:
                sub = Parser(f.read(), full, self.defs, self.include_stack + (full,))
            prog = sub.parse_program()
            if prog.body.body:
                raise RecipeError("included files may only contain 'def' sub-recipes", line, self.file)
            return None
        if w == "goal":
            self.next()
            self.goal = self.condition()
            return None
        if w == "track":
            self.next()
            label = self.expect("str", what="a label string").val
            self.tracks.append((label, self.condition()))
            return None
        self.next()
        if w in SIMPLE_ACTIONS:
            return Act(line, self.file, SIMPLE_ACTIONS[w])
        if w in CONQ_ACTIONS:
            return Act(line, self.file, "conqueror_exalt", {"influence": CONQ_ACTIONS[w]})
        if w == "buy":
            return self.buy(line)
        if w == "set_aside":
            return Act(line, self.file, "set_aside", {"name": self.expect("name", what="a name for the item").val})
        if w == "take":
            return Act(line, self.file, "take", {"name": self.expect("name", what="the name of a set-aside item").val})
        if w == "awaken":
            return Act(line, self.file, "awaken", {"donor": self.expect("name", what="the set-aside donor item").val})
        if w == "essence":
            return Act(line, self.file, "essence", {"name": self.string_or_param("an essence name")})
        if w == "craft":
            t2 = self.peek()
            if t2.kind == "name":
                self.next()
                return Act(line, self.file, "craft", {"what": t2.val})
            return Act(line, self.file, "craft", {"what": self.string_or_param("a bench craft")})
        if w == "beastcraft":
            what = self.expect("name", what="a beastcraft name").val
            args = {"what": what}
            if what == "add_mod":
                args["influence"] = self.expect("name", what="an influence (hunter, warlord, redeemer)").val
            return Act(line, self.file, "beastcraft", args)
        if w == "harvest":
            kind = self.expect("name", what="'reforge'").val
            if kind != "reforge":
                raise self.err(f"unknown harvest craft '{kind}' (only 'harvest reforge <tag>' is supported)")
            return Act(line, self.file, "harvest_reforge", {"tag": self.expect("name", what="a tag like caster").val})
        if w == "omen":
            return Act(line, self.file, "omen", {"what": self.expect("name").val})
        if w == "pay":
            n = self.expect("num", what="an amount").val
            unit = self.expect("name", what="a currency (chaos, divine, ...)").val
            return Act(line, self.file, "pay", {"cost": {unit: n}})
        if w in ("fail", "succeed"):
            msg = self.accept("str")
            return Act(line, self.file, w, {"msg": msg.val if msg else ""})
        if w == "show":
            msg = self.accept("str")
            return Act(line, self.file, "show", {"msg": msg.val if msg else ""})
        if w == "repeat":
            limit = self.opt_limit()
            body = self.block()
            if self.accept("name", "until"):
                return Loop(line, self.file, body, self.condition(), False, False, limit)
            if self.accept("name", "while"):
                return Loop(line, self.file, body, self.condition(), False, True, limit)
            raise self.err("'repeat { ... }' must be followed by 'until <condition>' or 'while <condition>'")
        if w in ("until", "while"):
            cond = self.condition()
            limit = self.opt_limit()
            body = self.block()
            return Loop(line, self.file, body, cond, True, w == "while", limit)
        if w == "if":
            return self.if_rest(line)
        if w == "foresee":
            act = self.statement()
            if not isinstance(act, Act):
                raise RecipeError("'foresee' must be followed by a single currency action", line, self.file)
            self.expect("name", "accept", "'accept if <condition>'")
            self.expect("name", "if", "'if'")
            cond = self.condition()
            other = None
            if self.accept("name", "otherwise"):
                other = self.block()
            return Foresee(line, self.file, act, cond, other)
        if self.peek().kind == "op" and self.peek().val == "(":
            self.next()
            args = []
            if not self.accept("op", ")"):
                while True:
                    args.append(self.string_or_param("an argument string"))
                    if self.accept("op", ")"):
                        break
                    self.expect("op", ",", "',' or ')'")
            return Call(line, self.file, w, args)
        hint = _suggest(w, list(SIMPLE_ACTIONS) + list(CONQ_ACTIONS) + [
            "buy", "set_aside", "take", "awaken", "essence", "craft", "beastcraft", "harvest", "omen",
            "pay", "fail", "succeed", "show", "repeat", "until", "while", "if", "foresee", "goal", "def", "include"])
        raise RecipeError(f"unknown statement '{w}'.{hint} (Sub-recipe calls need parentheses: {w}(...))",
                          line, self.file)

    def opt_limit(self):
        if self.accept("name", "limit"):
            return int(self.expect("num", what="a number of attempts").val)
        return None

    def if_rest(self, line):
        cond = self.condition()
        then = self.block()
        other = None
        if self.accept("name", "else"):
            if self.accept("name", "if"):
                other = Seq(line, self.file, [self.if_rest(self.toks[self.i - 1].line)])
            else:
                other = self.block()
        return If(line, self.file, cond, then, other)

    def buy(self, line):
        args = {"rarity": "normal", "influences": [], "with": [], "ilvl": 100, "cost": {}, "base": None}
        while True:
            t = self.peek()
            if t.kind == "str" and args["base"] is None:
                args["base"] = self.next().val
            elif t.kind == "name" and t.val in ("normal", "magic", "rare"):
                args["rarity"] = self.next().val
            elif t.kind == "name" and t.val == "influence":
                self.next()
                args["influences"].append(self.expect("name", what="an influence").val)
                while self.accept("op", ","):
                    args["influences"].append(self.expect("name", what="an influence").val)
            elif t.kind == "name" and t.val == "ilvl":
                self.next()
                args["ilvl"] = int(self.expect("num", what="an item level").val)
            elif t.kind == "name" and t.val == "with":
                self.next()
                fractured = bool(self.accept("name", "fractured"))
                ref = self.string_or_param("a mod name")
                tier = None
                elevated = False
                if self.accept("name", "tier"):
                    tier = int(self.expect("num", what="a tier number").val)
                elif self.accept("name", "elevated"):
                    elevated = True
                args["with"].append({"ref": ref, "tier": tier, "elevated": elevated, "fractured": fractured,
                                     "line": t.line})
            elif t.kind == "name" and t.val == "for":
                self.next()
                if self.accept("name", "price"):
                    key = self.expect("str", what="a price key").val
                    args["cost"] = {key: 1}
                else:
                    n = self.expect("num", what="an amount").val
                    unit = self.expect("name", what="a currency (chaos, divine)").val
                    args["cost"] = {unit: n}
            elif t.kind == "name" and t.val == "as":
                self.next()
                args["tag"] = self.expect("name").val
            else:
                break
        if args["base"] is None:
            raise RecipeError('buy needs a base name, e.g. buy "Astral Plate" ilvl 86', line, self.file)
        return Act(line, self.file, "buy", args)

    # -- conditions
    def condition(self):
        left = self.cond_and()
        while self.accept("name", "or"):
            left = ("or", left, self.cond_and())
        return left

    def cond_and(self):
        left = self.cond_not()
        while self.accept("name", "and"):
            left = ("and", left, self.cond_not())
        return left

    def cond_not(self):
        if self.accept("name", "not"):
            return ("not", self.cond_not())
        return self.cond_atom()

    def cond_atom(self):
        t = self.peek()
        if self.accept("op", "("):
            c = self.condition()
            self.expect("op", ")", "')'")
            return c
        if self.accept("name", "has"):
            ref = self.string_or_param("a mod name")
            op, n, elev = None, None, False
            if self.accept("name", "tier"):
                op = "<="
                o = self.peek()
                if o.kind == "op" and o.val in ("<=", ">=", "<", ">", "=", "=="):
                    self.next()
                    op = o.val
                n = int(self.expect("num", what="a tier number").val)
            elif self.accept("name", "elevated"):
                elev = True
            return ("has", ref, op, n, elev, t.line)
        if self.accept("name", "is"):
            r = self.expect("name", what="normal, magic or rare").val
            if r not in ("normal", "magic", "rare"):
                raise self.err("'is' must be followed by normal, magic or rare")
            return ("is", r)
        if self.accept("name", "influenced"):
            return ("influenced", self.expect("name", what="an influence").val)
        if self.accept("name", "true"):
            return ("const", True)
        if self.accept("name", "false"):
            return ("const", False)
        lhs = self.value()
        o = self.peek()
        if o.kind == "op" and o.val in ("<=", ">=", "<", ">", "=", "==", "!="):
            self.next()
            return ("cmp", o.val, lhs, self.value())
        if lhs[0] == "metric":
            return ("cmp", ">", lhs, ("num", 0))
        raise RecipeError("expected a comparison", t.line, self.file)

    def value(self):
        t = self.peek()
        if t.kind == "num":
            self.next()
            return ("num", t.val)
        if t.kind == "name" and t.val in METRICS:
            self.next()
            return ("metric", {"open_prefixes": "open_prefix", "open_suffixes": "open_suffix"}.get(t.val, t.val))
        if t.kind == "name" and t.val == "tier":
            self.next()
            return ("tier", self.string_or_param("a mod name"), t.line)
        if t.kind == "name" and t.val == "count":
            # count(<influence>)  -> number of mods of that influence
            self.next()
            self.expect("op", "(")
            inf = self.expect("name").val
            self.expect("op", ")")
            return ("count_infl", inf)
        hint = _suggest(str(t.val), sorted(METRICS) + ["has", "is", "influenced", "tier", "not"])
        raise RecipeError(f"unknown condition '{t.val}'.{hint}", t.line, self.file)


def _suggest(word, options):
    import difflib
    m = difflib.get_close_matches(word, options, n=3)
    return f" Did you mean: {', '.join(m)}?" if m else ""


# ---------------------------------------------------------------- expansion
def _subst(x, env):
    if isinstance(x, tuple) and len(x) == 3 and x[0] == "param":
        if x[1] not in env:
            raise RecipeError(f"'{x[1]}' is not a string or a parameter of this sub-recipe", x[2])
        return env[x[1]]
    if isinstance(x, tuple):
        return tuple(_subst(y, env) for y in x)
    if isinstance(x, list):
        return [_subst(y, env) for y in x]
    if isinstance(x, dict):
        return {k: _subst(v, env) for k, v in x.items()}
    return x


def expand(node, defs, env=None, depth=0):
    """Inline sub-recipe calls and substitute parameters."""
    env = env or {}
    if depth > 50:
        raise RecipeError("sub-recipe calls nested too deeply (recursion?)", node.line, node.file)
    if isinstance(node, Seq):
        out = []
        for st in node.body:
            e = expand(st, defs, env, depth)
            if isinstance(e, Seq) and getattr(e, "_inlined", False):
                out.extend(e.body)
            else:
                out.append(e)
        return Seq(node.line, node.file, out)
    if isinstance(node, Act):
        return Act(node.line, node.file, node.op, _subst(node.args, env))
    if isinstance(node, Loop):
        return Loop(node.line, node.file, expand(node.body, defs, env, depth), _subst(node.cond, env),
                    node.check_first, node.negate, node.limit)
    if isinstance(node, If):
        return If(node.line, node.file, _subst(node.cond, env), expand(node.then, defs, env, depth),
                  expand(node.other, defs, env, depth) if node.other else None)
    if isinstance(node, Foresee):
        return Foresee(node.line, node.file, expand(node.action, defs, env, depth), _subst(node.cond, env),
                       expand(node.other, defs, env, depth) if node.other else None)
    if isinstance(node, Call):
        if node.name not in defs:
            hint = _suggest(node.name, list(defs))
            raise RecipeError(f"unknown sub-recipe '{node.name}'.{hint}", node.line, node.file)
        d = defs[node.name]
        if len(node.args) != len(d.params):
            raise RecipeError(f"sub-recipe '{d.name}' takes {len(d.params)} argument(s) "
                              f"({', '.join(d.params)}), got {len(node.args)}", node.line, node.file)
        args = [_subst(a, env) for a in node.args]
        body = expand(d.body, defs, dict(zip(d.params, args)), depth + 1)
        body._inlined = True
        return body
    raise TypeError(node)


def parse(src, file=None) -> Program:
    p = Parser(src, file, include_stack=(os.path.abspath(file),) if file else ())
    prog = p.parse_program()
    body = expand(prog.body, prog.defs)
    goal = _subst(prog.goal, {}) if prog.goal else None
    tracks = [(lbl, _subst(c, {})) for lbl, c in prog.tracks]
    return Program(body, prog.defs, goal, name=os.path.splitext(os.path.basename(file))[0] if file else "recipe",
                   tracks=tracks)


def parse_file(path) -> Program:
    with open(path, encoding="utf-8") as f:
        return parse(f.read(), os.path.abspath(path))
