//! Exact engine (port of the exact half of `poecraft/engine.py`).
//!
//! Works on probability distributions over hashable states. Primitive actions are enumerated
//! exhaustively. Loops are absorbing Markov chains: transient states are discovered by running
//! the loop body, then absorption probabilities, expected currency counts and the second
//! moment of the chaos cost come from linear systems (I - Q) X = R.
//!
//! Differences from the Python reference (same results, less work):
//! * transitions stay sparse; only A = I - Q is dense and it is factorised once;
//! * quantities that are only needed for the start state use z = A^-T e0 (one extra solve)
//!   instead of solving for every row.

use crate::engine::{Cur, Engine, Iid, State, Status, LOST};
use crate::enumerate::enumerate;
use crate::lang::{Act, Loop, Node, Program, RecipeError, Seq};
use crate::linalg::{Lu, Mat};
use rustc_hash::{FxHashMap, FxHashSet};
use std::rc::Rc;
use std::time::Instant;

#[derive(Debug, Clone)]
pub enum ExactErr {
    /// Too big to solve exactly (the caller falls back to Monte Carlo).
    TooLarge(String),
    Recipe(RecipeError),
}
impl From<RecipeError> for ExactErr {
    fn from(e: RecipeError) -> Self {
        ExactErr::Recipe(e)
    }
}
type XResult<T> = Result<T, ExactErr>;

/// P = probability, s1 = P-weighted expected currency counts (by Cur), s2 = P-weighted second
/// moment of the chaos cost.
#[derive(Debug, Clone, Default)]
pub struct Entry {
    pub p: f64,
    pub s1: Vec<f64>,
    pub s2: f64,
}

pub type Dist = FxHashMap<State, Entry>;

fn acc(d: &mut Dist, s: State, p: f64, s1: &[f64], s2: f64) {
    let e = d.entry(s).or_default();
    e.p += p;
    if e.s1.len() < s1.len() {
        e.s1.resize(s1.len(), 0.0);
    }
    for (a, b) in e.s1.iter_mut().zip(s1) {
        *a += b;
    }
    e.s2 += s2;
}

fn point(s: State) -> Dist {
    let mut d = Dist::default();
    d.insert(s, Entry { p: 1.0, s1: vec![], s2: 0.0 });
    d
}

#[derive(Hash, PartialEq, Eq, Clone)]
enum SigState {
    State(State),
    Dist(usize),
}

#[derive(Hash, PartialEq, Eq, Clone)]
struct Sig {
    k: usize,
    st: SigState,
    cost: Vec<(usize, u64)>,
}

pub struct Budget {
    pub paths: u64,
    pub states: usize,
    pub deadline: Instant,
}

pub struct Exact<'e, 'a> {
    pub eng: &'e Engine<'a>,
    cache: FxHashMap<(usize, State), Rc<Dist>>,
    loop_cache: FxHashMap<(usize, Sig), Rc<Dist>>,
    shared: FxHashSet<usize>,
    budget: Budget,
}

#[derive(Debug, Clone, Default)]
pub struct ExactResult {
    pub p_success: f64,
    pub mean_counts: Vec<(String, f64)>,
    pub second_moment: f64,
    pub mean_chaos: f64,
    pub tracks: Vec<(String, f64)>,
    pub revenue: f64,
    pub n_outcomes: usize,
}

impl<'e, 'a> Exact<'e, 'a> {
    pub fn new(eng: &'e Engine<'a>, budget: Budget) -> Self {
        Exact { eng, cache: FxHashMap::default(), loop_cache: FxHashMap::default(), shared: FxHashSet::default(), budget }
    }

    fn chaos(&self, v: &[f64]) -> f64 {
        v.iter().enumerate().map(|(c, n)| if *n != 0.0 { self.eng.price(c as Cur) * n } else { 0.0 }).sum()
    }

    fn check_time(&self) -> XResult<()> {
        if Instant::now() > self.budget.deadline {
            return Err(ExactErr::TooLarge("exact computation exceeded its time budget".into()));
        }
        Ok(())
    }

    pub fn run(mut self, prog: &Program) -> XResult<ExactResult> {
        let out = self.seq(&prog.body, State::default())?;
        let eng = self.eng;
        let mut res = ExactResult::default();
        let mut counts: Vec<f64> = vec![];
        let mut tracks: Vec<f64> = vec![0.0; prog.tracks.len()];
        let lost = eng.cur(LOST) as usize;
        for (st, e) in out.iter() {
            let mut ok = st.status != Status::Fail;
            if ok {
                if let Some(g) = &prog.goal {
                    ok = st.item.is_some() && eng.cond(g, st, 0, &None)?;
                }
            }
            if ok {
                res.p_success += e.p;
            }
            if counts.len() < e.s1.len() {
                counts.resize(e.s1.len(), 0.0);
            }
            for (a, b) in counts.iter_mut().zip(&e.s1) {
                *a += b;
            }
            if !ok && st.value.get() != 0.0 {
                if counts.len() <= lost {
                    counts.resize(lost + 1, 0.0);
                }
                counts[lost] += e.p * st.value.get();
            }
            res.second_moment += e.s2;
            for (i, (_, c)) in prog.tracks.iter().enumerate() {
                if st.item.is_some() && eng.cond(c, st, 0, &None)? {
                    tracks[i] += e.p;
                }
            }
            res.revenue += e.p * eng.revenue(prog, st)?;
        }
        res.n_outcomes = out.len();
        res.mean_chaos = counts.iter().enumerate().map(|(c, n)| eng.price(c as Cur) * n).sum();
        res.mean_counts = counts.iter().enumerate().filter(|(_, v)| **v != 0.0).map(|(c, v)| (eng.cur_name(c as Cur), *v)).collect();
        res.tracks = prog.tracks.iter().map(|(l, _)| l.clone()).zip(tracks).collect();
        Ok(res)
    }

    // -------------------------------------------------------------- recursion
    fn seq(&mut self, s: &Seq, st: State) -> XResult<Rc<Dist>> {
        if st.status != Status::Run {
            return Ok(Rc::new(point(st)));
        }
        let key = (s as *const Seq as usize, st.clone());
        if let Some(d) = self.cache.get(&key) {
            return Ok(d.clone());
        }
        let mut dist = point(st);
        for n in &s.body {
            dist = self.compose(&dist, n)?;
        }
        let d = Rc::new(dist);
        self.cache.insert(key, d.clone());
        Ok(d)
    }

    fn node(&mut self, n: &Node, st: State) -> XResult<Rc<Dist>> {
        if st.status != Status::Run {
            return Ok(Rc::new(point(st)));
        }
        let key = (n as *const Node as usize, st.clone());
        if let Some(d) = self.cache.get(&key) {
            return Ok(d.clone());
        }
        let out: Rc<Dist> = match n {
            Node::Seq(s) => self.seq(s, st)?,
            Node::Act(a) => Rc::new(self.primitive(a, &st)?),
            Node::If(i) => {
                if self.eng.cond(&i.cond, &st, i.line, &i.file)? {
                    self.seq(&i.then, st)?
                } else if let Some(o) = &i.other {
                    self.seq(o, st)?
                } else {
                    Rc::new(point(st))
                }
            }
            Node::Loop(l) => self.lp(l, st)?,
            Node::Foresee(f) => {
                let locked = st.item.as_ref().is_some_and(|i| i.preview_lock);
                if !locked {
                    return Err(RecipeError::new("'foresee' needs an item with Hinekora's Lock", Some(f.line), f.file.clone()).into());
                }
                let outs = self.primitive(&f.action, &st)?;
                let unlocked = st.with_item(crate::item::Item { preview_lock: false, ..st.item.clone().unwrap() });
                let other = match &f.other {
                    Some(o) => self.seq(o, unlocked)?,
                    None => Rc::new(point(unlocked)),
                };
                let mut out = Dist::default();
                for (s2, e) in outs.iter() {
                    if self.eng.cond(&f.cond, s2, f.line, &f.file)? {
                        acc(&mut out, s2.clone(), e.p, &e.s1, e.s2);
                    } else {
                        for (s3, e3) in other.iter() {
                            let s1: Vec<f64> = e3.s1.iter().map(|v| v * e.p).collect();
                            acc(&mut out, s3.clone(), e.p * e3.p, &s1, e.p * e3.s2);
                        }
                    }
                }
                Rc::new(out)
            }
            Node::Call(_) => unreachable!("calls are expanded at parse time"),
        };
        self.cache.insert(key, out.clone());
        Ok(out)
    }

    fn compose(&mut self, dist: &Dist, n: &Node) -> XResult<Dist> {
        let mut out = Dist::default();
        for (s, x) in dist.iter() {
            let x1c = self.chaos(&x.s1);
            let d = self.node(n, s.clone())?;
            for (t, y) in d.iter() {
                let len = x.s1.len().max(y.s1.len());
                let mut s1 = vec![0.0; len];
                for (i, v) in x.s1.iter().enumerate() {
                    s1[i] += v * y.p;
                }
                for (i, v) in y.s1.iter().enumerate() {
                    s1[i] += x.p * v;
                }
                let y1c = self.chaos(&y.s1);
                acc(&mut out, t.clone(), x.p * y.p, &s1, x.s2 * y.p + 2.0 * x1c * y1c + x.p * y.s2);
            }
        }
        Ok(out)
    }

    fn primitive(&mut self, a: &Act, st: &State) -> XResult<Dist> {
        self.check_time()?;
        let eng = self.eng;
        let mut raw: FxHashMap<(State, Vec<(Cur, u64)>), f64> = FxHashMap::default();
        let mut err: Option<RecipeError> = None;
        let mut budget = self.budget.paths;
        let deadline = self.budget.deadline;
        let mut n = 0u64;
        let r = enumerate(
            |ch| eng.primitive(a, st, ch),
            &mut budget,
            |res, p| {
                n += 1;
                match res {
                    Ok((s, c)) => {
                        let mut c: Vec<(Cur, u64)> = c.iter().map(|(k, v)| (*k, v.to_bits())).collect();
                        c.sort_unstable();
                        *raw.entry((s, c)).or_insert(0.0) += p;
                        n % 4096 != 0 || Instant::now() <= deadline
                    }
                    Err(e) => {
                        err = Some(e);
                        false
                    }
                }
            },
        );
        if let Some(e) = err {
            return Err(e.into());
        }
        if r.is_err() {
            return Err(ExactErr::TooLarge("too many random outcomes to enumerate exactly".into()));
        }
        self.budget.paths = budget;
        let mut out = Dist::default();
        for ((s, c), p) in raw {
            let n = eng.n_curs();
            let mut s1 = vec![0.0; n];
            let mut ch = 0.0;
            for (k, v) in c {
                let v = f64::from_bits(v);
                s1[k as usize] += p * v;
                ch += eng.price(k) * v;
            }
            acc(&mut out, s, p, &s1, p * ch * ch);
        }
        Ok(out)
    }

    // -------------------------------------------------------------- loops
    fn lp(&mut self, l: &Loop, s0: State) -> XResult<Rc<Dist>> {
        if l.check_first && self.eng.loop_exit(l, &s0)? {
            return Ok(Rc::new(point(s0)));
        }
        if let Some(iid) = self.eng.iid_loop(l, &s0)? {
            return Ok(Rc::new(self.iid(l, &s0, &iid)?));
        }
        let sig = self.signature(&l.body, &s0)?;
        let key = (l as *const Loop as usize, sig);
        if let Some(d) = self.loop_cache.get(&key) {
            return Ok(d.clone());
        }
        let res = Rc::new(self.solve(l, s0)?);
        self.shared.insert(Rc::as_ptr(&res) as usize);
        self.loop_cache.insert(key, res.clone());
        Ok(res)
    }

    fn iid(&self, l: &Loop, s0: &State, iid: &Iid) -> XResult<Dist> {
        let eng = self.eng;
        let (p, q) = (iid.p, 1.0 - iid.p);
        let c = eng.chaos(&iid.cost);
        let n = eng.n_curs();
        let cost_vec = |scale: f64| -> Vec<f64> {
            let mut v = vec![0.0; n];
            for (k, x) in &iid.cost {
                v[*k as usize] += scale * x;
            }
            v
        };
        let mut out = Dist::default();
        match l.limit {
            None => {
                if p <= 0.0 {
                    return Err(RecipeError::new("this loop can never finish: its exit condition cannot be reached by this action", Some(l.line), l.file.clone()).into());
                }
                for (it, po) in &iid.succ {
                    let w = po / p;
                    acc(&mut out, s0.with_item(it.clone()), w, &cost_vec(w / p), w * c * c * (2.0 - p) / (p * p));
                }
            }
            Some(lim) => {
                let (mut en_s, mut en2_s) = (0.0, 0.0);
                for k in 1..=lim {
                    let pk = q.powi(k as i32 - 1) * p;
                    en_s += k as f64 * pk;
                    en2_s += (k * k) as f64 * pk;
                }
                let pf = q.powi(lim as i32);
                for (it, po) in &iid.succ {
                    let w = po / p;
                    acc(&mut out, s0.with_item(it.clone()), w * (1.0 - pf), &cost_vec(w * en_s), w * en2_s * c * c);
                }
                for (it, po) in &iid.fail {
                    let w = if q > 0.0 { po / q } else { 0.0 };
                    let l = lim as f64;
                    acc(&mut out, s0.with_item(it.clone()), w * pf, &cost_vec(w * pf * l), w * pf * l * l * c * c);
                }
            }
        }
        Ok(out)
    }

    /// Signature of "run seq from st" for loop lumping: run the longest deterministic prefix
    /// (one outcome, fixed cost); states with the same result, prefix length and cost behave
    /// identically. A shared cached loop solution also makes states equivalent.
    fn signature(&mut self, seq: &Seq, st: &State) -> XResult<Sig> {
        let mut cur = st.clone();
        let mut cost: Vec<f64> = vec![];
        let pack = |cost: &Vec<f64>| -> Vec<(usize, u64)> { cost.iter().enumerate().filter(|(_, v)| **v != 0.0).map(|(i, v)| (i, v.to_bits())).collect() };
        for (k, n) in seq.body.iter().enumerate() {
            if cur.status != Status::Run {
                return Ok(Sig { k, st: SigState::State(cur), cost: pack(&cost) });
            }
            let d = self.node(n, cur.clone())?;
            let ptr = Rc::as_ptr(&d) as usize;
            let deterministic = d.len() == 1 && {
                let (_, e) = d.iter().next().unwrap();
                let c = self.chaos(&e.s1);
                (e.p - 1.0).abs() <= 1e-12 && (e.s2 - c * c).abs() <= 1e-6 * (c * c).max(1.0)
            };
            if !deterministic {
                let st = if self.shared.contains(&ptr) { SigState::Dist(ptr) } else { SigState::State(cur) };
                return Ok(Sig { k, st, cost: pack(&cost) });
            }
            let (s2, e) = d.iter().next().unwrap();
            if cost.len() < e.s1.len() {
                cost.resize(e.s1.len(), 0.0);
            }
            for (a, b) in cost.iter_mut().zip(&e.s1) {
                *a += b;
            }
            cur = s2.clone();
        }
        Ok(Sig { k: seq.body.len(), st: SigState::State(cur), cost: pack(&cost) })
    }

    fn solve(&mut self, l: &Loop, s0: State) -> XResult<Dist> {
        struct Row {
            absorbed: bool,
            target: usize,
            q: f64,
            y1: Vec<f64>,
            y1c: f64,
            y2: f64,
        }
        let limit = l.limit;
        let mut idx: FxHashMap<(Sig, u64), usize> = FxHashMap::default();
        let mut reps: Vec<(State, u64)> = vec![];
        let mut absorbed: FxHashMap<State, usize> = FxHashMap::default();
        let mut absorbed_list: Vec<State> = vec![];
        let mut trans: Vec<Vec<Row>> = vec![];

        let sig0 = self.signature(&l.body, &s0)?;
        idx.insert((sig0, 0), 0);
        reps.push((s0, 0));
        let mut i = 0;
        while i < reps.len() {
            let (s, k) = reps[i].clone();
            let d = self.seq(&l.body, s)?;
            let mut rows = vec![];
            for (t, e) in d.iter() {
                let exit = t.status != Status::Run || self.eng.loop_exit(l, t)?;
                let at_limit = limit.is_some_and(|lim| k + 1 >= lim);
                let y1c = self.chaos(&e.s1);
                if exit || at_limit {
                    let u = *absorbed.entry(t.clone()).or_insert_with(|| {
                        absorbed_list.push(t.clone());
                        absorbed_list.len() - 1
                    });
                    rows.push(Row { absorbed: true, target: u, q: e.p, y1: e.s1.clone(), y1c, y2: e.s2 });
                } else {
                    let kk = if limit.is_some() { k + 1 } else { 0 };
                    let sig = self.signature(&l.body, t)?;
                    let key = (sig, kk);
                    let tgt = match idx.get(&key) {
                        Some(&x) => x,
                        None => {
                            let x = reps.len();
                            idx.insert(key, x);
                            reps.push((t.clone(), kk));
                            if reps.len() > self.budget.states {
                                return Err(ExactErr::TooLarge(format!("loop at line {} has more than {} states", l.line, self.budget.states)));
                            }
                            x
                        }
                    };
                    rows.push(Row { absorbed: false, target: tgt, q: e.p, y1: e.s1.clone(), y1c, y2: e.s2 });
                }
            }
            trans.push(rows);
            i += 1;
            self.check_time()?;
        }
        let (n, m) = (reps.len(), absorbed_list.len());
        let ncur = self.eng.n_curs();
        let never = || -> ExactErr {
            RecipeError::new("this loop can never finish from some reachable state (its exit condition is unreachable); add 'limit N' or fix the condition", Some(l.line), l.file.clone()).into()
        };
        let mut a = Mat::identity(n);
        let mut rp = Mat::zeros(n, m);
        for (x, rows) in trans.iter().enumerate() {
            for r in rows {
                if r.absorbed {
                    *rp.at_mut(x, r.target) += r.q;
                } else {
                    *a.at_mut(x, r.target) -= r.q;
                }
            }
        }
        let lu = Lu::new(&a).ok_or_else(never)?;
        let xm = lu.solve(&rp); // absorption probabilities from every transient state
        for x in 0..n {
            let s: f64 = xm.row(x).iter().sum();
            if !s.is_finite() || s < 1.0 - 1e-6 {
                return Err(never());
            }
        }
        let mut e0 = vec![0.0; n];
        e0[0] = 1.0;
        let z = lu.solve_transpose(&e0);
        // B1c = A^-1 (RY1c + QY1c X)   (needed for every row, for the second moment)
        let mut rhs = Mat::zeros(n, m);
        for (x, rows) in trans.iter().enumerate() {
            for r in rows {
                if r.y1c == 0.0 {
                    continue;
                }
                if r.absorbed {
                    *rhs.at_mut(x, r.target) += r.y1c;
                } else {
                    for u in 0..m {
                        *rhs.at_mut(x, u) += r.y1c * xm.at(r.target, u);
                    }
                }
            }
        }
        let b1c = lu.solve(&rhs);
        // start-row quantities via z: B1[j][0,:], B2[0,:]
        let mut b1 = vec![vec![0.0; m]; ncur];
        let mut b2 = vec![0.0; m];
        for (x, rows) in trans.iter().enumerate() {
            let w = z[x];
            if w == 0.0 {
                continue;
            }
            for r in rows {
                if r.absorbed {
                    for (j, v) in r.y1.iter().enumerate() {
                        b1[j][r.target] += w * v;
                    }
                    b2[r.target] += w * r.y2;
                } else {
                    let xt = xm.row(r.target);
                    let bt = b1c.row(r.target);
                    for (j, v) in r.y1.iter().enumerate() {
                        if *v != 0.0 {
                            for u in 0..m {
                                b1[j][u] += w * v * xt[u];
                            }
                        }
                    }
                    for u in 0..m {
                        b2[u] += w * (r.y2 * xt[u] + 2.0 * r.y1c * bt[u]);
                    }
                }
            }
        }
        let mut out = Dist::default();
        for (u, st) in absorbed_list.into_iter().enumerate() {
            let p = xm.at(0, u);
            if p <= 0.0 {
                continue;
            }
            let s1: Vec<f64> = (0..ncur).map(|j| b1[j][u]).collect();
            acc(&mut out, st, p, &s1, b2[u]);
        }
        Ok(out)
    }
}
