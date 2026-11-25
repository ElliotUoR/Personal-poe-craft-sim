//! Recipe execution: run state, primitive actions, conditions and the Monte Carlo runner
//! (port of `poecraft/engine.py`). The exact engine is in `exact.rs`.

use crate::data::{FamIdx, GameData, Gen, Influence, Influences, MetaKind};
use crate::enumerate::enumerate;
use crate::item::{Item, Lock, Rarity, Roll};
use crate::lang::{Act, Cond, Metric, Node, Op, Program, RecipeError, Seq, Val};
use crate::mechanics::{beast_cost, harvest_cost, Chooser, CraftError, Mech, RandomChooser};
use crate::prices::{unit_alias, Prices};
use crate::rules::Switches;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// Pseudo-currency: chaos value of bought bases that were thrown away (risk metric).
pub const LOST: &str = "_lost_chaos";

// ------------------------------------------------------------------ small types
/// f64 stored by bits so states can be hashed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct F64(pub u64);
impl F64 {
    pub fn new(v: f64) -> F64 {
        F64(if v == 0.0 { 0 } else { v.to_bits() })
    }
    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Run,
    Fail,
    Ok,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct State {
    pub item: Option<Item>,
    /// sorted by name: (name, item, chaos value paid for it)
    pub aside: Vec<(Arc<str>, Item, F64)>,
    pub imprint: Option<Item>,
    pub status: Status,
    pub msg: Arc<str>,
    /// chaos paid to buy the current item (for the "lost base" risk)
    pub value: F64,
}

impl Default for State {
    fn default() -> Self {
        State {
            item: None,
            aside: vec![],
            imprint: None,
            status: Status::Run,
            msg: Arc::from(""),
            value: F64::default(),
        }
    }
}

impl State {
    fn get_aside(&self, name: &str) -> Option<(&Item, f64)> {
        self.aside
            .iter()
            .find(|x| &*x.0 == name)
            .map(|x| (&x.1, x.2.get()))
    }
    fn without_aside(&self, name: &str) -> Vec<(Arc<str>, Item, F64)> {
        self.aside
            .iter()
            .filter(|x| &*x.0 != name)
            .cloned()
            .collect()
    }
    pub fn with_item(&self, it: Item) -> State {
        State {
            item: Some(it),
            ..self.clone()
        }
    }
}

/// Currency index into the engine's currency table.
pub type Cur = u16;
pub type Cost = Vec<(Cur, f64)>;

// ------------------------------------------------------------------ engine
pub struct Engine<'a> {
    pub gd: &'a GameData,
    pub prices: &'a Prices,
    pub mech: Mech<'a>,
    curs: RefCell<(Vec<String>, FxHashMap<String, Cur>)>,
    cur_price: RefCell<Vec<f64>>,
    resolve_cache: RefCell<FxHashMap<(String, String), Result<FamIdx, String>>>,
    pub(crate) iid_cache: RefCell<FxHashMap<IidKey, Option<Rc<Iid>>>>,
}

#[derive(Hash, PartialEq, Eq, Clone)]
pub(crate) struct IidKey {
    act: usize,
    node: usize,
    base: u32,
    ilvl: u32,
    infl: Influences,
    rarity: Rarity,
    kept: Vec<Roll>,
    links: u8,
    white: u8,
    tag: Option<Arc<str>>,
}

/// An i.i.d. loop: one trial's outcomes split into exit ("success") and stay ("fail").
pub(crate) struct Iid {
    pub succ: Vec<(Item, f64)>,
    pub fail: Vec<(Item, f64)>,
    pub p: f64,
    pub cost: Cost,
}

pub struct RunResult {
    pub success: bool,
    pub costs: Vec<(String, f64)>,
    pub item: Option<Item>,
    pub msg: String,
    pub steps: u64,
    pub tracks: Vec<(String, bool)>,
    pub revenue: f64,
}

impl<'a> Engine<'a> {
    pub fn new(gd: &'a GameData, prices: &'a Prices, sw: Switches) -> Engine<'a> {
        Engine {
            gd,
            prices,
            mech: Mech::new(gd, sw),
            curs: RefCell::new((vec![], FxHashMap::default())),
            cur_price: RefCell::new(vec![]),
            resolve_cache: RefCell::new(FxHashMap::default()),
            iid_cache: RefCell::new(FxHashMap::default()),
        }
    }

    /// Switches from the program's `rule` lines (errors on unknown rule names).
    pub fn switches_for(prog: &Program) -> Result<Switches, RecipeError> {
        let mut sw = Switches::default();
        for (k, v) in &prog.rules {
            sw.set(k, *v).map_err(|e| RecipeError::new(e, None, None))?;
        }
        Ok(sw)
    }

    // -------------------------------------------------------------- currencies
    pub fn cur(&self, name: &str) -> Cur {
        let name = unit_alias(name);
        let mut c = self.curs.borrow_mut();
        if let Some(&i) = c.1.get(name) {
            return i;
        }
        let i = c.0.len() as Cur;
        c.0.push(name.to_string());
        c.1.insert(name.to_string(), i);
        self.cur_price.borrow_mut().push(self.prices.get(name));
        i
    }
    pub fn cur_name(&self, c: Cur) -> String {
        self.curs.borrow().0[c as usize].clone()
    }
    pub fn n_curs(&self) -> usize {
        self.curs.borrow().0.len()
    }
    pub fn price(&self, c: Cur) -> f64 {
        self.cur_price.borrow()[c as usize]
    }
    pub fn chaos(&self, cost: &[(Cur, f64)]) -> f64 {
        let p = self.cur_price.borrow();
        cost.iter().map(|(c, n)| p[*c as usize] * n).sum()
    }
    fn cost_of(&self, items: &[(&str, f64)]) -> Cost {
        items.iter().map(|(k, n)| (self.cur(k), *n)).collect()
    }
    fn cost_owned(&self, items: &[(String, f64)]) -> Cost {
        items.iter().map(|(k, n)| (self.cur(k), *n)).collect()
    }

    // -------------------------------------------------------------- names
    pub fn resolve(
        &self,
        r: &str,
        item: Option<&Item>,
        line: usize,
        file: &Option<Arc<str>>,
    ) -> Result<FamIdx, RecipeError> {
        let cls = item
            .map(|i| self.gd.item_class(i.base).to_string())
            .unwrap_or_default();
        let key = (r.to_string(), cls.clone());
        if let Some(v) = self.resolve_cache.borrow().get(&key) {
            return v
                .clone()
                .map_err(|e| RecipeError::new(e, Some(line), file.clone()));
        }
        let res = self
            .gd
            .resolve_mod(
                r,
                if cls.is_empty() {
                    None
                } else {
                    Some(cls.as_str())
                },
            )
            .map_err(|e| e.0);
        self.resolve_cache.borrow_mut().insert(key, res.clone());
        res.map_err(|e| RecipeError::new(e, Some(line), file.clone()))
    }

    // -------------------------------------------------------------- conditions
    pub fn cond(
        &self,
        c: &Cond,
        st: &State,
        line: usize,
        file: &Option<Arc<str>>,
    ) -> Result<bool, RecipeError> {
        Ok(match c {
            Cond::And(a, b) => self.cond(a, st, line, file)? && self.cond(b, st, line, file)?,
            Cond::Or(a, b) => self.cond(a, st, line, file)? || self.cond(b, st, line, file)?,
            Cond::Not(a) => !self.cond(a, st, line, file)?,
            Cond::Const(b) => *b,
            other => {
                let Some(item) = &st.item else {
                    return Err(RecipeError::new(
                        "condition checked but there is no current item (missing 'buy'?)",
                        Some(line),
                        file.clone(),
                    ));
                };
                match other {
                    Cond::Has {
                        r,
                        tier,
                        elevated,
                        line: l,
                    } => {
                        let fam = self.resolve(r.s(), Some(item), *l, file)?;
                        let best = self.best_tier(item, fam);
                        match best {
                            None => false,
                            Some(t) if *elevated => t == 0,
                            Some(t) => match tier {
                                None => true,
                                Some((op, n)) => op.eval(t as f64, *n as f64),
                            },
                        }
                    }
                    Cond::Is(r) => item.rarity.name() == r,
                    Cond::Influenced(i) => {
                        Influence::parse(i).is_some_and(|i| item.influences.contains(i))
                    }
                    Cond::Cmp(op, a, b) => {
                        op.eval(self.value(a, item, file)?, self.value(b, item, file)?)
                    }
                    _ => unreachable!(),
                }
            }
        })
    }

    fn best_tier(&self, item: &Item, fam: FamIdx) -> Option<u32> {
        item.mods
            .iter()
            .filter(|r| self.gd.family_idx(r.m) == fam)
            .map(|r| self.gd.tier(r.m, item.base))
            .min()
    }

    fn value(&self, v: &Val, item: &Item, file: &Option<Arc<str>>) -> Result<f64, RecipeError> {
        let gd = self.gd;
        Ok(match v {
            Val::Num(n) => *n,
            Val::Tier(r, l) => {
                let fam = self.resolve(r.s(), Some(item), *l, file)?;
                self.best_tier(item, fam).map(|t| t as f64).unwrap_or(99.0)
            }
            Val::CountInfluence(i) => {
                let inf = Influence::parse(i);
                item.influenced_mods(gd)
                    .iter()
                    .filter(|r| gd.m(r.m).influence == inf)
                    .count() as f64
            }
            Val::Metric(m) => {
                (match m {
                    Metric::PrefixCount => item.n_side(gd, Gen::Prefix),
                    Metric::SuffixCount => item.n_side(gd, Gen::Suffix),
                    Metric::ModCount => item.mods.len(),
                    Metric::OpenPrefix => item.open_slots(gd, Gen::Prefix),
                    Metric::OpenSuffix => item.open_slots(gd, Gen::Suffix),
                    Metric::InfluencedCount => item.influenced_mods(gd).len(),
                    Metric::ElevatedCount => {
                        item.mods.iter().filter(|r| gd.m(r.m).elevated).count()
                    }
                    Metric::CraftedCount => item.crafted_count(),
                    Metric::Links => item.links as usize,
                }) as f64
            }
        })
    }

    pub fn loop_exit(&self, l: &crate::lang::Loop, st: &State) -> Result<bool, RecipeError> {
        let c = self.cond(&l.cond, st, l.line, &l.file)?;
        Ok(if l.negate { !c } else { c })
    }

    // -------------------------------------------------------------- primitives
    fn err(&self, a: &Act, msg: impl Into<String>) -> RecipeError {
        RecipeError::new(msg, Some(a.line), a.file.clone())
    }

    fn op_name(op: &Op) -> String {
        match op {
            Op::Simple(s) => s.to_string(),
            Op::FoulbornExalt(_) => "foulborn_exalt".into(),
            Op::Conqueror(_) => "conqueror_exalt".into(),
            Op::Buy(_) => "buy".into(),
            Op::SetAside(_) => "set_aside".into(),
            Op::Take(_) => "take".into(),
            Op::Awaken(_) => "awaken".into(),
            Op::Unveil(_) => "unveil".into(),
            Op::Essence(_) => "essence".into(),
            Op::Craft(_) => "craft".into(),
            Op::Beastcraft { .. } => "beastcraft".into(),
            Op::HarvestReforge(_) => "harvest_reforge".into(),
            Op::HarvestAdd(_) => "harvest_add".into(),
            Op::Omen(_) => "omen".into(),
            Op::Pay(_) => "pay".into(),
            Op::Fail(_) => "fail".into(),
            Op::Succeed(_) => "succeed".into(),
            Op::Show(_) => "show".into(),
        }
    }

    /// Execute one primitive action: (new state, cost).
    pub fn primitive(
        &self,
        a: &Act,
        st: &State,
        ch: &mut dyn Chooser,
    ) -> Result<(State, Cost), RecipeError> {
        self.primitive_inner(a, st, ch).map_err(|e| match e {
            PrimErr::Craft(c) => self.err(a, format!("{}: {}", Self::op_name(&a.op), c.0)),
            PrimErr::Recipe(r) => r,
        })
    }

    fn need_item<'s>(&self, a: &Act, st: &'s State) -> Result<&'s Item, PrimErr> {
        st.item.as_ref().ok_or_else(|| {
            PrimErr::Recipe(self.err(
                a,
                format!(
                    "'{}' needs a current item; start with 'buy' (or 'take <name>').",
                    Self::op_name(&a.op)
                ),
            ))
        })
    }

    fn primitive_inner(
        &self,
        a: &Act,
        st: &State,
        ch: &mut dyn Chooser,
    ) -> Result<(State, Cost), PrimErr> {
        let m = &self.mech;
        let gd = self.gd;
        let item_state = |it: Item| State {
            item: Some(it),
            ..st.clone()
        };
        match &a.op {
            Op::Buy(b) => return self.buy(a, b, st).map_err(PrimErr::Recipe),
            Op::Pay(c) => return Ok((st.clone(), self.cost_owned(c))),
            Op::Fail(msg) => {
                let msg = if msg.is_empty() {
                    format!("fail at line {}", a.line)
                } else {
                    msg.clone()
                };
                return Ok((
                    State {
                        status: Status::Fail,
                        msg: Arc::from(msg.as_str()),
                        ..st.clone()
                    },
                    vec![],
                ));
            }
            Op::Succeed(msg) => {
                return Ok((
                    State {
                        status: Status::Ok,
                        msg: Arc::from(msg.as_str()),
                        ..st.clone()
                    },
                    vec![],
                ))
            }
            Op::Show(_) => return Ok((st.clone(), vec![])),
            Op::SetAside(name) => {
                let it = self.need_item(a, st)?.clone();
                if st.get_aside(name).is_some() {
                    return Err(PrimErr::Recipe(
                        self.err(a, format!("an item named '{name}' is already set aside")),
                    ));
                }
                let mut aside = st.aside.clone();
                aside.push((Arc::from(name.as_str()), it, st.value));
                aside.sort_by(|x, y| x.0.cmp(&y.0));
                return Ok((
                    State {
                        item: None,
                        aside,
                        value: F64::default(),
                        ..st.clone()
                    },
                    vec![],
                ));
            }
            Op::Take(name) => {
                let Some((it, v)) = st.get_aside(name) else {
                    return Err(PrimErr::Recipe(self.err(
                        a,
                        format!("no set-aside item named '{name}' (set_aside it first)"),
                    )));
                };
                if st.item.is_some() {
                    return Err(PrimErr::Recipe(self.err(
                        a,
                        "'take' needs the current item to be set aside first (set_aside <name>)",
                    )));
                }
                let it = it.clone();
                return Ok((
                    State {
                        item: Some(it),
                        aside: st.without_aside(name),
                        value: F64::new(v),
                        ..st.clone()
                    },
                    vec![],
                ));
            }
            _ => {}
        }
        let item = self.need_item(a, st)?;
        let one = |k: &str| self.cost_of(&[(k, 1.0)]);
        Ok(match &a.op {
            Op::Simple(op) => {
                let (it, key) = match *op {
                    "transmute" => (m.transmute(item, ch)?, "transmute"),
                    "alteration" => (m.alteration(item, ch)?, "alt"),
                    "augment" => (m.augment(item, ch)?, "aug"),
                    "regal" => (m.regal(item, ch)?, "regal"),
                    "alchemy" => (m.alchemy(item, ch)?, "alch"),
                    "chaos" => (m.chaos(item, ch)?, "chaos"),
                    "exalt" => (m.exalt(item, ch)?, "exalt"),
                    "annul" => (m.annul(item, ch)?, "annul"),
                    "dominance" => (m.dominance(item, ch)?, "dominance"),
                    "fracture" => (m.fracture(item, ch)?, "fracture"),
                    "veiled_exalt" => (m.veiled_exalt(item, ch)?, "veiled_exalt"),
                    "scour" => (m.scour(item, ch)?, "scour"),
                    "remove_crafted" => return Ok((item_state(m.remove_crafted(item)), vec![])),
                    other => {
                        return Err(PrimErr::Recipe(
                            self.err(a, format!("unknown action {other}")),
                        ))
                    }
                };
                (item_state(it), one(key))
            }
            Op::FoulbornExalt(side) => {
                let g = side.as_deref().map(|s| {
                    if s == "prefix" {
                        Gen::Prefix
                    } else {
                        Gen::Suffix
                    }
                });
                (
                    item_state(m.foulborn_exalt(item, ch, g)?),
                    one("foulborn_exalt"),
                )
            }
            Op::Conqueror(inf) => {
                let i = Influence::parse(inf).ok_or_else(|| {
                    PrimErr::Recipe(self.err(a, format!("unknown influence {inf}")))
                })?;
                (
                    item_state(m.conqueror_exalt(item, ch, i)?),
                    one(&format!("{inf}_exalt")),
                )
            }
            Op::Unveil(prefs) => {
                let mut fams = Vec::new();
                for p in prefs {
                    fams.push(
                        self.resolve(p.s(), Some(item), a.line, &a.file)
                            .map_err(PrimErr::Recipe)?,
                    );
                }
                (item_state(m.unveil(item, ch, &fams)?), vec![])
            }
            Op::Awaken(donor) => {
                let Some((d, dv)) = st.get_aside(donor) else {
                    return Err(PrimErr::Recipe(self.err(
                        a,
                        format!("awaken: no set-aside item named '{donor}'. Use 'set_aside {donor}' on the donor item first."),
                    )));
                };
                let new = m.awaken(item, d, ch)?;
                (
                    State {
                        item: Some(new),
                        aside: st.without_aside(donor),
                        value: F64::new(st.value.get() + dv),
                        ..st.clone()
                    },
                    one("awakener"),
                )
            }
            Op::HarvestReforge(tag) => {
                let res = m.harvest_reforge(item, ch, tag)?;
                if tag == "influence" {
                    let (key, n) = crate::mechanics::HARVEST_REFORGE_INFLUENCE;
                    (
                        item_state(res),
                        self.cost_of(&[(key, n), ("lifeforce_sacred", 1.0)]),
                    )
                } else {
                    let (key, n) = harvest_cost(tag).unwrap();
                    (item_state(res), self.cost_of(&[(key, n)]))
                }
            }
            Op::HarvestAdd(tag) => {
                let res = m.harvest_add(item, ch, tag)?;
                let (key, n) = crate::mechanics::harvest_add_cost(tag).unwrap();
                (
                    item_state(res),
                    self.cost_of(&[(key, n), ("lifeforce_sacred", 1.0)]),
                )
            }
            Op::Essence(name) => {
                let e = self.essence_idx(a, name.s()).map_err(PrimErr::Recipe)?;
                let ename = format!("essence:{}", gd.essences[e].name);
                (item_state(m.essence(item, ch, e)?), one(&ename))
            }
            Op::Craft(opts) => {
                let mut last = None;
                for (i, what) in opts.iter().enumerate() {
                    let (mi, cost) = self.bench(a, item, what.s()).map_err(PrimErr::Recipe)?;
                    match m.bench_craft(item, mi) {
                        Ok(it) => return Ok((item_state(it), self.cost_owned(&cost))),
                        Err(e) if i + 1 < opts.len() => last = Some(e),
                        Err(e) => return Err(PrimErr::Craft(e)),
                    }
                }
                return Err(PrimErr::Craft(last.unwrap()));
            }
            Op::Beastcraft { what, influence } => {
                return self.beast(a, st, item, ch, what, influence.as_deref())
            }
            Op::Omen(what) => {
                if what != "blanching" {
                    return Err(PrimErr::Recipe(
                        self.err(a, format!("unknown omen '{what}' (supported: blanching)")),
                    ));
                }
                (
                    item_state(m.omen_blanching(item)),
                    self.cost_of(&[("omen_blanching", 1.0), ("chromatic", 1.0)]),
                )
            }
            _ => unreachable!(),
        })
    }

    fn buy(
        &self,
        a: &Act,
        b: &crate::lang::BuyArgs,
        st: &State,
    ) -> Result<(State, Cost), RecipeError> {
        let gd = self.gd;
        let base = gd.resolve_base(&b.base).map_err(|e| self.err(a, e.0))?;
        let mut infl = Influences::default();
        for i in &b.influences {
            let inf = Influence::parse(i).ok_or_else(|| self.err(a, format!("unknown influence '{i}' (use one of shaper, elder, crusader, redeemer, hunter, warlord)")))?;
            gd.influence_tag(base, inf).map_err(|e| self.err(a, e.0))?;
            infl = infl.with(inf);
        }
        let rarity = Rarity::parse(&b.rarity).unwrap_or(Rarity::Normal);
        let mut item = Item::new(base, b.ilvl, rarity, infl);
        item.tag = b.tag.as_deref().map(Arc::from);
        let mut mods: Vec<Roll> = Vec::new();
        for w in &b.with {
            let e = |msg: String| RecipeError::new(msg, Some(w.line), a.file.clone());
            let fam_i = gd
                .resolve_mod(w.r.s(), Some(gd.item_class(base)))
                .map_err(|x| e(x.0))?;
            let fam = &gd.families[fam_i as usize];
            let tiers = gd.tiers_for(fam_i, base);
            let r = w.r.s();
            let mi = if w.elevated {
                fam.elevated
                    .ok_or_else(|| e(format!("\"{r}\" has no elevated version")))?
            } else if let Some(t) = w.tier {
                if t < 1 || t as usize > tiers.len() {
                    let lv: Vec<String> = tiers
                        .iter()
                        .enumerate()
                        .map(|(i, m)| format!("T{}=ilvl{}", i + 1, gd.m(*m).level))
                        .collect();
                    return Err(e(format!(
                        "\"{r}\" has tiers 1..{} on this base (asked for tier {t}); levels: {}",
                        tiers.len(),
                        lv.join(", ")
                    )));
                }
                tiers[t as usize - 1]
            } else {
                match tiers.iter().find(|&&m| gd.m(m).level <= item.ilvl) {
                    Some(&m) => m,
                    None if tiers.is_empty() => {
                        return Err(e(format!("\"{r}\" cannot roll on {}", gd.base(base).name)))
                    }
                    None => {
                        return Err(e(format!(
                            "\"{r}\" needs item level {}+",
                            gd.m(*tiers.last().unwrap()).level
                        )))
                    }
                }
            };
            let m = gd.m(mi);
            if let Some(inf) = m.influence {
                if !item.influences.contains(inf) {
                    return Err(e(format!(
                        "\"{r}\" is a {} mod; add \"influence {}\" to the buy",
                        crate::data::title(inf.name()),
                        inf.name()
                    )));
                }
            }
            if !m.elevated && m.level > item.ilvl {
                return Err(e(format!(
                    "\"{}\" (tier {}) needs item level {}, item is {}",
                    m.short(),
                    gd.tier(mi, base),
                    m.level,
                    item.ilvl
                )));
            }
            if mods
                .iter()
                .any(|x| gd.m(x.m).group_ids.iter().any(|g| m.group_ids.contains(g)))
            {
                return Err(e(format!(
                    "two mods of the same group ({}) on one item",
                    m.groups.first().cloned().unwrap_or_default()
                )));
            }
            mods.push(Roll {
                m: mi,
                fractured: w.fractured,
                crafted: false,
            });
        }
        let item = item.with_mods(mods);
        let mx = item.max_affixes(gd);
        if item.n_side(gd, Gen::Prefix) > mx || item.n_side(gd, Gen::Suffix) > mx {
            return Err(self.err(
                a,
                format!(
                    "a {} item can have at most {mx} prefix(es) and {mx} suffix(es)",
                    item.rarity.name()
                ),
            ));
        }
        let mut cost = self.cost_owned(&b.cost);
        let value = self.chaos(&cost);
        if st.item.is_some() && st.value.get() != 0.0 {
            cost.push((self.cur(LOST), st.value.get()));
        }
        let imprint = if st.item.is_some() {
            None
        } else {
            st.imprint.clone()
        };
        Ok((
            State {
                item: Some(item),
                value: F64::new(value),
                imprint,
                ..st.clone()
            },
            cost,
        ))
    }

    fn essence_idx(&self, a: &Act, name: &str) -> Result<usize, RecipeError> {
        let low = name.to_lowercase();
        let ess = &self.gd.essences;
        let mut hits: Vec<usize> = (0..ess.len())
            .filter(|&i| ess[i].name.to_lowercase() == low)
            .collect();
        if hits.is_empty() {
            hits = (0..ess.len())
                .filter(|&i| ess[i].name.to_lowercase().contains(&low))
                .collect();
        }
        if hits.len() != 1 {
            let mut names: Vec<&str> = hits.iter().map(|&i| ess[i].name.as_str()).collect();
            names.sort();
            let tail = if hits.is_empty() {
                "not found".to_string()
            } else {
                format!(
                    "is ambiguous: {}",
                    names
                        .iter()
                        .take(12)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            return Err(self.err(a, format!("essence \"{name}\" {tail}")));
        }
        Ok(hits[0])
    }

    const BENCH_BUILTINS: [(&'static str, MetaKind); 5] = [
        ("lock_suffixes", MetaKind::LockSuffixes(None)),
        ("suffixes_cannot_be_changed", MetaKind::LockSuffixes(None)),
        ("lock_prefixes", MetaKind::LockPrefixes(None)),
        ("prefixes_cannot_be_changed", MetaKind::LockPrefixes(None)),
        ("multimod", MetaKind::Multimod),
    ];

    /// Bench option for `what` on this item's class: builtin name, alias, mod id or text.
    fn bench(
        &self,
        a: &Act,
        item: &Item,
        what: &str,
    ) -> Result<(u32, Vec<(String, f64)>), RecipeError> {
        let gd = self.gd;
        let cls = gd.item_class(item.base);
        let extra: [(&str, MetaKind); 6] = [
            ("lock_1_suffix", MetaKind::LockSuffixes(Some(1))),
            ("lock_2_suffixes", MetaKind::LockSuffixes(Some(2))),
            ("lock_1_prefix", MetaKind::LockPrefixes(Some(1))),
            ("lock_2_prefixes", MetaKind::LockPrefixes(Some(2))),
            ("no_attack_mods", MetaKind::NoAttackMods),
            ("no_caster_mods", MetaKind::NoCasterMods),
        ];
        if let Some((_, kind)) = Self::BENCH_BUILTINS
            .iter()
            .chain(extra.iter())
            .find(|x| x.0 == what)
        {
            let opt = gd
                .metamod_for(cls, *kind)
                .ok_or_else(|| self.err(a, format!("bench craft {what} not available on {cls}")))?;
            return Ok((
                opt.module,
                opt.cost.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            ));
        }
        let low = gd
            .aliases
            .get(&what.to_lowercase())
            .cloned()
            .unwrap_or_else(|| what.to_string())
            .to_lowercase();
        let hits: Vec<&crate::data::BenchOption> = gd
            .bench
            .iter()
            .filter(|b| b.classes.iter().any(|c| c == cls))
            .filter(|b| {
                gd.m(b.module).text.to_lowercase().contains(&low)
                    || gd.m(b.module).id.to_lowercase() == low
            })
            .collect();
        if hits.is_empty() {
            let names: Vec<&str> = Self::BENCH_BUILTINS
                .iter()
                .map(|x| x.0)
                .chain(extra.iter().map(|x| x.0))
                .collect();
            return Err(self.err(
                a,
                format!(
                    "no bench craft matching \"{what}\" for {cls}. Builtins: {}",
                    names.join(", ")
                ),
            ));
        }
        let groups: std::collections::BTreeSet<&str> = hits
            .iter()
            .map(|b| {
                gd.m(b.module)
                    .groups
                    .first()
                    .map(|s| s.as_str())
                    .unwrap_or("")
            })
            .collect();
        if groups.len() > 1 {
            let mut t: Vec<String> = hits.iter().map(|b| gd.m(b.module).short()).collect();
            t.sort();
            t.dedup();
            let s = t.join("; ");
            return Err(self.err(
                a,
                format!(
                    "bench craft \"{what}\" is ambiguous: {}",
                    &s[..s.len().min(400)]
                ),
            ));
        }
        let best = hits
            .iter()
            .max_by(|x, y| {
                gd.m(x.module)
                    .level
                    .cmp(&gd.m(y.module).level)
                    .then(gd.m(x.module).id.cmp(&gd.m(y.module).id))
            })
            .unwrap();
        Ok((
            best.module,
            best.cost.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        ))
    }

    fn beast(
        &self,
        a: &Act,
        st: &State,
        item: &Item,
        ch: &mut dyn Chooser,
        what: &str,
        influence: Option<&str>,
    ) -> Result<(State, Cost), PrimErr> {
        let m = &self.mech;
        let bc = |n: &str| -> Cost {
            beast_cost(n)
                .iter()
                .map(|(k, v)| (self.cur(k), *v))
                .collect()
        };
        let item_state = |it: Item| State {
            item: Some(it),
            ..st.clone()
        };
        Ok(match what {
            "lock_suffixes" | "lock_prefixes" | "multimod" => {
                let kind = match what {
                    "lock_suffixes" => MetaKind::LockSuffixes(None),
                    "lock_prefixes" => MetaKind::LockPrefixes(None),
                    _ => MetaKind::Multimod,
                };
                (item_state(m.beast_metamod(item, kind)?), bc("metamod"))
            }
            "imprint" => {
                if item.rarity != Rarity::Magic {
                    return Err(PrimErr::Recipe(self.err(a, "beastcraft imprint needs a magic item")));
                }
                (State { imprint: Some(item.clone()), ..st.clone() }, bc("imprint"))
            }
            "restore_imprint" => {
                let Some(imp) = &st.imprint else {
                    return Err(PrimErr::Recipe(self.err(a, "no imprint to restore (use 'beastcraft imprint' first)")));
                };
                (State { item: Some(imp.clone()), imprint: None, ..st.clone() }, vec![])
            }
            "hinekoras_lock" => (item_state(m.hinekoras_lock(item)?), bc("hinekoras_lock")),
            "max_links" => (item_state(m.max_links(item)), bc("max_links")),
            "add_mod" => {
                let inf = influence.unwrap_or("");
                let i = match inf {
                    "hunter" => Influence::Hunter,
                    "warlord" => Influence::Warlord,
                    "redeemer" => Influence::Redeemer,
                    _ => return Err(PrimErr::Recipe(self.err(a, "beastcraft add_mod supports hunter, warlord, redeemer"))),
                };
                (item_state(m.beast_add_influence_mod(item, ch, i)?), bc(&format!("add_{inf}_mod")))
            }
            "add_prefix_remove_suffix" | "add_suffix_remove_prefix" => {
                let g = if what == "add_prefix_remove_suffix" { Gen::Prefix } else { Gen::Suffix };
                (item_state(m.beast_swap_side(item, ch, g)?), bc(what))
            }
            other => {
                return Err(PrimErr::Recipe(self.err(
                    a,
                    format!("unknown beastcraft '{other}'. Known: lock_suffixes, lock_prefixes, multimod, imprint, restore_imprint, hinekoras_lock, max_links, add_mod <influence>, add_prefix_remove_suffix, add_suffix_remove_prefix"),
                )))
            }
        })
    }

    // -------------------------------------------------------------- i.i.d. loops
    /// A loop whose body is one "full reroll" action (alteration, chaos, unlocked harvest
    /// reforge) is a sequence of independent identical trials, solved in closed form.
    pub(crate) fn iid_loop(
        &self,
        l: &crate::lang::Loop,
        st: &State,
    ) -> Result<Option<Rc<Iid>>, RecipeError> {
        let [Node::Act(act)] = l.body.body.as_slice() else {
            return Ok(None);
        };
        let op = match &act.op {
            Op::Simple(s) if *s == "alteration" || *s == "chaos" => *s,
            Op::HarvestReforge(_) => "harvest_reforge",
            _ => return Ok(None),
        };
        let Some(item) = &st.item else {
            return Ok(None);
        };
        let gd = self.gd;
        match op {
            "alteration" if item.rarity != Rarity::Magic => return Ok(None),
            "chaos" | "harvest_reforge" if item.rarity != Rarity::Rare => return Ok(None),
            _ => {}
        }
        let locked = [Gen::Prefix, Gen::Suffix]
            .iter()
            .map(|g| item.lock_on(gd, *g))
            .collect::<Vec<_>>();
        if op == "harvest_reforge" && locked.iter().any(|l| *l != Lock::None) {
            return Ok(None); // the reforge consumes the lock: later iterations differ
        }
        if locked.iter().any(|l| matches!(l, Lock::Count(_))) {
            return Ok(None); // partial locks pick protected mods at random: not i.i.d.
        }
        let kept: Vec<Roll> = if op == "alteration" {
            item.mods.iter().filter(|r| r.fractured).copied().collect()
        } else {
            let mut dummy = RandomChooser::new(0);
            let prot = self.mech.protected(item, &mut dummy);
            item.mods
                .iter()
                .zip(prot)
                .filter(|(_, p)| *p)
                .map(|(r, _)| *r)
                .collect()
        };
        let key = IidKey {
            act: act as *const Act as usize,
            node: l as *const _ as usize,
            base: item.base,
            ilvl: item.ilvl,
            infl: item.influences,
            rarity: item.rarity,
            kept,
            links: item.links,
            white: item.white_sockets,
            tag: item.tag.clone(),
        };
        if let Some(v) = self.iid_cache.borrow().get(&key) {
            return Ok(v.clone());
        }
        let mut budget = 200_000u64;
        let mut outs: Vec<(State, Cost, f64)> = Vec::new();
        let mut err = None;
        let res = enumerate(
            |ch| self.primitive(act, st, ch),
            &mut budget,
            |r, p| match r {
                Ok((s, c)) => {
                    outs.push((s, c, p));
                    true
                }
                Err(e) => {
                    err = Some(e);
                    false
                }
            },
        );
        if let Some(e) = err {
            return Err(e);
        }
        if res.is_err() {
            self.iid_cache.borrow_mut().insert(key, None);
            return Ok(None);
        }
        let mut succ: FxHashMap<Item, f64> = FxHashMap::default();
        let mut fail: FxHashMap<Item, f64> = FxHashMap::default();
        let mut cost = vec![];
        for (s, c, p) in outs {
            cost = c;
            let it = s.item.clone().unwrap();
            if self.loop_exit(l, &s)? {
                *succ.entry(it).or_insert(0.0) += p;
            } else {
                *fail.entry(it).or_insert(0.0) += p;
            }
        }
        let p: f64 = succ.values().sum();
        let iid = Rc::new(Iid {
            succ: succ.into_iter().collect(),
            fail: fail.into_iter().collect(),
            p,
            cost,
        });
        self.iid_cache.borrow_mut().insert(key, Some(iid.clone()));
        Ok(Some(iid))
    }

    // -------------------------------------------------------------- Monte Carlo
    /// Run the recipe once with random choices.
    pub fn run_mc(
        &self,
        prog: &Program,
        seed: u64,
        max_steps: u64,
        mut trace: Option<&mut Vec<String>>,
    ) -> Result<RunResult, RecipeError> {
        let mut ch = RandomChooser::new(seed);
        let mut costs: Vec<f64> = vec![];
        let mut steps = 0u64;
        let st = self.mc_seq(
            &prog.body,
            State::default(),
            &mut ch,
            &mut costs,
            &mut steps,
            max_steps,
            &mut trace,
        )?;
        self.finish(prog, st, costs, steps)
    }

    fn add_cost(costs: &mut Vec<f64>, c: &[(Cur, f64)], times: f64) {
        for &(k, v) in c {
            let k = k as usize;
            if costs.len() <= k {
                costs.resize(k + 1, 0.0);
            }
            costs[k] += v * times;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn mc_seq(
        &self,
        s: &Seq,
        mut st: State,
        ch: &mut RandomChooser,
        costs: &mut Vec<f64>,
        steps: &mut u64,
        max: u64,
        trace: &mut Option<&mut Vec<String>>,
    ) -> Result<State, RecipeError> {
        for n in &s.body {
            if st.status != Status::Run {
                break;
            }
            st = self.mc_node(n, st, ch, costs, steps, max, trace)?;
        }
        Ok(st)
    }

    #[allow(clippy::too_many_arguments)]
    fn mc_node(
        &self,
        n: &Node,
        st: State,
        ch: &mut RandomChooser,
        costs: &mut Vec<f64>,
        steps: &mut u64,
        max: u64,
        trace: &mut Option<&mut Vec<String>>,
    ) -> Result<State, RecipeError> {
        if st.status != Status::Run {
            return Ok(st);
        }
        if let Node::Seq(s) = n {
            return self.mc_seq(s, st, ch, costs, steps, max, trace);
        }
        *steps += 1;
        if *steps > max {
            let (line, file) = node_pos(n);
            return Err(RecipeError::new(format!("run exceeded {max} steps: a loop probably never ends (check its condition, or add 'limit N')"), Some(line), file));
        }
        match n {
            Node::Act(a) => {
                let (st2, c) = self.primitive(a, &st, ch)?;
                Self::add_cost(costs, &c, 1.0);
                if let (Some(t), Op::Show(msg)) = (trace.as_deref_mut(), &a.op) {
                    let label = if msg.is_empty() {
                        format!("line {}", a.line)
                    } else {
                        msg.clone()
                    };
                    t.push(format!(
                        "{label}\n{}",
                        st.item
                            .as_ref()
                            .map(|i| i.describe(self.gd))
                            .unwrap_or_else(|| "(no item)".into())
                    ));
                }
                Ok(st2)
            }
            Node::If(i) => {
                if self.cond(&i.cond, &st, i.line, &i.file)? {
                    self.mc_seq(&i.then, st, ch, costs, steps, max, trace)
                } else if let Some(o) = &i.other {
                    self.mc_seq(o, st, ch, costs, steps, max, trace)
                } else {
                    Ok(st)
                }
            }
            Node::Loop(l) => {
                if l.check_first && self.loop_exit(l, &st)? {
                    return Ok(st);
                }
                if let Some(iid) = self.iid_loop(l, &st)? {
                    return self.mc_iid(l, st, &iid, ch, costs);
                }
                let mut st = st;
                let mut k = 0u64;
                loop {
                    st = self.mc_seq(&l.body, st, ch, costs, steps, max, trace)?;
                    k += 1;
                    if st.status != Status::Run || self.loop_exit(l, &st)? {
                        return Ok(st);
                    }
                    if l.limit.is_some_and(|lim| k >= lim) {
                        return Ok(st);
                    }
                }
            }
            Node::Foresee(f) => {
                let Some(it) = &st.item else {
                    return Err(RecipeError::new("'foresee' needs an item with Hinekora's Lock (beastcraft hinekoras_lock on a magic item first)", Some(f.line), f.file.clone()));
                };
                if !it.preview_lock {
                    return Err(RecipeError::new("'foresee' needs an item with Hinekora's Lock (beastcraft hinekoras_lock on a magic item first)", Some(f.line), f.file.clone()));
                }
                let (st2, c) = self.primitive(&f.action, &st, ch)?;
                if self.cond(&f.cond, &st2, f.line, &f.file)? {
                    Self::add_cost(costs, &c, 1.0);
                    return Ok(st2);
                }
                let st = st.with_item(Item {
                    preview_lock: false,
                    ..it.clone()
                });
                match &f.other {
                    Some(o) => self.mc_seq(o, st, ch, costs, steps, max, trace),
                    None => Ok(st),
                }
            }
            Node::Seq(_) | Node::Call(_) => unreachable!("calls are expanded at parse time"),
        }
    }

    fn mc_iid(
        &self,
        l: &crate::lang::Loop,
        st: State,
        iid: &Iid,
        ch: &mut RandomChooser,
        costs: &mut Vec<f64>,
    ) -> Result<State, RecipeError> {
        if iid.p <= 0.0 && l.limit.is_none() {
            return Err(RecipeError::new(
                "this loop can never finish: its exit condition cannot be reached by this action",
                Some(l.line),
                l.file.clone(),
            ));
        }
        let n: f64 = if iid.p >= 1.0 {
            1.0
        } else if iid.p <= 0.0 {
            f64::INFINITY
        } else {
            use rand::RngExt;
            let u: f64 = 1.0 - ch.rng.random::<f64>();
            1.0 + (u.ln() / (-iid.p).ln_1p()).floor()
        };
        let (n, pool) = match l.limit {
            Some(lim) if n > lim as f64 => (lim as f64, &iid.fail),
            _ => (n, &iid.succ),
        };
        Self::add_cost(costs, &iid.cost, n);
        let tot: f64 = pool.iter().map(|x| x.1).sum();
        use rand::RngExt;
        let mut x = ch.rng.random::<f64>() * tot;
        let mut pick = &pool[pool.len() - 1].0;
        for (it, p) in pool {
            if x < *p {
                pick = it;
                break;
            }
            x -= p;
        }
        Ok(st.with_item(pick.clone()))
    }

    fn finish(
        &self,
        prog: &Program,
        st: State,
        costs: Vec<f64>,
        steps: u64,
    ) -> Result<RunResult, RecipeError> {
        let mut ok = st.status != Status::Fail;
        if ok {
            if let Some(g) = &prog.goal {
                ok = st.item.is_some() && self.cond(g, &st, 0, &None)?;
            }
        }
        let mut costs: Vec<(String, f64)> = costs
            .iter()
            .enumerate()
            .filter(|(_, v)| **v != 0.0)
            .map(|(i, v)| (self.cur_name(i as Cur), *v))
            .collect();
        if !ok && st.value.get() != 0.0 {
            match costs.iter_mut().find(|x| x.0 == LOST) {
                Some(x) => x.1 += st.value.get(),
                None => costs.push((LOST.into(), st.value.get())),
            }
        }
        let mut tracks = vec![];
        for (lbl, c) in &prog.tracks {
            tracks.push((
                lbl.clone(),
                st.item.is_some() && self.cond(c, &st, 0, &None)?,
            ));
        }
        let revenue = self.revenue(prog, &st)?;
        Ok(RunResult {
            success: ok,
            costs,
            msg: st.msg.to_string(),
            item: st.item,
            steps,
            tracks,
            revenue,
        })
    }

    /// Chaos value of the final item from the recipe's `sell` lines (first match wins).
    pub fn revenue(&self, prog: &Program, st: &State) -> Result<f64, RecipeError> {
        if st.item.is_none() {
            return Ok(0.0);
        }
        for (amt, c) in &prog.sales {
            if self.cond(c, st, 0, &None)? {
                return Ok(self.prices.chaos(amt));
            }
        }
        Ok(0.0)
    }
}

pub(crate) enum PrimErr {
    Craft(CraftError),
    Recipe(RecipeError),
}
impl From<CraftError> for PrimErr {
    fn from(e: CraftError) -> Self {
        PrimErr::Craft(e)
    }
}

pub fn node_pos(n: &Node) -> (usize, Option<Arc<str>>) {
    match n {
        Node::Act(a) => (a.line, a.file.clone()),
        Node::Seq(s) => (s.line, s.file.clone()),
        Node::Loop(l) => (l.line, l.file.clone()),
        Node::If(i) => (i.line, i.file.clone()),
        Node::Foresee(f) => (f.line, f.file.clone()),
        Node::Call(c) => (c.line, c.file.clone()),
    }
}
