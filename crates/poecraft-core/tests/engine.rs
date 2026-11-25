//! Engine tests: exact maths, exact vs Monte Carlo, error messages, every shipped recipe runs.
use poecraft_core::analysis::{analyse, check_agreement, exact_stats, mc_stats, Mode, Options};
use poecraft_core::data::default_data;
use poecraft_core::engine::Engine;
use poecraft_core::exact::Budget;
use poecraft_core::lang::{parse, parse_file};
use poecraft_core::prices::Prices;
use poecraft_core::rules::RuleUse;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn prices() -> Prices {
    Prices::load(&root().join("data"), &[], false).unwrap()
}
fn budget() -> Budget {
    Budget {
        paths: 1_000_000,
        states: 20_000,
        deadline: Instant::now() + Duration::from_secs(60),
    }
}

const SMALL: &str = r#"
repeat {
    buy "Astral Plate" ilvl 86 rare influence hunter
        with "Hunter: attack crit" tier 1 with "Hunter: max life %" tier 1 for 50 chaos
    dominance
} until has "Hunter: attack crit" elevated
goal has "Hunter: attack crit" elevated
"#;

#[test]
fn geometric_loop_exact() {
    let gd = default_data();
    let pr = prices();
    let prog = parse(SMALL, None).unwrap();
    let ex = exact_stats(gd, &pr, &prog, budget(), &mut RuleUse::default())
        .unwrap()
        .unwrap();
    let dom = pr.get("dominance");
    assert!((ex.mean_counts["dominance"] - 2.0).abs() < 1e-9);
    assert!((ex.mean_chaos - 2.0 * (dom + 50.0)).abs() < 1e-6);
    // Var of geometric(1/2) trial count * cost^2 = 2 cost^2
    assert!((ex.std_chaos - 2f64.sqrt() * (dom + 50.0)).abs() < 1e-6);
}

#[test]
fn exact_and_monte_carlo_agree_on_test_recipes() {
    let gd = default_data();
    let pr = prices();
    for f in [
        "dominance_2mods",
        "dominance_loop",
        "three_mods",
        "alt_spam",
    ] {
        let prog = parse_file(&root().join(format!("recipes/tests/{f}.craft"))).unwrap();
        let ex = exact_stats(gd, &pr, &prog, budget(), &mut RuleUse::default())
            .unwrap()
            .unwrap();
        let mc = mc_stats(gd, &pr, &prog, 4000, 7, &mut RuleUse::default()).unwrap();
        let (ok, msg) = check_agreement(&ex, &mc, 4.0);
        assert!(ok, "{f}: {msg}");
    }
}

#[test]
fn chest_pieces_need_two_dominance_each() {
    let gd = default_data();
    let pr = prices();
    for f in ["chest_hunter_piece", "chest_warlord_piece"] {
        let prog = parse_file(&root().join(format!("recipes/{f}.craft"))).unwrap();
        let rep = analyse(
            gd,
            &pr,
            &prog,
            &Options {
                runs: 2000,
                mode: Mode::Exact,
                seed: 0,
                exact_seconds: 60,
                progress: None,
            },
        )
        .unwrap();
        let dom = rep.mean_counts["dominance"];
        assert!((dom - 2.0).abs() < 0.05, "{f}: {dom} Dominance");
    }
}

fn err(src: &str) -> String {
    let gd = default_data();
    let pr = prices();
    match parse(src, None) {
        Err(e) => e.to_string(),
        Ok(prog) => Engine::new(gd, &pr, Engine::switches_for(&prog).unwrap())
            .run_mc(&prog, 0, 200_000, None)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default(),
    }
}

#[test]
fn error_messages() {
    assert!(err("buy \"Astral Plate\"\ndominanse").contains("Did you mean: dominance"));
    assert!(err("chaos").contains("start with 'buy'"));
    let m = err("buy \"Astral Plate\" ilvl 86\nregal");
    assert!(
        m.contains(":2:") && m.contains("Regal Orb needs a magic item"),
        "{m}"
    );
    assert!(
        err("buy \"Astral Plate\" rare with \"Hunter: additional curse\"")
            .contains("add \"influence hunter\"")
    );
    assert!(err(
        "buy \"Astral Plate\" ilvl 86 magic\nrepeat { alt } until has \"Hunter: additional curse\""
    )
    .contains("can never finish"));
    assert!(
        err("buy \"Astral Plate\" ilvl 86 rare\nharvest reforge cast")
            .contains("Unknown Harvest reforge tag")
    );
}

#[test]
fn every_shipped_recipe_runs() {
    let gd = default_data();
    let pr = prices();
    let mut n = 0;
    for entry in walk(&root().join("recipes")) {
        let is_lib = entry.components().any(|c| c.as_os_str() == "lib");
        let prog = parse_file(&entry).unwrap_or_else(|e| panic!("{}: {e}", entry.display()));
        if is_lib {
            continue;
        }
        let eng = Engine::new(gd, &pr, Engine::switches_for(&prog).unwrap());
        for s in 0..5 {
            eng.run_mc(&prog, s, 200_000, None)
                .unwrap_or_else(|e| panic!("{}: {e}", entry.display()));
        }
        n += 1;
    }
    assert!(n >= 15, "only {n} recipes found");
}

fn walk(d: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(d).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else if p.extension().is_some_and(|x| x == "craft") {
            out.push(p);
        }
    }
    out.sort();
    out
}
