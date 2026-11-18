//! Item text in and out (spec §4.A/§4.F): Path of Building item text for "Copy item" /
//! "Import into PoB", and a parser for PoB text or in-game clipboard text ("Paste item").
//!
//! PoB format (what PoB's own `Item:BuildRaw` writes, see the harness output):
//! ```text
//! Rarity: RARE
//! <name>
//! <base>
//! Hunter Item
//! Item Level: 86
//! Quality: 20
//! Sockets: R-R-R-R-R-R
//! Implicits: 1
//! +12% to all Elemental Resistances
//! {crafted}73% increased Armour
//! ```

use crate::data::{title, BaseIdx, GameData, Gen, Influence, Influences, ModIdx};
use crate::item::{Item, Rarity, Roll};
use std::collections::BTreeSet;

// ------------------------------------------------------------------ templates
#[derive(Debug, Clone, PartialEq)]
enum Part {
    Lit(String),
    Range(f64, f64, usize), // lo, hi, decimals
}

/// Remove RePoE link markup: "[A|B]" -> "B", "[A]" -> "A".
pub fn clean(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find('[') {
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find(']') else {
            out.push_str(&rest[i..]);
            return out;
        };
        let inner = &rest[i + 1..i + j];
        out.push_str(inner.rsplit('|').next().unwrap_or(inner));
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    out
}

fn decimals(s: &str) -> usize {
    s.split_once('.').map(|(_, d)| d.len()).unwrap_or(0)
}

/// Parse a number at the start of `s`: optional sign, digits, optional fraction.
fn number_prefix(s: &str) -> Option<(f64, usize)> {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }
    let ds = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == ds {
        return None;
    }
    if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    s[..i].parse().ok().map(|v| (v, i))
}

/// Split one template line into literal text and "(lo-hi)" ranges.
fn parse_template(line: &str) -> Vec<Part> {
    let mut parts = vec![];
    let mut lit = String::new();
    let mut rest = line;
    while !rest.is_empty() {
        if rest.starts_with('(') {
            if let Some((lo, n1)) = number_prefix(&rest[1..]) {
                let after = &rest[1 + n1..];
                if let Some(after) = after.strip_prefix('-') {
                    if let Some((hi, n2)) = number_prefix(after) {
                        if after[n2..].starts_with(')') {
                            let dec = decimals(&rest[1..1 + n1]).max(decimals(&after[..n2]));
                            if !lit.is_empty() {
                                parts.push(Part::Lit(std::mem::take(&mut lit)));
                            }
                            parts.push(Part::Range(lo, hi, dec));
                            rest = &after[n2 + 1..];
                            continue;
                        }
                    }
                }
            }
        }
        let c = rest.chars().next().unwrap();
        lit.push(c);
        rest = &rest[c.len_utf8()..];
    }
    if !lit.is_empty() {
        parts.push(Part::Lit(lit));
    }
    parts
}

/// The lines of a mod's text (cleaned).
pub fn mod_lines(gd: &GameData, m: ModIdx) -> Vec<String> {
    clean(&gd.m(m).text)
        .lines()
        .map(|s| s.to_string())
        .collect()
}

/// Render a template line with every range at fraction `q` of its span (0 = min, 1 = max).
pub fn render_line(line: &str, q: f64) -> String {
    let mut out = String::new();
    for p in parse_template(line) {
        match p {
            Part::Lit(s) => out.push_str(&s),
            Part::Range(lo, hi, dec) => {
                let v = lo + (hi - lo) * q.clamp(0.0, 1.0);
                let f = 10f64.powi(dec as i32);
                let v = (v * f).round() / f;
                out.push_str(&format!("{v:.dec$}"));
            }
        }
    }
    out
}

/// Match a text line against a template line; returns the values of the ranges, and whether
/// they all lie inside their ranges.
fn match_line(template: &[Part], text: &str) -> Option<(Vec<f64>, bool)> {
    let mut rest = text;
    let mut vals = vec![];
    let mut inside = true;
    for p in template {
        match p {
            Part::Lit(s) => rest = rest.strip_prefix(s.as_str())?,
            Part::Range(lo, hi, _) => {
                let (v, n) = number_prefix(rest)?;
                let (a, b) = if lo <= hi { (*lo, *hi) } else { (*hi, *lo) };
                inside &= v >= a - 1e-9 && v <= b + 1e-9;
                vals.push(v);
                rest = &rest[n..];
            }
        }
    }
    rest.is_empty().then_some((vals, inside))
}

// ------------------------------------------------------------------ export
#[derive(Debug, Clone)]
pub struct ExportOpts {
    pub name: String,
    pub quality: u32,
    /// e.g. "R-R-R-R-R-R"; empty = omitted (PoB uses the base default)
    pub sockets: String,
    /// value roll for every range (0 = minimum, 1 = maximum)
    pub roll: f64,
}

impl Default for ExportOpts {
    fn default() -> Self {
        ExportOpts {
            name: "Crafted Item".into(),
            quality: 20,
            sockets: String::new(),
            roll: 1.0,
        }
    }
}

const INFLUENCE_ORDER: [Influence; 6] = [
    Influence::Shaper,
    Influence::Elder,
    Influence::Crusader,
    Influence::Redeemer,
    Influence::Hunter,
    Influence::Warlord,
];

/// PoB item text for "Copy item" / "Import into PoB".
pub fn to_pob_text(gd: &GameData, item: &Item, o: &ExportOpts) -> String {
    let b = gd.base(item.base);
    let mut l = vec![format!("Rarity: {}", item.rarity.name().to_uppercase())];
    match item.rarity {
        Rarity::Rare => {
            l.push(o.name.clone());
            l.push(b.name.clone());
        }
        _ => l.push(b.name.clone()),
    }
    for inf in INFLUENCE_ORDER {
        if item.influences.contains(inf) {
            l.push(format!("{} Item", title(inf.name())));
        }
    }
    l.push(format!("Item Level: {}", item.ilvl));
    if o.quality > 0 {
        l.push(format!("Quality: {}", o.quality));
    }
    if !o.sockets.is_empty() {
        l.push(format!("Sockets: {}", o.sockets));
    } else if item.links >= 6 {
        l.push("Sockets: R-R-R-R-R-R".into());
    }
    let implicits: Vec<String> = b
        .implicits
        .iter()
        .filter_map(|id| gd.mod_idx(id))
        .flat_map(|m| mod_lines(gd, m))
        .map(|t| render_line(&t, o.roll))
        .collect();
    l.push(format!("Implicits: {}", implicits.len()));
    l.extend(implicits);
    // prefixes first, then suffixes (the game's order)
    for gen in [Gen::Prefix, Gen::Suffix] {
        for r in item.side(gd, gen) {
            let tag = if r.crafted {
                "{crafted}"
            } else if r.fractured {
                "{fractured}"
            } else {
                ""
            };
            for line in mod_lines(gd, r.m) {
                l.push(format!("{tag}{}", render_line(&line, o.roll)));
            }
        }
    }
    l.join("\n")
}

// ------------------------------------------------------------------ import
#[derive(Debug, Clone)]
pub struct Parsed {
    pub item: Item,
    pub name: String,
    pub quality: u32,
    pub sockets: String,
    pub implicit_lines: Vec<String>,
    /// value of every range per explicit mod, aligned with the order mods were matched
    pub values: Vec<(ModIdx, Vec<f64>)>,
    /// lines that could not be matched to any mod (reported to the user)
    pub unmatched: Vec<String>,
    /// mods whose side could not be told from the text (identical prefix/suffix wording)
    pub ambiguous: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError(pub String);
impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ParseError {}

const META_PREFIXES: &[&str] = &[
    "Item Class:",
    "Rarity:",
    "Item Level:",
    "Quality",
    "Sockets:",
    "LevelReq:",
    "Level:",
    "Requirements:",
    "Str:",
    "Dex:",
    "Int:",
    "Armour:",
    "Evasion",
    "Energy Shield:",
    "Ward:",
    "Chance to Block:",
    "Unique ID:",
    "ArmourBasePercentile:",
    "EvasionBasePercentile:",
    "EnergyShieldBasePercentile:",
    "WardBasePercentile:",
    "Implicits:",
    "Note:",
    "Corrupted",
    "Mirrored",
    "Split",
    "Unidentified",
    "Shaper Item",
    "Elder Item",
    "Crusader Item",
    "Redeemer Item",
    "Hunter Item",
    "Warlord Item",
    "Searing Exarch Item",
    "Eater of Worlds Item",
    "Prefix:",
    "Suffix:",
    "Crafted:",
    "Radius:",
    "Limited to:",
    "Physical Damage:",
    "Elemental Damage:",
    "Critical Strike Chance:",
    "Attacks per Second:",
    "Weapon Range:",
    "Stack Size:",
    "Catalyst:",
    "Talisman Tier:",
    "Experience:",
    "Requires",
    "Variant:",
    "Selected Variant:",
    "Has Alt Variant",
    "Item Level",
    "~",
    "Price:",
];

/// Parse PoB item text or in-game clipboard text into an item.
pub fn parse_text(gd: &GameData, text: &str) -> Result<Parsed, ParseError> {
    let raw: Vec<String> = text
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with("----"))
        .collect();
    let field = |key: &str| {
        raw.iter()
            .find_map(|l| l.strip_prefix(key).map(|v| v.trim().to_string()))
    };
    let rarity = match field("Rarity:").map(|r| r.to_lowercase()).as_deref() {
        Some("rare") => Rarity::Rare,
        Some("magic") => Rarity::Magic,
        Some("normal") => Rarity::Normal,
        Some(other) => {
            return Err(ParseError(format!(
                "unsupported rarity '{other}' (normal, magic or rare only)"
            )))
        }
        None => return Err(ParseError("no 'Rarity:' line".into())),
    };
    // base: an exact line match first, otherwise the longest base name contained in a line
    let mut base: Option<(BaseIdx, usize)> = None;
    for (i, l) in raw.iter().enumerate().take(6) {
        if let Ok(b) = gd.resolve_base(l) {
            base = Some((b, i));
            break;
        }
    }
    if base.is_none() {
        let mut best: Option<(usize, BaseIdx, usize)> = None;
        for (i, l) in raw.iter().enumerate().take(6) {
            for (bi, b) in gd.bases.iter().enumerate() {
                if l.contains(b.name.as_str()) && best.as_ref().is_none_or(|x| b.name.len() > x.0) {
                    best = Some((b.name.len(), bi as BaseIdx, i));
                }
            }
        }
        base = best.map(|(_, b, i)| (b, i));
    }
    let Some((base_idx, base_line)) = base else {
        return Err(ParseError(
            "could not find a known base type in the first lines".into(),
        ));
    };
    let base_idx = gd.resolve_base(&gd.base(base_idx).name).unwrap_or(base_idx);
    let rarity_line = raw
        .iter()
        .position(|l| l.starts_with("Rarity:"))
        .unwrap_or(0);
    let name = if rarity == Rarity::Rare && base_line > rarity_line + 1 {
        raw[base_line - 1].clone()
    } else {
        String::new()
    };
    let ilvl: u32 = field("Item Level:")
        .and_then(|v| v.split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(100);
    let quality: u32 = field("Quality:")
        .map(|v| v.chars().filter(|c| c.is_ascii_digit()).collect::<String>())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let sockets = field("Sockets:").unwrap_or_default();
    let mut infl = Influences::default();
    for inf in Influence::ALL {
        if raw
            .iter()
            .any(|l| l == &format!("{} Item", title(inf.name())))
        {
            infl = infl.with(inf);
        }
    }
    // implicit lines: PoB "Implicits: N" block, or game "(implicit)" suffix
    let mut implicit_lines = vec![];
    let mut mod_lines_in: Vec<(String, bool, bool)> = vec![]; // text, crafted, fractured
    let mut hints: Vec<Option<Gen>> = vec![]; // side from "{ Prefix Modifier ... }" headers
    let mut hint: Option<Gen> = None;
    let mut hint_crafted = false;
    let imp_start = raw.iter().position(|l| l.starts_with("Implicits:"));
    let n_imp: usize = imp_start
        .and_then(|i| raw[i].trim_start_matches("Implicits:").trim().parse().ok())
        .unwrap_or(0);
    let body_start = imp_start.map(|i| i + 1).unwrap_or(base_line + 1);
    for (k, l) in raw.iter().enumerate().skip(body_start) {
        if imp_start.is_some() && k < body_start + n_imp {
            implicit_lines.push(l.clone());
            continue;
        }
        if l.starts_with("{ ") || l.starts_with("{Prefix") || l.starts_with("{Suffix") {
            // in-game advanced copy (Ctrl+Alt+C): "{ Prefix Modifier "Hunter's" (Tier: 1) ... }"
            let low = l.to_lowercase();
            hint = if low.contains("prefix modifier") {
                Some(Gen::Prefix)
            } else if low.contains("suffix modifier") {
                Some(Gen::Suffix)
            } else {
                None
            };
            hint_crafted = low.contains("master crafted");
            continue;
        }
        if META_PREFIXES.iter().any(|p| l.starts_with(p)) {
            continue;
        }
        let mut t = l.as_str();
        if t.ends_with("(implicit)") || t.ends_with("(enchant)") || t.starts_with("{enchant}") {
            implicit_lines.push(t.trim_end_matches("(implicit)").trim().to_string());
            continue;
        }
        let mut crafted = false;
        let mut fractured = false;
        loop {
            if let Some(s) = t.strip_prefix("{crafted}") {
                crafted = true;
                t = s;
            } else if let Some(s) = t.strip_prefix("{fractured}") {
                fractured = true;
                t = s;
            } else if t.starts_with("{range:")
                || t.starts_with("{tags:")
                || t.starts_with("{variant:")
            {
                t = t.split_once('}').map(|x| x.1).unwrap_or(t);
            } else {
                break;
            }
        }
        if let Some(s) = t.strip_suffix("(crafted)") {
            crafted = true;
            t = s.trim();
        }
        if let Some(s) = t.strip_suffix("(fractured)") {
            fractured = true;
            t = s.trim();
        }
        mod_lines_in.push((t.to_string(), crafted || hint_crafted, fractured));
        hints.push(hint);
    }

    let cands = candidates(gd, base_idx, infl);
    let mut used = vec![false; mod_lines_in.len()];
    let mut rolls: Vec<Roll> = vec![];
    let mut values = vec![];
    let mut roll_lines: Vec<Vec<usize>> = vec![];
    // multi-line mods first (most lines first), then single-line; within a size prefer mods
    // whose values fit their ranges, then higher level
    let mut by_size: Vec<&Cand> = cands.iter().collect();
    by_size.sort_by(|a, b| {
        b.lines
            .len()
            .cmp(&a.lines.len())
            .then(gd.m(b.m).level.cmp(&gd.m(a.m).level))
    });
    let max_lines = by_size.first().map(|c| c.lines.len()).unwrap_or(1);
    // pass 1: only matches whose values fit the mod's ranges; pass 2 (fallback, e.g. legacy
    // rolls): anything that matches the text
    for strict in [true, false] {
        for size in (1..=max_lines).rev() {
            loop {
                // preference: values inside the ranges, then natural over essence-only, then level
                let mut best: Option<((bool, bool, u32), &Cand, Vec<usize>, Vec<f64>)> = None;
                for c in by_size.iter().filter(|c| c.lines.len() == size) {
                    if let Some((idxs, vals, inside)) = assign(c, &mod_lines_in, &used) {
                        if strict && !inside {
                            continue;
                        }
                        let crafted_ok = idxs.iter().all(|&i| mod_lines_in[i].1 == c.crafted);
                        let side_ok = idxs.iter().all(|&i| hints[i].is_none_or(|h| h == gd.m(c.m).gen));
                        if !crafted_ok || !side_ok {
                            continue;
                        }
                        let key = (inside, !gd.m(c.m).essence_only, gd.m(c.m).level);
                        if best.as_ref().is_none_or(|b| key > b.0) {
                            best = Some((key, c, idxs, vals));
                        }
                    }
                }
                let Some((_, c, idxs, vals)) = best else {
                    break;
                };
                let fractured = idxs.iter().any(|&i| mod_lines_in[i].2);
                for i in &idxs {
                    used[*i] = true;
                }
                rolls.push(Roll {
                    m: c.m,
                    fractured,
                    crafted: c.crafted,
                });
                values.push((c.m, vals));
                roll_lines.push(idxs);
            }
        }
    }
    // Some mods have identical text on both sides (e.g. "+#% to Item Rarity" prefix 8-12 and
    // suffix 6-10). Use the prefix/suffix limits to fix the side; report what stays ambiguous.
    let max_side = Item::new(base_idx, ilvl, rarity, infl).max_affixes(gd);
    let mut ambiguous = vec![];
    let alt_for = |k: usize, rolls: &Vec<Roll>| -> Option<ModIdx> {
        let r = rolls[k];
        let other = gd.m(r.m).gen.other_side();
        let free: Vec<bool> = mod_lines_in.iter().enumerate().map(|(i, _)| !roll_lines[k].contains(&i)).collect();
        cands
            .iter()
            .filter(|c| gd.m(c.m).gen == other && c.crafted == r.crafted && c.lines.len() == roll_lines[k].len())
            .filter(|_c| roll_lines[k].iter().all(|&i| hints[i].is_none_or(|h| h == other)))
            .filter_map(|c| assign(c, &mod_lines_in, &free).filter(|(ix, _, inside)| *inside && ix.iter().all(|i| roll_lines[k].contains(i))).map(|_| c.m))
            .max_by_key(|m| gd.m(*m).level)
    };
    for gen in [Gen::Prefix, Gen::Suffix] {
        loop {
            let n = rolls.iter().filter(|r| gd.m(r.m).gen == gen).count();
            if n <= max_side {
                break;
            }
            let swap = (0..rolls.len()).filter(|&k| gd.m(rolls[k].m).gen == gen).find_map(|k| alt_for(k, &rolls).map(|m| (k, m)));
            match swap {
                Some((k, m)) => {
                    values[k].0 = m;
                    rolls[k].m = m;
                }
                None => break,
            }
        }
    }
    for k in 0..rolls.len() {
        if let Some(alt) = alt_for(k, &rolls) {
            let other = gd.m(alt).gen;
            let n_other = rolls.iter().filter(|r| gd.m(r.m).gen == other).count();
            if n_other < max_side {
                ambiguous.push(format!(
                    "\"{}\" could be the {} ({}) or the {} ({}); use the in-game advanced copy (Ctrl+Alt+C) to tell",
                    mod_lines(gd, rolls[k].m).join(" / "),
                    gd.m(rolls[k].m).gen.name(),
                    gd.m(rolls[k].m).id,
                    other.name(),
                    gd.m(alt).id
                ));
            }
        }
    }
    let unmatched: Vec<String> = mod_lines_in
        .iter()
        .zip(&used)
        .filter(|(_, u)| !**u)
        .map(|(l, _)| l.0.clone())
        .collect();
    let mut item = Item::new(base_idx, ilvl, rarity, infl).with_mods(rolls);
    if sockets.matches('-').count() >= 5 {
        item.links = 6;
    }
    Ok(Parsed {
        item,
        name,
        quality,
        sockets,
        implicit_lines,
        values,
        unmatched,
        ambiguous,
    })
}

struct Cand {
    m: ModIdx,
    crafted: bool,
    lines: Vec<Vec<Part>>,
}

/// Every explicit mod that could be on this base: natural mods of any influence of its class,
/// elevated versions of those families, bench crafts for the class, essence mods, unveiled mods.
fn candidates(gd: &GameData, b: BaseIdx, infl: Influences) -> Vec<Cand> {
    let base = gd.base(b);
    let cls = &base.item_class;
    let mut set: BTreeSet<ModIdx> = BTreeSet::new();
    for (i, m) in gd.mods.iter().enumerate() {
        let i = i as ModIdx;
        if m.gen == Gen::Other {
            continue;
        }
        if (m.domain == base.domain || m.domain == "unveiled") && gd.weight_on(i, b) > 0 {
            set.insert(i);
        }
    }
    let fams: BTreeSet<u32> = set.iter().map(|&m| gd.family_idx(m)).collect();
    for f in fams {
        if let Some(e) = gd.families.get(f as usize).and_then(|x| x.elevated) {
            set.insert(e);
        }
    }
    for o in &gd.bench {
        if o.classes.iter().any(|c| c == cls) {
            set.insert(o.module);
        }
    }
    for e in &gd.essences {
        if let Some(m) = e.mods.get(cls).and_then(|id| gd.mod_idx(id)) {
            set.insert(m);
        }
    }
    set.into_iter()
        .map(|m| Cand {
            m,
            crafted: gd.m(m).crafted,
            lines: mod_lines(gd, m).iter().map(|l| parse_template(l)).collect(),
        })
        .filter(|c| !c.lines.is_empty() && c.lines.iter().all(|l| !l.is_empty()))
        // influenced mods only for the item's own influences
        .filter(|c| gd.m(c.m).influence.is_none_or(|i| infl.contains(i)))
        .collect()
}

/// Match all of a candidate's template lines to distinct unused text lines (any order).
fn assign(
    c: &Cand,
    lines: &[(String, bool, bool)],
    used: &[bool],
) -> Option<(Vec<usize>, Vec<f64>, bool)> {
    let mut idxs = vec![];
    let mut vals = vec![];
    let mut inside_all = true;
    for t in &c.lines {
        let mut hit = None;
        for (i, (text, _, _)) in lines.iter().enumerate() {
            if used[i] || idxs.contains(&i) {
                continue;
            }
            if let Some((v, inside)) = match_line(t, text) {
                // prefer a line whose values fit
                if inside || hit.is_none() {
                    hit = Some((i, v, inside));
                    if inside {
                        break;
                    }
                }
            }
        }
        let (i, v, inside) = hit?;
        idxs.push(i);
        vals.extend(v);
        inside_all &= inside;
    }
    Some((idxs, vals, inside_all))
}
