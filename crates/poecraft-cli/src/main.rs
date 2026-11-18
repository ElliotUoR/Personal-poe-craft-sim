//! poecraft command line (Rust). Run `poecraft help`.
use anyhow::{anyhow, Result};
use poecraft_core::data::{default_data, Influence, Influences};

fn usage() -> &'static str {
    "poecraft (rust)\n\
     commands:\n\
     \x20 pool <base> [--influence a,b] [--ilvl N]     mod pool as JSON lines: id, weight, tier\n\
     \x20 resolve <mod reference> [--class C]          resolve a readable mod name to a family\n"
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        print!("{}", usage());
        return Ok(());
    };
    let gd = default_data();
    match cmd.as_str() {
        "pool" => {
            let base = args.get(1).ok_or_else(|| anyhow!("pool needs a base name"))?;
            let b = gd.resolve_base(base)?;
            let mut infl = Influences::default();
            if let Some(s) = arg_value(&args, "--influence") {
                for x in s.split(',').filter(|x| !x.is_empty()) {
                    infl = infl.with(Influence::parse(x).ok_or_else(|| anyhow!("unknown influence {x}"))?);
                }
            }
            let ilvl: u32 = arg_value(&args, "--ilvl").map(|s| s.parse()).transpose()?.unwrap_or(86);
            for &(mi, w) in gd.spawn_pool(b, infl, ilvl).iter() {
                println!("{}\t{}\t{}", gd.m(mi).id, w, gd.tier(mi, b));
            }
        }
        "resolve" => {
            let r = args.get(1).ok_or_else(|| anyhow!("resolve needs a mod reference"))?;
            let cls = arg_value(&args, "--class");
            match gd.resolve_mod(r, cls.as_deref()) {
                Ok(fi) => println!("{}", gd.family_label(fi)),
                Err(e) => println!("ERROR {e}"),
            }
        }
        "help" | "--help" | "-h" => print!("{}", usage()),
        other => return Err(anyhow!("unknown command {other}\n{}", usage())),
    }
    Ok(())
}
