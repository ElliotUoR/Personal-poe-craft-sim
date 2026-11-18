//! Recipe language: lexer, parser and sub-recipe expansion (port of `poecraft/lang.py`).
//! Full reference: docs/language.md.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct RecipeError {
    pub msg: String,
    pub line: Option<usize>,
    pub file: Option<Arc<str>>,
}

impl RecipeError {
    pub fn new(msg: impl Into<String>, line: Option<usize>, file: Option<Arc<str>>) -> Self {
        RecipeError { msg: msg.into(), line, file }
    }
}

impl std::fmt::Display for RecipeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = self
            .file
            .as_deref()
            .map(|p| Path::new(p).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string()))
            .unwrap_or_else(|| "<recipe>".into());
        match self.line {
            Some(l) => write!(f, "{name}:{l}: {}", self.msg),
            None if self.file.is_some() => write!(f, "{name}: {}", self.msg),
            None => f.write_str(&self.msg),
        }
    }
}
impl std::error::Error for RecipeError {}

pub type PResult<T> = Result<T, RecipeError>;

// ------------------------------------------------------------------ lexer
#[derive(Debug, Clone, PartialEq)]
pub enum TokKind {
    Str(String),
    Num(f64, bool), // value, is_integer
    Op(&'static str),
    Name(String),
    Eof,
}

#[derive(Debug, Clone)]
pub struct Tok {
    pub kind: TokKind,
    pub line: usize,
}

const OPS: [&str; 13] = ["<=", ">=", "==", "!=", "<", ">", "=", "(", ")", "{", "}", ",", ""];

pub fn lex(src: &str, file: &Option<Arc<str>>) -> PResult<Vec<Tok>> {
    let b: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    let mut line = 1;
    while i < b.len() {
        let c = b[i];
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c == ' ' || c == '\t' || c == '\r' {
            i += 1;
        } else if c == '#' {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if c == '"' {
            let mut s = String::new();
            i += 1;
            loop {
                if i >= b.len() || b[i] == '\n' {
                    return Err(RecipeError::new("unterminated string (missing closing \")", Some(line), file.clone()));
                }
                match b[i] {
                    '"' => {
                        i += 1;
                        break;
                    }
                    '\\' if i + 1 < b.len() => {
                        s.push(match b[i + 1] {
                            'n' => '\n',
                            't' => '\t',
                            o => o,
                        });
                        i += 2;
                    }
                    o => {
                        s.push(o);
                        i += 1;
                    }
                }
            }
            toks.push(Tok { kind: TokKind::Str(s), line });
        } else if c.is_ascii_digit() || (c == '-' && i + 1 < b.len() && b[i + 1].is_ascii_digit()) {
            let start = i;
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let mut int = true;
            if i + 1 < b.len() && b[i] == '.' && b[i + 1].is_ascii_digit() {
                int = false;
                i += 1;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let s: String = b[start..i].iter().collect();
            toks.push(Tok { kind: TokKind::Num(s.parse().unwrap(), int), line });
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == '_' || b[i] == '\'') {
                i += 1;
            }
            toks.push(Tok { kind: TokKind::Name(b[start..i].iter().collect()), line });
        } else {
            let two: String = b[i..(i + 2).min(b.len())].iter().collect();
            if let Some(op) = OPS.iter().find(|o| o.len() == 2 && **o == two) {
                toks.push(Tok { kind: TokKind::Op(op), line });
                i += 2;
            } else if let Some(op) = OPS.iter().find(|o| o.len() == 1 && o.starts_with(c)) {
                toks.push(Tok { kind: TokKind::Op(op), line });
                i += 1;
            } else {
                return Err(RecipeError::new(format!("unexpected character '{c}'"), Some(line), file.clone()));
            }
        }
    }
    toks.push(Tok { kind: TokKind::Eof, line });
    Ok(toks)
}

// ------------------------------------------------------------------ AST
/// A string argument: a literal, or a sub-recipe parameter (replaced during expansion).
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Lit(String),
    Param(String, usize),
}

impl Arg {
    /// The literal text (only valid after expansion).
    pub fn s(&self) -> &str {
        match self {
            Arg::Lit(s) => s,
            Arg::Param(p, _) => p,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Le,
    Ge,
    Lt,
    Gt,
    Eq,
    Ne,
}

impl CmpOp {
    fn parse(s: &str) -> Option<CmpOp> {
        Some(match s {
            "<=" => CmpOp::Le,
            ">=" => CmpOp::Ge,
            "<" => CmpOp::Lt,
            ">" => CmpOp::Gt,
            "=" | "==" => CmpOp::Eq,
            "!=" => CmpOp::Ne,
            _ => return None,
        })
    }
    pub fn eval(self, a: f64, b: f64) -> bool {
        match self {
            CmpOp::Le => a <= b,
            CmpOp::Ge => a >= b,
            CmpOp::Lt => a < b,
            CmpOp::Gt => a > b,
            CmpOp::Eq => a == b,
            CmpOp::Ne => a != b,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    PrefixCount,
    SuffixCount,
    ModCount,
    OpenPrefix,
    OpenSuffix,
    InfluencedCount,
    ElevatedCount,
    CraftedCount,
    Links,
}

impl Metric {
    fn parse(s: &str) -> Option<Metric> {
        Some(match s {
            "prefix_count" => Metric::PrefixCount,
            "suffix_count" => Metric::SuffixCount,
            "mod_count" => Metric::ModCount,
            "open_prefix" | "open_prefixes" => Metric::OpenPrefix,
            "open_suffix" | "open_suffixes" => Metric::OpenSuffix,
            "influenced_count" => Metric::InfluencedCount,
            "elevated_count" => Metric::ElevatedCount,
            "crafted_count" => Metric::CraftedCount,
            "links" => Metric::Links,
            _ => return None,
        })
    }
    const NAMES: [&'static str; 11] = [
        "prefix_count", "suffix_count", "mod_count", "open_prefix", "open_suffix", "open_prefixes", "open_suffixes",
        "influenced_count", "elevated_count", "crafted_count", "links",
    ];
}

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Num(f64),
    Metric(Metric),
    Tier(Arg, usize),
    CountInfluence(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cond {
    And(Box<Cond>, Box<Cond>),
    Or(Box<Cond>, Box<Cond>),
    Not(Box<Cond>),
    Const(bool),
    /// has <mod> [tier <op> N | elevated]
    Has { r: Arg, tier: Option<(CmpOp, i64)>, elevated: bool, line: usize },
    Is(String),
    Influenced(String),
    Cmp(CmpOp, Val, Val),
}

#[derive(Debug, Clone, PartialEq)]
pub struct WithMod {
    pub r: Arg,
    pub tier: Option<i64>,
    pub elevated: bool,
    pub fractured: bool,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BuyArgs {
    pub base: String,
    pub rarity: String,
    pub influences: Vec<String>,
    pub ilvl: u32,
    pub with: Vec<WithMod>,
    pub cost: Vec<(String, f64)>,
    pub tag: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// transmute, alteration, augment, regal, alchemy, chaos, exalt, annul, scour, dominance,
    /// fracture, veiled_exalt, remove_crafted
    Simple(&'static str),
    FoulbornExalt(Option<String>),
    Conqueror(String),
    Buy(BuyArgs),
    SetAside(String),
    Take(String),
    Awaken(String),
    Unveil(Vec<Arg>),
    Essence(Arg),
    /// craft <builtin> | craft "a", "b" (fallbacks)
    Craft(Vec<Arg>),
    Beastcraft { what: String, influence: Option<String> },
    HarvestReforge(String),
    /// Harvest "add a new <tag> modifier and remove another random modifier" (non-influenced)
    HarvestAdd(String),
    Omen(String),
    Pay(Vec<(String, f64)>),
    Fail(String),
    Succeed(String),
    Show(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Act {
    pub line: usize,
    pub file: Option<Arc<str>>,
    pub op: Op,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Act(Act),
    Seq(Seq),
    Loop(Loop),
    If(If),
    Foresee(Foresee),
    Call(Call),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Seq {
    pub line: usize,
    pub file: Option<Arc<str>>,
    pub body: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Loop {
    pub line: usize,
    pub file: Option<Arc<str>>,
    pub body: Seq,
    pub cond: Cond,
    /// until/while check before the first iteration
    pub check_first: bool,
    /// while: loop while cond holds
    pub negate: bool,
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct If {
    pub line: usize,
    pub file: Option<Arc<str>>,
    pub cond: Cond,
    pub then: Seq,
    pub other: Option<Seq>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Foresee {
    pub line: usize,
    pub file: Option<Arc<str>>,
    pub action: Act,
    pub cond: Cond,
    pub other: Option<Seq>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub line: usize,
    pub file: Option<Arc<str>>,
    pub name: String,
    pub args: Vec<Arg>,
}

#[derive(Debug, Clone)]
pub struct Def {
    pub name: String,
    pub params: Vec<String>,
    pub body: Seq,
    pub line: usize,
    pub file: Option<Arc<str>>,
}

#[derive(Debug, Clone)]
pub struct Program {
    pub name: String,
    pub body: Seq,
    pub goal: Option<Cond>,
    pub tracks: Vec<(String, Cond)>,
    pub sales: Vec<(Vec<(String, f64)>, Cond)>,
    pub rules: BTreeMap<String, bool>,
}

// ------------------------------------------------------------------ parser
const SIMPLE: &[(&str, &str)] = &[
    ("transmute", "transmute"),
    ("alt", "alteration"),
    ("alteration", "alteration"),
    ("aug", "augment"),
    ("augment", "augment"),
    ("regal", "regal"),
    ("alch", "alchemy"),
    ("alchemy", "alchemy"),
    ("chaos", "chaos"),
    ("exalt", "exalt"),
    ("annul", "annul"),
    ("scour", "scour"),
    ("dominance", "dominance"),
    ("orb_of_dominance", "dominance"),
    ("fracture", "fracture"),
    ("veiled_exalt", "veiled_exalt"),
    ("remove_crafted", "remove_crafted"),
];
const CONQ: &[(&str, &str)] = &[("hunter_exalt", "hunter"), ("warlord_exalt", "warlord"), ("redeemer_exalt", "redeemer"), ("crusader_exalt", "crusader")];
const KEYWORDS: &[&str] = &[
    "sell", "rule", "track", "if", "else", "repeat", "until", "while", "foresee", "accept", "otherwise", "def", "include", "goal", "and", "or", "not", "has", "with", "for", "limit",
];
const STATEMENTS: &[&str] = &[
    "buy", "set_aside", "take", "awaken", "essence", "craft", "beastcraft", "harvest", "omen", "pay", "fail", "succeed", "show", "unveil", "rule", "track", "sell", "repeat", "until", "while", "if",
    "foresee", "goal", "def", "include", "foulborn_exalt",
];

struct Parser<'d> {
    file: Option<Arc<str>>,
    toks: Vec<Tok>,
    i: usize,
    defs: &'d mut BTreeMap<String, Def>,
    goal: Option<Cond>,
    tracks: Vec<(String, Cond)>,
    sales: Vec<(Vec<(String, f64)>, Cond)>,
    rules: BTreeMap<String, bool>,
    include_stack: Vec<PathBuf>,
}

impl<'d> Parser<'d> {
    fn peek(&self) -> &Tok {
        &self.toks[self.i]
    }
    fn err(&self, msg: impl Into<String>) -> RecipeError {
        RecipeError::new(msg, Some(self.peek().line), self.file.clone())
    }
    fn err_at(&self, msg: impl Into<String>, line: usize) -> RecipeError {
        RecipeError::new(msg, Some(line), self.file.clone())
    }
    fn describe(t: &Tok) -> String {
        match &t.kind {
            TokKind::Eof => "end of file".into(),
            TokKind::Str(s) => format!("'{s}'"),
            TokKind::Num(n, _) => format!("'{n}'"),
            TokKind::Op(o) => format!("'{o}'"),
            TokKind::Name(n) => format!("'{n}'"),
        }
    }
    fn accept_op(&mut self, op: &str) -> bool {
        if matches!(&self.peek().kind, TokKind::Op(o) if *o == op) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn accept_name(&mut self, n: &str) -> bool {
        if matches!(&self.peek().kind, TokKind::Name(x) if x == n) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn expect_op(&mut self, op: &str, what: &str) -> PResult<()> {
        if self.accept_op(op) {
            Ok(())
        } else {
            Err(self.err(format!("expected {what}, got {}", Self::describe(self.peek()))))
        }
    }
    fn expect_name(&mut self, what: &str) -> PResult<String> {
        if let TokKind::Name(n) = &self.peek().kind {
            let n = n.clone();
            self.i += 1;
            Ok(n)
        } else {
            Err(self.err(format!("expected {what}, got {}", Self::describe(self.peek()))))
        }
    }
    fn expect_kw(&mut self, kw: &str, what: &str) -> PResult<()> {
        if self.accept_name(kw) {
            Ok(())
        } else {
            Err(self.err(format!("expected {what}, got {}", Self::describe(self.peek()))))
        }
    }
    fn expect_str(&mut self, what: &str) -> PResult<String> {
        if let TokKind::Str(s) = &self.peek().kind {
            let s = s.clone();
            self.i += 1;
            Ok(s)
        } else {
            Err(self.err(format!("expected {what}, got {}", Self::describe(self.peek()))))
        }
    }
    fn expect_num(&mut self, what: &str) -> PResult<f64> {
        if let TokKind::Num(n, _) = &self.peek().kind {
            let n = *n;
            self.i += 1;
            Ok(n)
        } else {
            Err(self.err(format!("expected {what}, got {}", Self::describe(self.peek()))))
        }
    }
    fn accept_str(&mut self) -> Option<String> {
        if let TokKind::Str(s) = &self.peek().kind {
            let s = s.clone();
            self.i += 1;
            Some(s)
        } else {
            None
        }
    }

    fn string_or_param(&mut self, what: &str) -> PResult<Arg> {
        let t = self.peek().clone();
        match t.kind {
            TokKind::Str(s) => {
                self.i += 1;
                Ok(Arg::Lit(s))
            }
            TokKind::Name(n) if !KEYWORDS.contains(&n.as_str()) => {
                self.i += 1;
                Ok(Arg::Param(n, t.line))
            }
            _ => Err(self.err(format!("expected {what}"))),
        }
    }

    fn parse_program(&mut self) -> PResult<Seq> {
        let mut body = Vec::new();
        while self.peek().kind != TokKind::Eof {
            if let Some(st) = self.statement(true)? {
                body.push(st);
            }
        }
        Ok(Seq { line: 1, file: self.file.clone(), body })
    }

    fn block(&mut self) -> PResult<Seq> {
        let start = self.peek().line;
        self.expect_op("{", "'{'")?;
        let mut body = Vec::new();
        while !self.accept_op("}") {
            if self.peek().kind == TokKind::Eof {
                return Err(self.err_at("unclosed '{' (missing '}')", start));
            }
            if let Some(st) = self.statement(false)? {
                body.push(st);
            }
        }
        Ok(Seq { line: start, file: self.file.clone(), body })
    }

    fn act(&self, line: usize, op: Op) -> Option<Node> {
        Some(Node::Act(Act { line, file: self.file.clone(), op }))
    }

    fn statement(&mut self, top: bool) -> PResult<Option<Node>> {
        let t = self.peek().clone();
        let line = t.line;
        let w = match &t.kind {
            TokKind::Name(n) => n.clone(),
            _ => return Err(self.err(format!("expected a statement, got {}", Self::describe(&t)))),
        };
        match w.as_str() {
            "def" => {
                if !top {
                    return Err(self.err("'def' is only allowed at the top level of a file"));
                }
                self.i += 1;
                let name = self.expect_name("sub-recipe name")?;
                self.expect_op("(", "'('")?;
                let mut params = Vec::new();
                if !self.accept_op(")") {
                    loop {
                        params.push(self.expect_name("parameter name")?);
                        if self.accept_op(")") {
                            break;
                        }
                        self.expect_op(",", "',' or ')'")?;
                    }
                }
                let body = self.block()?;
                if self.defs.contains_key(&name) {
                    return Err(self.err_at(format!("sub-recipe '{name}' defined twice"), line));
                }
                self.defs.insert(name.clone(), Def { name, params, body, line, file: self.file.clone() });
                return Ok(None);
            }
            "include" => {
                self.i += 1;
                let path = self.expect_str("file name")?;
                let base = self.file.as_deref().and_then(|f| Path::new(f).parent().map(|p| p.to_path_buf())).unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                let full = normalize(&base.join(&path));
                if !full.exists() {
                    return Err(self.err_at(format!("include file not found: {path} (looked for {})", full.display()), line));
                }
                if self.include_stack.contains(&full) {
                    return Ok(None);
                }
                let src = std::fs::read_to_string(&full).map_err(|e| self.err_at(format!("cannot read {}: {e}", full.display()), line))?;
                let mut stack = self.include_stack.clone();
                stack.push(full.clone());
                let file: Option<Arc<str>> = Some(Arc::from(full.to_string_lossy().as_ref()));
                let toks = lex(&src, &file)?;
                let mut sub = Parser { file, toks, i: 0, defs: self.defs, goal: None, tracks: vec![], sales: vec![], rules: BTreeMap::new(), include_stack: stack };
                let body = sub.parse_program()?;
                if !body.body.is_empty() {
                    return Err(self.err_at("included files may only contain 'def' sub-recipes", line));
                }
                return Ok(None);
            }
            "goal" => {
                self.i += 1;
                self.goal = Some(self.condition()?);
                return Ok(None);
            }
            "rule" => {
                self.i += 1;
                let name = self.expect_name("a rule name")?;
                let val = self.expect_name("on or off")?;
                if val != "on" && val != "off" {
                    return Err(self.err("rule value must be 'on' or 'off'"));
                }
                self.rules.insert(name, val == "on");
                return Ok(None);
            }
            "sell" => {
                self.i += 1;
                let n = self.expect_num("an amount")?;
                let unit = self.expect_name("a currency (chaos, divine)")?;
                let cond = if self.accept_name("if") { self.condition()? } else { Cond::Const(true) };
                self.sales.push((vec![(unit, n)], cond));
                return Ok(None);
            }
            "track" => {
                self.i += 1;
                let label = self.expect_str("a label string")?;
                let c = self.condition()?;
                self.tracks.push((label, c));
                return Ok(None);
            }
            _ => {}
        }
        self.i += 1;
        if w == "foulborn_exalt" {
            let side = match &self.peek().kind {
                TokKind::Name(n) if n == "prefix" || n == "suffix" => Some(self.expect_name("side")?),
                _ => None,
            };
            return Ok(self.act(line, Op::FoulbornExalt(side)));
        }
        if let Some((_, op)) = SIMPLE.iter().find(|s| s.0 == w) {
            return Ok(self.act(line, Op::Simple(op)));
        }
        if let Some((_, inf)) = CONQ.iter().find(|s| s.0 == w) {
            return Ok(self.act(line, Op::Conqueror(inf.to_string())));
        }
        match w.as_str() {
            "buy" => {
                let b = self.buy(line)?;
                return Ok(self.act(line, Op::Buy(b)));
            }
            "set_aside" => {
                let n = self.expect_name("a name for the item")?;
                return Ok(self.act(line, Op::SetAside(n)));
            }
            "take" => {
                let n = self.expect_name("the name of a set-aside item")?;
                return Ok(self.act(line, Op::Take(n)));
            }
            "awaken" => {
                let n = self.expect_name("the set-aside donor item")?;
                return Ok(self.act(line, Op::Awaken(n)));
            }
            "unveil" => {
                let mut prefs = vec![self.string_or_param("a mod name")?];
                while self.accept_op(",") {
                    prefs.push(self.string_or_param("a mod name")?);
                }
                return Ok(self.act(line, Op::Unveil(prefs)));
            }
            "essence" => {
                let a = self.string_or_param("an essence name")?;
                return Ok(self.act(line, Op::Essence(a)));
            }
            "craft" => {
                if let TokKind::Name(n) = &self.peek().kind {
                    let n = n.clone();
                    self.i += 1;
                    return Ok(self.act(line, Op::Craft(vec![Arg::Lit(n)])));
                }
                let mut opts = vec![self.string_or_param("a bench craft")?];
                while self.accept_op(",") {
                    opts.push(self.string_or_param("a bench craft")?);
                }
                return Ok(self.act(line, Op::Craft(opts)));
            }
            "beastcraft" => {
                let what = self.expect_name("a beastcraft name")?;
                let influence = if what == "add_mod" { Some(self.expect_name("an influence (hunter, warlord, redeemer)")?) } else { None };
                return Ok(self.act(line, Op::Beastcraft { what, influence }));
            }
            "harvest" => {
                let kind = self.expect_name("'reforge' or 'add'")?;
                let tag = self.expect_name("a tag like caster")?;
                return match kind.as_str() {
                    "reforge" => Ok(self.act(line, Op::HarvestReforge(tag))),
                    "add" => Ok(self.act(line, Op::HarvestAdd(tag))),
                    _ => Err(self.err(format!("unknown harvest craft '{kind}' (use 'harvest reforge <tag>' or 'harvest add <tag>')"))),
                };
            }
            "omen" => {
                let what = self.expect_name("an omen name")?;
                return Ok(self.act(line, Op::Omen(what)));
            }
            "pay" => {
                let n = self.expect_num("an amount")?;
                let unit = self.expect_name("a currency (chaos, divine, ...)")?;
                return Ok(self.act(line, Op::Pay(vec![(unit, n)])));
            }
            "fail" | "succeed" | "show" => {
                let msg = self.accept_str().unwrap_or_default();
                let op = match w.as_str() {
                    "fail" => Op::Fail(msg),
                    "succeed" => Op::Succeed(msg),
                    _ => Op::Show(msg),
                };
                return Ok(self.act(line, op));
            }
            "repeat" => {
                let limit = self.opt_limit()?;
                let body = self.block()?;
                let negate = if self.accept_name("until") {
                    false
                } else if self.accept_name("while") {
                    true
                } else {
                    return Err(self.err("'repeat { ... }' must be followed by 'until <condition>' or 'while <condition>'"));
                };
                let cond = self.condition()?;
                return Ok(Some(Node::Loop(Loop { line, file: self.file.clone(), body, cond, check_first: false, negate, limit })));
            }
            "until" | "while" => {
                let cond = self.condition()?;
                let limit = self.opt_limit()?;
                let body = self.block()?;
                return Ok(Some(Node::Loop(Loop { line, file: self.file.clone(), body, cond, check_first: true, negate: w == "while", limit })));
            }
            "if" => return Ok(Some(self.if_rest(line)?)),
            "foresee" => {
                let act = match self.statement(false)? {
                    Some(Node::Act(a)) => a,
                    _ => return Err(self.err_at("'foresee' must be followed by a single currency action", line)),
                };
                self.expect_kw("accept", "'accept if <condition>'")?;
                self.expect_kw("if", "'if'")?;
                let cond = self.condition()?;
                let other = if self.accept_name("otherwise") { Some(self.block()?) } else { None };
                return Ok(Some(Node::Foresee(Foresee { line, file: self.file.clone(), action: act, cond, other })));
            }
            _ => {}
        }
        if self.accept_op("(") {
            let mut args = Vec::new();
            if !self.accept_op(")") {
                loop {
                    args.push(self.string_or_param("an argument string")?);
                    if self.accept_op(")") {
                        break;
                    }
                    self.expect_op(",", "',' or ')'")?;
                }
            }
            return Ok(Some(Node::Call(Call { line, file: self.file.clone(), name: w, args })));
        }
        let mut known: Vec<&str> = SIMPLE.iter().map(|s| s.0).chain(CONQ.iter().map(|s| s.0)).collect();
        known.extend_from_slice(STATEMENTS);
        let hint = suggest(&w, &known);
        Err(self.err_at(format!("unknown statement '{w}'.{hint} (Sub-recipe calls need parentheses: {w}(...))"), line))
    }

    fn opt_limit(&mut self) -> PResult<Option<u64>> {
        if self.accept_name("limit") {
            Ok(Some(self.expect_num("a number of attempts")? as u64))
        } else {
            Ok(None)
        }
    }

    fn if_rest(&mut self, line: usize) -> PResult<Node> {
        let cond = self.condition()?;
        let then = self.block()?;
        let other = if self.accept_name("else") {
            if self.accept_name("if") {
                let l = self.toks[self.i - 1].line;
                Some(Seq { line, file: self.file.clone(), body: vec![self.if_rest(l)?] })
            } else {
                Some(self.block()?)
            }
        } else {
            None
        };
        Ok(Node::If(If { line, file: self.file.clone(), cond, then, other }))
    }

    fn buy(&mut self, line: usize) -> PResult<BuyArgs> {
        let mut a = BuyArgs { base: String::new(), rarity: "normal".into(), influences: vec![], ilvl: 100, with: vec![], cost: vec![], tag: None };
        let mut have_base = false;
        loop {
            let t = self.peek().clone();
            match &t.kind {
                TokKind::Str(s) if !have_base => {
                    a.base = s.clone();
                    have_base = true;
                    self.i += 1;
                }
                TokKind::Name(n) if n == "normal" || n == "magic" || n == "rare" => {
                    a.rarity = n.clone();
                    self.i += 1;
                }
                TokKind::Name(n) if n == "influence" => {
                    self.i += 1;
                    a.influences.push(self.expect_name("an influence")?);
                    while self.accept_op(",") {
                        a.influences.push(self.expect_name("an influence")?);
                    }
                }
                TokKind::Name(n) if n == "ilvl" => {
                    self.i += 1;
                    a.ilvl = self.expect_num("an item level")? as u32;
                }
                TokKind::Name(n) if n == "with" => {
                    self.i += 1;
                    let fractured = self.accept_name("fractured");
                    let r = self.string_or_param("a mod name")?;
                    let (mut tier, mut elevated) = (None, false);
                    if self.accept_name("tier") {
                        tier = Some(self.expect_num("a tier number")? as i64);
                    } else if self.accept_name("elevated") {
                        elevated = true;
                    }
                    a.with.push(WithMod { r, tier, elevated, fractured, line: t.line });
                }
                TokKind::Name(n) if n == "for" => {
                    self.i += 1;
                    if self.accept_name("price") {
                        let key = self.expect_str("a price key")?;
                        a.cost = vec![(key, 1.0)];
                    } else {
                        let n = self.expect_num("an amount")?;
                        let unit = self.expect_name("a currency (chaos, divine)")?;
                        a.cost = vec![(unit, n)];
                    }
                }
                TokKind::Name(n) if n == "as" => {
                    self.i += 1;
                    a.tag = Some(self.expect_name("a label")?);
                }
                _ => break,
            }
        }
        if !have_base {
            return Err(self.err_at("buy needs a base name, e.g. buy \"Astral Plate\" ilvl 86", line));
        }
        Ok(a)
    }

    // -- conditions
    fn condition(&mut self) -> PResult<Cond> {
        let mut left = self.cond_and()?;
        while self.accept_name("or") {
            left = Cond::Or(Box::new(left), Box::new(self.cond_and()?));
        }
        Ok(left)
    }
    fn cond_and(&mut self) -> PResult<Cond> {
        let mut left = self.cond_not()?;
        while self.accept_name("and") {
            left = Cond::And(Box::new(left), Box::new(self.cond_not()?));
        }
        Ok(left)
    }
    fn cond_not(&mut self) -> PResult<Cond> {
        if self.accept_name("not") {
            return Ok(Cond::Not(Box::new(self.cond_not()?)));
        }
        self.cond_atom()
    }
    fn cond_atom(&mut self) -> PResult<Cond> {
        let t = self.peek().clone();
        if self.accept_op("(") {
            let c = self.condition()?;
            self.expect_op(")", "')'")?;
            return Ok(c);
        }
        if self.accept_name("has") {
            let r = self.string_or_param("a mod name")?;
            let (mut tier, mut elevated) = (None, false);
            if self.accept_name("tier") {
                let mut op = CmpOp::Le;
                if let TokKind::Op(o) = &self.peek().kind {
                    if let Some(c) = CmpOp::parse(o).filter(|c| *c != CmpOp::Ne) {
                        op = c;
                        self.i += 1;
                    }
                }
                tier = Some((op, self.expect_num("a tier number")? as i64));
            } else if self.accept_name("elevated") {
                elevated = true;
            }
            return Ok(Cond::Has { r, tier, elevated, line: t.line });
        }
        if self.accept_name("is") {
            let r = self.expect_name("normal, magic or rare")?;
            if !["normal", "magic", "rare"].contains(&r.as_str()) {
                return Err(self.err("'is' must be followed by normal, magic or rare"));
            }
            return Ok(Cond::Is(r));
        }
        if self.accept_name("influenced") {
            return Ok(Cond::Influenced(self.expect_name("an influence")?));
        }
        if self.accept_name("true") {
            return Ok(Cond::Const(true));
        }
        if self.accept_name("false") {
            return Ok(Cond::Const(false));
        }
        let lhs = self.value()?;
        if let TokKind::Op(o) = &self.peek().kind {
            if let Some(op) = CmpOp::parse(o) {
                self.i += 1;
                let rhs = self.value()?;
                return Ok(Cond::Cmp(op, lhs, rhs));
            }
        }
        if matches!(lhs, Val::Metric(_)) {
            return Ok(Cond::Cmp(CmpOp::Gt, lhs, Val::Num(0.0)));
        }
        Err(self.err_at("expected a comparison", t.line))
    }
    fn value(&mut self) -> PResult<Val> {
        let t = self.peek().clone();
        match &t.kind {
            TokKind::Num(n, _) => {
                self.i += 1;
                Ok(Val::Num(*n))
            }
            TokKind::Name(n) if Metric::parse(n).is_some() => {
                self.i += 1;
                Ok(Val::Metric(Metric::parse(n).unwrap()))
            }
            TokKind::Name(n) if n == "tier" => {
                self.i += 1;
                Ok(Val::Tier(self.string_or_param("a mod name")?, t.line))
            }
            TokKind::Name(n) if n == "count" => {
                self.i += 1;
                self.expect_op("(", "'('")?;
                let inf = self.expect_name("an influence")?;
                self.expect_op(")", "')'")?;
                Ok(Val::CountInfluence(inf))
            }
            other => {
                let word = match other {
                    TokKind::Name(n) => n.clone(),
                    _ => Self::describe(&t),
                };
                let mut opts: Vec<&str> = Metric::NAMES.to_vec();
                opts.extend_from_slice(&["has", "is", "influenced", "tier", "not"]);
                let hint = suggest(&word, &opts);
                Err(self.err_at(format!("unknown condition '{word}'.{hint}"), t.line))
            }
        }
    }
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            o => out.push(o),
        }
    }
    out
}

/// "Did you mean ..." hint (similarity in the spirit of difflib.get_close_matches, cutoff 0.6).
pub fn suggest(word: &str, options: &[&str]) -> String {
    fn ratio(a: &str, b: &str) -> f64 {
        let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
        let mut prev: Vec<usize> = (0..=b.len()).collect();
        for i in 1..=a.len() {
            let mut cur = vec![i; b.len() + 1];
            for j in 1..=b.len() {
                cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1]));
            }
            prev = cur;
        }
        let d = prev[b.len()] as f64;
        1.0 - d / (a.len().max(b.len()).max(1) as f64)
    }
    let mut scored: Vec<(f64, &str)> = options.iter().map(|o| (ratio(word, o), *o)).filter(|(r, _)| *r >= 0.6).collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    scored.dedup_by(|a, b| a.1 == b.1);
    if scored.is_empty() {
        String::new()
    } else {
        format!(" Did you mean: {}?", scored.iter().take(3).map(|x| x.1).collect::<Vec<_>>().join(", "))
    }
}

// ------------------------------------------------------------------ expansion
type Env = BTreeMap<String, String>;

fn sub_arg(a: &Arg, env: &Env, file: &Option<Arc<str>>) -> PResult<Arg> {
    match a {
        Arg::Lit(_) => Ok(a.clone()),
        Arg::Param(p, line) => env
            .get(p)
            .map(|v| Arg::Lit(v.clone()))
            .ok_or_else(|| RecipeError::new(format!("'{p}' is not a string or a parameter of this sub-recipe"), Some(*line), file.clone())),
    }
}

fn sub_val(v: &Val, env: &Env, f: &Option<Arc<str>>) -> PResult<Val> {
    Ok(match v {
        Val::Tier(a, l) => Val::Tier(sub_arg(a, env, f)?, *l),
        o => o.clone(),
    })
}

fn sub_cond(c: &Cond, env: &Env, f: &Option<Arc<str>>) -> PResult<Cond> {
    Ok(match c {
        Cond::And(a, b) => Cond::And(Box::new(sub_cond(a, env, f)?), Box::new(sub_cond(b, env, f)?)),
        Cond::Or(a, b) => Cond::Or(Box::new(sub_cond(a, env, f)?), Box::new(sub_cond(b, env, f)?)),
        Cond::Not(a) => Cond::Not(Box::new(sub_cond(a, env, f)?)),
        Cond::Has { r, tier, elevated, line } => Cond::Has { r: sub_arg(r, env, f)?, tier: *tier, elevated: *elevated, line: *line },
        Cond::Cmp(op, a, b) => Cond::Cmp(*op, sub_val(a, env, f)?, sub_val(b, env, f)?),
        o => o.clone(),
    })
}

fn sub_op(op: &Op, env: &Env, f: &Option<Arc<str>>) -> PResult<Op> {
    let subs = |v: &Vec<Arg>| v.iter().map(|a| sub_arg(a, env, f)).collect::<PResult<Vec<_>>>();
    Ok(match op {
        Op::Unveil(v) => Op::Unveil(subs(v)?),
        Op::Craft(v) => Op::Craft(subs(v)?),
        Op::Essence(a) => Op::Essence(sub_arg(a, env, f)?),
        Op::Buy(b) => {
            let mut b = b.clone();
            for w in &mut b.with {
                w.r = sub_arg(&w.r, env, f)?;
            }
            Op::Buy(b)
        }
        o => o.clone(),
    })
}

fn expand_seq(s: &Seq, defs: &BTreeMap<String, Def>, env: &Env, depth: usize) -> PResult<Seq> {
    let mut out = Vec::new();
    for n in &s.body {
        match n {
            Node::Call(c) => out.extend(expand_call(c, defs, env, depth)?.body),
            other => out.push(expand_node(other, defs, env, depth)?),
        }
    }
    Ok(Seq { line: s.line, file: s.file.clone(), body: out })
}

fn expand_call(c: &Call, defs: &BTreeMap<String, Def>, env: &Env, depth: usize) -> PResult<Seq> {
    if depth > 50 {
        return Err(RecipeError::new("sub-recipe calls nested too deeply (recursion?)", Some(c.line), c.file.clone()));
    }
    let Some(d) = defs.get(&c.name) else {
        let names: Vec<&str> = defs.keys().map(|s| s.as_str()).collect();
        let hint = suggest(&c.name, &names);
        return Err(RecipeError::new(format!("unknown sub-recipe '{}'.{hint}", c.name), Some(c.line), c.file.clone()));
    };
    if c.args.len() != d.params.len() {
        return Err(RecipeError::new(
            format!("sub-recipe '{}' takes {} argument(s) ({}), got {}", d.name, d.params.len(), d.params.join(", "), c.args.len()),
            Some(c.line),
            c.file.clone(),
        ));
    }
    let mut new_env = Env::new();
    for (p, a) in d.params.iter().zip(&c.args) {
        new_env.insert(p.clone(), sub_arg(a, env, &c.file)?.s().to_string());
    }
    expand_seq(&d.body, defs, &new_env, depth + 1)
}

fn expand_node(n: &Node, defs: &BTreeMap<String, Def>, env: &Env, depth: usize) -> PResult<Node> {
    Ok(match n {
        Node::Act(a) => Node::Act(Act { line: a.line, file: a.file.clone(), op: sub_op(&a.op, env, &a.file)? }),
        Node::Seq(s) => Node::Seq(expand_seq(s, defs, env, depth)?),
        Node::Loop(l) => Node::Loop(Loop { body: expand_seq(&l.body, defs, env, depth)?, cond: sub_cond(&l.cond, env, &l.file)?, ..l.clone() }),
        Node::If(i) => Node::If(If {
            cond: sub_cond(&i.cond, env, &i.file)?,
            then: expand_seq(&i.then, defs, env, depth)?,
            other: i.other.as_ref().map(|o| expand_seq(o, defs, env, depth)).transpose()?,
            ..i.clone()
        }),
        Node::Foresee(f) => Node::Foresee(Foresee {
            action: Act { op: sub_op(&f.action.op, env, &f.file)?, ..f.action.clone() },
            cond: sub_cond(&f.cond, env, &f.file)?,
            other: f.other.as_ref().map(|o| expand_seq(o, defs, env, depth)).transpose()?,
            ..f.clone()
        }),
        Node::Call(c) => Node::Seq(expand_call(c, defs, env, depth)?),
    })
}

pub fn parse(src: &str, file: Option<&Path>) -> PResult<Program> {
    let abs = file.map(|f| std::fs::canonicalize(f).map(|p| PathBuf::from(p.to_string_lossy().trim_start_matches(r"\\?\"))).unwrap_or_else(|_| f.to_path_buf()));
    let farc: Option<Arc<str>> = abs.as_ref().map(|p| Arc::from(p.to_string_lossy().as_ref()));
    let toks = lex(src, &farc)?;
    let mut defs = BTreeMap::new();
    let mut p = Parser { file: farc.clone(), toks, i: 0, defs: &mut defs, goal: None, tracks: vec![], sales: vec![], rules: BTreeMap::new(), include_stack: abs.iter().cloned().collect() };
    let body = p.parse_program()?;
    let (goal, tracks, sales, rules) = (p.goal.take(), std::mem::take(&mut p.tracks), std::mem::take(&mut p.sales), std::mem::take(&mut p.rules));
    let env = Env::new();
    let body = expand_seq(&body, &defs, &env, 0)?;
    let goal = goal.map(|g| sub_cond(&g, &env, &farc)).transpose()?;
    let tracks = tracks.into_iter().map(|(l, c)| Ok((l, sub_cond(&c, &env, &farc)?))).collect::<PResult<Vec<_>>>()?;
    let sales = sales.into_iter().map(|(a, c)| Ok((a, sub_cond(&c, &env, &farc)?))).collect::<PResult<Vec<_>>>()?;
    let name = file.and_then(|f| f.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "recipe".into());
    Ok(Program { name, body, goal, tracks, sales, rules })
}

pub fn parse_file(path: &Path) -> PResult<Program> {
    let src = std::fs::read_to_string(path).map_err(|e| RecipeError::new(format!("cannot read {}: {e}", path.display()), None, None))?;
    parse(&src, Some(path))
}
