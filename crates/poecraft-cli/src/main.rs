//! poecraft command line (Rust). Run `poecraft help`.
use anyhow::{anyhow, bail, Result};
use poecraft_core::analysis::{analyse, compare_table, Mode, Options, Report};
use poecraft_core::data::{default_data, title, Gen, Influence, Influences};
use poecraft_core::engine::Engine;
use poecraft_core::lang::parse_file;
use poecraft_core::prices::Prices;
use poecraft_core::rules::RULES;
use std::path::Path;

const USAGE: &str = "poecraft (Rust) - Path of Exile 1 crafting simulator

usage: poecraft [--prices FILE]... <command> [options]

commands:
  run RECIPE [--runs N] [--mode auto|exact|mc|both] [--seed S] [--json] [--show-item]
  compare RECIPE... [--runs N] [--mode M] [--sort cost_per_success|mean|risk|std] [--json] [-v]
  sample RECIPE [--seed S]                run once; print the item, cost and `show` output
  check RECIPE [--runs N]                 parse and run a few times to catch errors
  mods BASE [--influence a,b] [--ilvl N] [--side prefix|suffix] [--search TEXT] [--tag TAG]
  prices [--grep TEXT]                    show the price table in use
  rules                                   list every crafting rule with source and status
  pool BASE [--influence a,b] [--ilvl N]  raw pool (id, weight, tier) for parity checks
  resolve REF [--class CLASS]             resolve a readable mod name
";

struct Args {
    v: Vec<String>,
}
impl Args {
    fn flag(&self, f: &str) -> bool {
        self.v.iter().any(|a| a == f)
    }
    fn val(&self, f: &str) -> Option<String> {
        self.v.iter().position(|a| a == f).and_then(|i| self.v.get(i + 1).cloned())
    }
    /// Positional arguments after the command (skipping flags and their values).
    fn positional(&self) -> Vec<String> {
        const WITH_VALUE: [&str; 13] = ["--runs", "--mode", "--seed", "--sort", "--prices", "--influence", "--ilvl", "--side", "--search", "--tag", "--grep", "--class", "--exact-seconds"];
        let mut out = vec![];
        let mut i = 1;
        while i < self.v.len() {
            let a = &self.v[i];
            if WITH_VALUE.contains(&a.as_str()) {
                i += 2;
                continue;
            }
            if !a.starts_with('-') {
                out.push(a.clone());
            }
            i += 1;
        }
        out
    }
}

/// Progress bar on stderr (only when stderr is a terminal and --json/--quiet are not given).
struct ProgressBar {
    state: std::sync::Arc<poecraft_core::analysis::Progress>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl ProgressBar {
    fn start(args: &Args, label: &str) -> Option<ProgressBar> {
        use std::io::IsTerminal;
        if args.flag("--json") || args.flag("--quiet") || (!std::io::stderr().is_terminal() && !args.flag("--progress")) {
            return None;
        }
        let state = std::sync::Arc::new(poecraft_core::analysis::Progress::default());
        *state.label.lock().unwrap() = label.to_string();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (s2, st2) = (state.clone(), stop.clone());
        let handle = std::thread::spawn(move || {
            use std::io::Write;
            use std::sync::atomic::Ordering::Relaxed;
            let t0 = std::time::Instant::now();
            let spin = ['|', '/', '-', '\\'];
            let mut k = 0;
            let (mut last_phase, mut tp) = (String::new(), std::time::Instant::now());
            while !st2.load(Relaxed) {
                let (done, total) = (s2.done.load(Relaxed), s2.total.load(Relaxed));
                let phase = s2.phase.lock().unwrap().clone();
                if phase != last_phase {
                    last_phase = phase.clone();
                    tp = std::time::Instant::now(); // ETA per phase
                }
                let label = s2.label.lock().unwrap().clone();
                let el = t0.elapsed().as_secs_f64();
                let pel = tp.elapsed().as_secs_f64();
                let line = if total > 0 {
                    let f = done as f64 / total as f64;
                    let w = 30;
                    let fill = (f * w as f64) as usize;
                    let eta = if done > 0 { pel / f - pel } else { 0.0 };
                    format!("{label} {phase} [{}{}] {:5.1}% {done}/{total}  {el:.1}s  eta {eta:.1}s", "#".repeat(fill), "-".repeat(w - fill), f * 100.0)
                } else {
                    format!("{label} {} {phase}...  {el:.1}s", spin[k % 4])
                };
                eprint!("\r{line:<100}");
                let _ = std::io::stderr().flush();
                k += 1;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            eprint!("\r{:<100}\r", "");
        });
        Some(ProgressBar { state, stop, handle: Some(handle) })
    }
}

impl Drop for ProgressBar {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn influences(s: Option<String>) -> Result<Influences> {
    let mut infl = Influences::default();
    if let Some(s) = s {
        for x in s.split(',').filter(|x| !x.is_empty()) {
            infl = infl.with(Influence::parse(x).ok_or_else(|| anyhow!("unknown influence {x}"))?);
        }
    }
    Ok(infl)
}

fn main() {
    if let Err(e) = real_main() {
        eprintln!("error: {e}");
        std::process::exit(2);
    }
}

fn real_main() -> Result<()> {
    // global --prices FILE options may come before the command: pull them out first
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut v = vec![];
    let mut price_files = vec![];
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == "--prices" && i + 1 < raw.len() {
            price_files.push(raw[i + 1].clone());
            i += 2;
        } else {
            v.push(raw[i].clone());
            i += 1;
        }
    }
    let args = Args { v };
    let Some(cmd) = args.v.first().cloned() else {
        print!("{USAGE}");
        return Ok(());
    };
    let gd = default_data();
    let pf: Vec<&Path> = price_files.iter().map(Path::new).collect();
    let prices = Prices::load(&gd.data_dir, &pf, true).map_err(|e| anyhow!(e))?;
    let opts = |a: &Args| -> Result<Options> {
        Ok(Options {
            runs: a.val("--runs").map(|s| s.parse()).transpose()?.unwrap_or(2000),
            mode: a.val("--mode").map(|s| Mode::parse(&s).ok_or_else(|| anyhow!("bad --mode {s}"))).transpose()?.unwrap_or(Mode::Auto),
            seed: a.val("--seed").map(|s| s.parse()).transpose()?.unwrap_or(0),
            exact_seconds: a.val("--exact-seconds").map(|s| s.parse()).transpose()?.unwrap_or(10),
            progress: None,
        })
    };
    match cmd.as_str() {
        "run" => {
            let pos = args.positional();
            let path = pos.first().ok_or_else(|| anyhow!("run needs a recipe file"))?;
            let prog = parse_file(Path::new(path))?;
            let mut o = opts(&args)?;
            let bar = ProgressBar::start(&args, &prog.name);
            o.progress = bar.as_ref().map(|b| b.state.clone());
            let rep = analyse(gd, &prices, &prog, &o);
            drop(bar);
            let rep = rep?;
            if args.flag("--json") {
                println!("{}", serde_json::to_string_pretty(&rep)?);
            } else {
                println!("{}", rep.text(&prices));
                if args.flag("--show-item") {
                    if let Some(it) = &rep.example_item {
                        println!("  example finished item:\n    {}", it.replace('\n', "\n    "));
                    }
                }
            }
        }
        "compare" => {
            let mut o = opts(&args)?;
            let files = args.positional();
            let mut reps: Vec<Report> = vec![];
            for (k, p) in files.iter().enumerate() {
                let prog = parse_file(Path::new(p))?;
                let bar = ProgressBar::start(&args, &format!("[{}/{}] {}", k + 1, files.len(), prog.name));
                o.progress = bar.as_ref().map(|b| b.state.clone());
                let r = analyse(gd, &prices, &prog, &o);
                drop(bar);
                reps.push(r?);
            }
            match args.val("--sort").as_deref().unwrap_or("cost_per_success") {
                "cost_per_success" => reps.sort_by(|a, b| a.cost_per_success.partial_cmp(&b.cost_per_success).unwrap()),
                "mean" => reps.sort_by(|a, b| a.mean_chaos.partial_cmp(&b.mean_chaos).unwrap()),
                "risk" => reps.sort_by(|a, b| ((1.0 - a.p_success), a.mean_lost).partial_cmp(&((1.0 - b.p_success), b.mean_lost)).unwrap()),
                "std" => reps.sort_by(|a, b| a.std_chaos.partial_cmp(&b.std_chaos).unwrap()),
                s => bail!("bad --sort {s}"),
            }
            if args.flag("--json") {
                println!("{}", serde_json::to_string_pretty(&reps)?);
            } else {
                println!("{}", compare_table(&reps));
                if args.flag("-v") || args.flag("--verbose") {
                    for r in &reps {
                        println!("\n{}", r.text(&prices));
                    }
                }
            }
        }
        "sample" | "check" => {
            let pos = args.positional();
            let path = pos.first().ok_or_else(|| anyhow!("{cmd} needs a recipe file"))?;
            let prog = parse_file(Path::new(path))?;
            let eng = Engine::new(gd, &prices, Engine::switches_for(&prog)?);
            if cmd == "check" {
                let n: u64 = args.val("--runs").map(|s| s.parse()).transpose()?.unwrap_or(20);
                for s in 0..n {
                    eng.run_mc(&prog, s, 200_000, None)?;
                }
                println!("OK: {path} parsed and ran {n} time(s) without errors");
            } else {
                let seed = args.val("--seed").map(|s| s.parse()).transpose()?.unwrap_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64);
                let mut trace = vec![];
                let r = eng.run_mc(&prog, seed, 200_000, Some(&mut trace))?;
                for t in trace {
                    println!("-- {t}");
                }
                println!("== result: {}", if r.success { "SUCCESS".to_string() } else { format!("FAILED {}", r.msg) });
                if let Some(it) = &r.item {
                    println!("{}", it.describe(gd));
                }
                let total: f64 = r.costs.iter().map(|(k, v)| prices.get(k) * v).sum();
                println!("cost: {} chaos {:?}", poecraft_core::analysis::fmt0(total), r.costs);
            }
        }
        "mods" | "pool" => {
            let pos = args.positional();
            let base = pos.first().ok_or_else(|| anyhow!("{cmd} needs a base name"))?;
            let b = gd.resolve_base(base)?;
            let infl = influences(args.val("--influence"))?;
            let ilvl: u32 = args.val("--ilvl").map(|s| s.parse()).transpose()?.unwrap_or(86);
            let pool = gd.spawn_pool(b, infl, ilvl);
            if cmd == "pool" {
                for &(mi, w) in pool.iter() {
                    println!("{}\t{}\t{}", gd.m(mi).id, w, gd.tier(mi, b));
                }
                return Ok(());
            }
            let (mut tp, mut ts) = (0u64, 0u64);
            for &(mi, w) in pool.iter() {
                match gd.m(mi).gen {
                    Gen::Prefix => tp += w as u64,
                    Gen::Suffix => ts += w as u64,
                    _ => {}
                }
            }
            let side = args.val("--side");
            let search = args.val("--search").map(|s| s.to_lowercase());
            let tag = args.val("--tag");
            let mut names: Vec<&str> = infl.iter().map(|i| i.name()).collect();
            names.sort();
            println!("{} ilvl {ilvl} influences {}: total prefix weight {tp}, suffix weight {ts}", gd.base(b).name, if names.is_empty() { "none".into() } else { names.join(",") });
            let mut rows: Vec<_> = pool
                .iter()
                .filter(|(mi, _)| {
                    let m = gd.m(*mi);
                    side.as_deref().is_none_or(|s| m.gen.name() == s)
                        && search.as_deref().is_none_or(|s| m.text.to_lowercase().contains(s) || m.id.to_lowercase().contains(s))
                        && tag.as_deref().is_none_or(|t| m.has_tag(t))
                })
                .collect();
            rows.sort_by_key(|(mi, _)| {
                let m = gd.m(*mi);
                (m.gen, m.influence.map(|i| i.name()).unwrap_or(""), m.groups.first().cloned().unwrap_or_default(), std::cmp::Reverse(m.level))
            });
            for &&(mi, w) in &rows {
                let m = gd.m(mi);
                let tot = if m.gen == Gen::Prefix { tp } else { ts };
                println!(
                    "  {} {:8} T{:<2} w={:<5} {:5.2}%  lvl{:<3} {}  [{}] tags={}",
                    &m.gen.name()[..3],
                    m.influence.map(|i| i.name()).unwrap_or(""),
                    gd.tier(mi, b),
                    w,
                    100.0 * w as f64 / tot as f64,
                    m.level,
                    m.short().chars().take(70).collect::<String>(),
                    m.id,
                    m.tags.join(",")
                );
            }
        }
        "prices" => {
            println!("{}", serde_json::to_string_pretty(&prices.meta)?);
            let g = args.val("--grep").map(|s| s.to_lowercase());
            for (k, v) in &prices.table {
                if g.as_deref().is_none_or(|g| k.to_lowercase().contains(g)) {
                    println!("  {k:45} {v}");
                }
            }
        }
        "rules" => {
            for r in RULES {
                println!("{:28} {:15} {}{}\n{:44}source: {}", r.id, format!("{:?}", r.status), r.summary, r.question.map(|q| format!(" [spec {q}]")).unwrap_or_default(), "", r.source);
            }
        }
        "resolve" => {
            let pos = args.positional();
            let r = pos.first().ok_or_else(|| anyhow!("resolve needs a mod reference"))?;
            match gd.resolve_mod(r, args.val("--class").as_deref()) {
                Ok(fi) => println!("{}", gd.family_label(fi)),
                Err(e) => println!("ERROR {e}"),
            }
        }
        "help" | "--help" | "-h" => print!("{USAGE}"),
        other => bail!("unknown command {other}\n{USAGE}"),
    }
    let _ = title;
    Ok(())
}
