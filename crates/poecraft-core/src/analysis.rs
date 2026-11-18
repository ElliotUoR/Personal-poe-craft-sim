//! Statistics over recipe runs, exact/MC cross-check and recipe comparison
//! (port of `poecraft/analysis.py`). Monte Carlo runs on every CPU core; run i always uses
//! seed `seed * 1_000_003 + i`, so results do not depend on the number of threads.

use crate::data::GameData;
use crate::engine::{Engine, LOST};
use crate::exact::{Budget, Exact, ExactErr};
use crate::lang::{Program, RecipeError};
use crate::prices::Prices;
use crate::rules::{RuleInfo, Status};
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Auto,
    Exact,
    Mc,
    Both,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        Some(match s {
            "auto" => Mode::Auto,
            "exact" => Mode::Exact,
            "mc" => Mode::Mc,
            "both" => Mode::Both,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Stats {
    pub p_success: f64,
    pub mean_chaos: f64,
    pub std_chaos: f64,
    pub se_mean: f64,
    pub mean_counts: BTreeMap<String, f64>,
    pub mean_lost: f64,
    pub tracks: BTreeMap<String, f64>,
    pub revenue: f64,
    // Monte Carlo only
    pub runs: usize,
    pub percentiles: BTreeMap<u32, f64>,
    pub p_lose_base: Option<f64>,
    pub profit: Option<ProfitStats>,
    pub example: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfitStats {
    pub p_loss: f64,
    pub p10: f64,
    pub p50: f64,
    pub p90: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub name: String,
    pub method: String,
    pub runs: usize,
    pub p_success: f64,
    pub mean_chaos: f64,
    pub std_chaos: f64,
    pub cost_per_success: f64,
    pub mean_counts: BTreeMap<String, f64>,
    pub divine: f64,
    pub percentiles: BTreeMap<u32, f64>,
    pub mean_lost: f64,
    pub p_lose_base: Option<f64>,
    pub agreement: Option<String>,
    pub missing_prices: Vec<String>,
    pub example_item: Option<String>,
    pub tracks: BTreeMap<String, f64>,
    pub revenue: Option<f64>,
    pub profit: Option<ProfitStats>,
    pub exact: Option<Stats>,
    pub mc: Option<Stats>,
    /// Unverified or disputed rules the result depends on.
    pub uncertain_rules: Vec<RuleInfo>,
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    sorted[((q / 100.0 * sorted.len() as f64) as usize).min(sorted.len() - 1)]
}

struct RunRec {
    i: usize,
    total: f64,
    success: bool,
    lost: f64,
    revenue: f64,
    counts: Vec<(String, f64)>,
    tracks: Vec<bool>,
    example: Option<String>,
}

pub fn mc_stats(gd: &GameData, prices: &Prices, prog: &Program, runs: usize, seed: u64, used: &mut crate::rules::RuleUse) -> Result<Stats, RecipeError> {
    mc_stats_p(gd, prices, prog, runs, seed, used, None)
}

pub fn mc_stats_p(gd: &GameData, prices: &Prices, prog: &Program, runs: usize, seed: u64, used: &mut crate::rules::RuleUse, progress: Option<&Progress>) -> Result<Stats, RecipeError> {
    if let Some(p) = progress {
        p.start("monte carlo", runs);
    }
    let sw = Engine::switches_for(prog)?;
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(runs.max(1));
    let results: Vec<Result<(Vec<RunRec>, crate::rules::RuleUse), RecipeError>> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..threads)
            .map(|t| {
                sc.spawn(move || {
                    let eng = Engine::new(gd, prices, sw);
                    let mut out = vec![];
                    let mut i = t;
                    let mut have_example = false;
                    while i < runs {
                        let r = eng.run_mc(prog, seed.wrapping_mul(1_000_003).wrapping_add(i as u64), 200_000, None)?;
                        if let Some(p) = progress {
                            p.tick();
                        }
                        let total: f64 = r.costs.iter().map(|(k, v)| prices.get(k) * v).sum();
                        let lost = r.costs.iter().find(|x| x.0 == LOST).map(|x| x.1).unwrap_or(0.0);
                        let example = if r.success && !have_example {
                            have_example = true;
                            r.item.as_ref().map(|it| it.describe(gd))
                        } else {
                            None
                        };
                        out.push(RunRec { i, total, success: r.success, lost, revenue: r.revenue, counts: r.costs, tracks: r.tracks.iter().map(|x| x.1).collect(), example });
                        i += threads;
                    }
                    let used = eng.mech.used.borrow().clone();
                    Ok((out, used))
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().expect("worker panicked")).collect()
    });
    let mut recs = vec![];
    for r in results {
        let (v, u) = r?;
        used.0.extend(u.0);
        recs.extend(v);
    }
    recs.sort_by_key(|r| r.i);
    let n = recs.len().max(1) as f64;
    let mut st = Stats { runs: recs.len(), ..Default::default() };
    let mut totals: Vec<f64> = recs.iter().map(|r| r.total).collect();
    st.p_success = recs.iter().filter(|r| r.success).count() as f64 / n;
    st.mean_chaos = totals.iter().sum::<f64>() / n;
    let var = totals.iter().map(|t| (t - st.mean_chaos).powi(2)).sum::<f64>() / n;
    st.std_chaos = var.sqrt();
    st.se_mean = st.std_chaos / n.sqrt();
    for r in &recs {
        for (k, v) in &r.counts {
            *st.mean_counts.entry(k.clone()).or_insert(0.0) += v / n;
        }
    }
    st.mean_lost = recs.iter().map(|r| r.lost).sum::<f64>() / n;
    st.p_lose_base = Some(recs.iter().filter(|r| r.lost > 0.0).count() as f64 / n);
    for (j, (lbl, _)) in prog.tracks.iter().enumerate() {
        st.tracks.insert(lbl.clone(), recs.iter().filter(|r| r.tracks[j]).count() as f64 / n);
    }
    st.revenue = recs.iter().map(|r| r.revenue).sum::<f64>() / n;
    totals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if !totals.is_empty() {
        for q in [50, 90, 99] {
            st.percentiles.insert(q, quantile(&totals, q as f64));
        }
    }
    if !prog.sales.is_empty() && !recs.is_empty() {
        let mut prof: Vec<f64> = recs.iter().map(|r| r.revenue - r.total).collect();
        prof.sort_by(|a, b| a.partial_cmp(b).unwrap());
        st.profit = Some(ProfitStats { p_loss: prof.iter().filter(|p| **p < 0.0).count() as f64 / n, p10: quantile(&prof, 10.0), p50: quantile(&prof, 50.0), p90: quantile(&prof, 90.0) });
    }
    st.example = recs.iter().find_map(|r| r.example.clone());
    Ok(st)
}

pub fn exact_stats(gd: &GameData, prices: &Prices, prog: &Program, budget: Budget, used: &mut crate::rules::RuleUse) -> Result<Result<Stats, String>, RecipeError> {
    let sw = Engine::switches_for(prog)?;
    let eng = Engine::new(gd, prices, sw);
    let res = match Exact::new(&eng, budget).run(prog) {
        Ok(r) => r,
        Err(ExactErr::TooLarge(m)) => return Ok(Err(m)),
        Err(ExactErr::Recipe(e)) => return Err(e),
    };
    used.0.extend(eng.mech.used.borrow().0.iter().copied());
    let mut var = (res.second_moment - res.mean_chaos * res.mean_chaos).max(0.0);
    if var < 1e-9 * res.mean_chaos * res.mean_chaos {
        var = 0.0; // rounding left over from E[X^2] - E[X]^2
    }
    let mean_lost = res.mean_counts.iter().find(|x| x.0 == LOST).map(|x| x.1).unwrap_or(0.0);
    Ok(Ok(Stats {
        p_success: res.p_success,
        mean_chaos: res.mean_chaos,
        std_chaos: var.sqrt(),
        mean_counts: res.mean_counts.into_iter().collect(),
        mean_lost,
        tracks: res.tracks.into_iter().collect(),
        revenue: res.revenue,
        ..Default::default()
    }))
}

/// Exact mean must lie within z standard errors of the MC mean, and success probabilities
/// within z binomial standard errors.
pub fn check_agreement(ex: &Stats, mc: &Stats, z: f64) -> (bool, String) {
    let n = mc.runs as f64;
    let zc = (ex.mean_chaos - mc.mean_chaos).abs() / mc.se_mean.max(1e-9);
    let p = ex.p_success;
    let sep = ((p * (1.0 - p)).max(1e-12) / n).sqrt();
    let zp = (p - mc.p_success).abs() / sep;
    let ok = zc <= z && zp <= z;
    (
        ok,
        format!(
            "{} (mean: exact {:.1} vs mc {:.1}, z={:.1}; success: exact {:.3} vs mc {:.3}, z={:.1})",
            if ok { "AGREE" } else { "DISAGREE" },
            ex.mean_chaos,
            mc.mean_chaos,
            zc,
            p,
            mc.p_success,
            zp
        ),
    )
}

pub struct Options {
    pub runs: usize,
    pub mode: Mode,
    pub seed: u64,
    pub exact_seconds: u64,
    /// Live progress for a UI (CLI bar now, web app later). None = no reporting.
    pub progress: Option<std::sync::Arc<Progress>>,
}

impl Default for Options {
    fn default() -> Self {
        Options { runs: 2000, mode: Mode::Auto, seed: 0, exact_seconds: 10, progress: None }
    }
}

/// Shared progress state, updated by the workers and read by whoever draws the bar.
#[derive(Debug, Default)]
pub struct Progress {
    pub done: std::sync::atomic::AtomicUsize,
    pub total: std::sync::atomic::AtomicUsize,
    /// Current phase, e.g. "exact solve" or "monte carlo".
    pub phase: std::sync::Mutex<String>,
    /// Which recipe (for compare).
    pub label: std::sync::Mutex<String>,
}

impl Progress {
    pub fn start(&self, phase: &str, total: usize) {
        use std::sync::atomic::Ordering::Relaxed;
        *self.phase.lock().unwrap() = phase.to_string();
        self.total.store(total, Relaxed);
        self.done.store(0, Relaxed);
    }
    pub fn tick(&self) {
        self.done.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

pub fn analyse(gd: &GameData, prices: &Prices, prog: &Program, opt: &Options) -> Result<Report, RecipeError> {
    prices.missing.lock().unwrap().clear();
    let mut used = crate::rules::RuleUse::default();
    let mut ex = None;
    let mut note = None;
    if matches!(opt.mode, Mode::Auto | Mode::Exact | Mode::Both) {
        let budget = Budget { paths: 300_000, states: 20_000, deadline: Instant::now() + Duration::from_secs(opt.exact_seconds) };
        if let Some(p) = &opt.progress {
            p.start("exact solve", 0);
        }
        match exact_stats(gd, prices, prog, budget, &mut used)? {
            Ok(s) => ex = Some(s),
            Err(m) => {
                if opt.mode == Mode::Exact {
                    return Err(RecipeError::new(format!("exact engine: {m}"), None, None));
                }
                note = Some(format!("exact engine skipped ({m}); using Monte Carlo"));
            }
        }
    }
    let mc = if matches!(opt.mode, Mode::Mc | Mode::Both | Mode::Auto) || ex.is_none() { Some(mc_stats_p(gd, prices, prog, opt.runs, opt.seed, &mut used, opt.progress.as_deref())?) } else { None };
    let agreement = match (&ex, &mc) {
        (Some(e), Some(m)) => Some(check_agreement(e, m, 4.0).1),
        _ => note,
    };
    let main = ex.clone().or_else(|| mc.clone()).unwrap();
    let method = match (&ex, &mc) {
        (Some(_), Some(_)) => "exact+mc",
        (Some(_), None) => "exact",
        _ => "monte carlo",
    };
    let cps = if main.p_success > 0.0 { main.mean_chaos / main.p_success } else { f64::INFINITY };
    let mut missing: Vec<String> = prices.missing.lock().unwrap().iter().cloned().collect();
    missing.sort();
    Ok(Report {
        name: prog.name.clone(),
        method: method.into(),
        runs: mc.as_ref().map(|m| m.runs).unwrap_or(0),
        p_success: main.p_success,
        mean_chaos: main.mean_chaos,
        std_chaos: main.std_chaos,
        cost_per_success: cps,
        mean_counts: main.mean_counts.clone(),
        divine: prices.divine(),
        percentiles: mc.as_ref().map(|m| m.percentiles.clone()).unwrap_or_default(),
        mean_lost: main.mean_lost,
        p_lose_base: mc.as_ref().and_then(|m| m.p_lose_base),
        agreement,
        missing_prices: missing,
        example_item: mc.as_ref().and_then(|m| m.example.clone()),
        tracks: main.tracks.clone(),
        revenue: if prog.sales.is_empty() { None } else { Some(main.revenue) },
        profit: mc.as_ref().and_then(|m| m.profit.clone()),
        uncertain_rules: used.uncertain().into_iter().cloned().collect(),
        exact: ex,
        mc,
    })
}

impl Report {
    pub fn text(&self, prices: &Prices) -> String {
        let dv = self.divine.max(1e-9);
        let mut l = vec![
            format!("== {}  [{}{}]", self.name, self.method, if self.runs > 0 { format!(", {} runs", self.runs) } else { String::new() }),
            format!("  success probability : {:.2}%", self.p_success * 100.0),
            format!("  expected cost       : {} chaos  ({:.2} divine)", fmt0(self.mean_chaos), self.mean_chaos / dv),
            format!("  std dev             : {} chaos  ({:.2} divine)", fmt0(self.std_chaos), self.std_chaos / dv),
            format!("  cost per success    : {} chaos  ({:.2} divine)", fmt0(self.cost_per_success), self.cost_per_success / dv),
        ];
        if !self.percentiles.is_empty() {
            l.push(format!("  cost percentiles    : {}", self.percentiles.iter().map(|(k, v)| format!("p{k} {:.1}div", v / dv)).collect::<Vec<_>>().join(", ")));
        }
        if self.mean_lost > 0.0 {
            let mut s = format!("  bases lost          : {} chaos expected", fmt0(self.mean_lost));
            if let Some(p) = self.p_lose_base {
                s += &format!("; P(lose at least one bought base) = {:.1}%", p * 100.0);
            }
            l.push(s);
        }
        l.push("  expected currency used:".into());
        let mut cs: Vec<(&String, &f64)> = self.mean_counts.iter().filter(|(k, v)| !k.starts_with('_') && **v > 1e-9).collect();
        cs.sort_by(|a, b| (b.1 * prices.get(b.0)).partial_cmp(&(a.1 * prices.get(a.0))).unwrap());
        for (k, v) in cs {
            l.push(format!("    {v:10.3} x {k}  = {}c", fmt0(v * prices.get(k))));
        }
        if let Some(rev) = self.revenue {
            let prof = rev - self.mean_chaos;
            l.push(format!("  expected sale value : {} chaos  ({:.2} divine)", fmt0(rev), rev / dv));
            if rev > 0.0 && self.mean_chaos > 0.0 {
                l.push(format!(
                    "  expected profit     : {} chaos  ({:+.2} divine) per attempt; margin {:+.1}% on cost, {:+.1}% of sales",
                    fmt0(prof),
                    prof / dv,
                    prof / self.mean_chaos * 100.0,
                    prof / rev * 100.0
                ));
            }
            if let Some(p) = &self.profit {
                l.push(format!("  profit distribution : P(loss) {:.1}%; p10 {:+.1}div, p50 {:+.1}div, p90 {:+.1}div", p.p_loss * 100.0, p.p10 / dv, p.p50 / dv, p.p90 / dv));
            }
        }
        for (k, p) in &self.tracks {
            l.push(format!("  P({k}) = {:.2}%", p * 100.0));
        }
        if let Some(a) = &self.agreement {
            l.push(format!("  exact vs monte carlo: {a}"));
        }
        if !self.missing_prices.is_empty() {
            l.push(format!("  WARNING: no price for {} (counted as 0)", self.missing_prices.join(", ")));
        }
        if !self.uncertain_rules.is_empty() {
            l.push("  depends on unverified/disputed rules:".into());
            for r in &self.uncertain_rules {
                let tag = if r.status == Status::Disputed { "DISPUTED" } else { "unverified" };
                l.push(format!("    - [{tag}] {}{}", r.id, r.question.map(|q| format!(" (spec {q})")).unwrap_or_default()));
            }
        }
        l.join("\n")
    }
}

pub fn fmt0(v: f64) -> String {
    if !v.is_finite() {
        return "inf".into();
    }
    let s = format!("{:.0}", v.abs());
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if v < -0.5 {
        format!("-{out}")
    } else {
        out
    }
}

pub fn compare_table(reps: &[Report]) -> String {
    let dv = reps.first().map(|r| r.divine).unwrap_or(1.0).max(1e-9);
    let hdr = format!("{:>2} {:28} {:>8} {:>9} {:>12} {:>8} {:>8} {:>12} {:>12}", "#", "recipe", "success", "E[cost]", "per success", "std", "p90", "E[lost base]", "P(lose base)");
    let mut l = vec![hdr.clone(), "-".repeat(hdr.len())];
    for (i, r) in reps.iter().enumerate() {
        let p90 = r.percentiles.get(&90).map(|v| format!("{:.1}", v / dv)).unwrap_or("-".into());
        let pl = r.p_lose_base.map(|v| format!("{:.1}%", v * 100.0)).unwrap_or("-".into());
        let name: String = r.name.chars().take(28).collect();
        l.push(format!(
            "{:>2} {:28} {:>7.1}% {:>8.2}d {:>11.2}d {:>7.2}d {:>7}d {:>11.2}d {:>12}",
            i + 1,
            name,
            r.p_success * 100.0,
            r.mean_chaos / dv,
            r.cost_per_success / dv,
            r.std_chaos / dv,
            p90,
            r.mean_lost / dv,
            pl
        ));
    }
    l.push("(d = divine orbs; 'per success' = expected cost / success probability, i.e. the cost if you restart the whole recipe after a failure)".into());
    l.join("\n")
}
