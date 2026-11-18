//! pathofexile.com trade search links for an item (spec §4.H, "Check price").
//!
//! Verified 2026-11-18 in the owner's browser: the trade page keeps the search in its URL as
//! `/trade/search/<League>/<code>`, where `<code>` is the `query` JSON object gzip-compressed and
//! base64url-encoded (no padding); a generated link opened with every filter filled in.
//! Stat ids come from data/trade_stats.json (`tools/fetch_trade_stats.py`).
//! Status: `securable` (instant buyout); `online` returned nothing in Covens.

use crate::data::{GameData, Gen};
use crate::item::Item;
use crate::itemtext::{clean, mod_lines};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

#[derive(Deserialize)]
struct RawStats {
    stats: Vec<RawStat>,
}
#[derive(Deserialize)]
struct RawStat {
    id: String,
    text: String,
    #[serde(rename = "type")]
    typ: String,
}

pub struct TradeStats {
    /// (group, normalised text) -> stat id
    map: FxHashMap<(String, String), String>,
    pub fetched: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Filter {
    pub line: String,
    pub stat: String,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeLink {
    pub url: String,
    pub query: Value,
    pub filters: Vec<Filter>,
    /// mod lines with no trade stat (e.g. the elevated "Ignore Stuns" line)
    pub skipped: Vec<String>,
}

/// Normalise a stat line for matching: numbers and "(a-b)" ranges become "#", a sign before "#"
/// is dropped, lowercase, single spaces.
pub fn normalise(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let starts_number = c.is_ascii_digit() || ((c == '-' || c == '+') && i + 1 < b.len() && b[i + 1].is_ascii_digit());
        if c == '(' && i + 1 < b.len() && (b[i + 1].is_ascii_digit() || b[i + 1] == '-') {
            // range "(a-b)"
            if let Some(j) = b[i..].iter().position(|&x| x == ')') {
                let inner: String = b[i + 1..i + j].iter().collect();
                if inner.chars().all(|x| x.is_ascii_digit() || x == '-' || x == '.') {
                    out.push('#');
                    i += j + 1;
                    continue;
                }
            }
        }
        if starts_number {
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == '.' || b[i] == '-' || b[i] == '+') {
                i += 1;
            }
            out.push('#');
            continue;
        }
        out.push(c);
        i += 1;
    }
    let out = out.replace("+#", "#").replace("-#", "#");
    out.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// The value used in the filter: the lower end of the first range (or the literal number);
/// negative values become a maximum.
fn line_value(line: &str) -> Option<f64> {
    let s = line.replace(['(', ')'], " ");
    let mut it = s.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter(|t| !t.is_empty() && t.chars().any(|c| c.is_ascii_digit()));
    let first = it.next()?;
    // "-20": a negative literal; "1.6-2" (from a range): take the lower end
    if first.starts_with('-') {
        return first.parse().ok();
    }
    first.split('-').next().and_then(|a| a.parse().ok())
}

impl TradeStats {
    pub fn load(data_dir: &Path) -> Result<TradeStats, String> {
        let p = data_dir.join("trade_stats.json");
        let txt = std::fs::read_to_string(&p).map_err(|e| format!("cannot read {}: {e} (run tools/fetch_trade_stats.py)", p.display()))?;
        let raw: RawStats = serde_json::from_str(&txt).map_err(|e| e.to_string())?;
        let fetched = serde_json::from_str::<Value>(&txt).ok().and_then(|v| v["fetched"].as_str().map(|s| s.to_string())).unwrap_or_default();
        let mut map = FxHashMap::default();
        for s in raw.stats {
            map.entry((s.typ, normalise(&s.text))).or_insert(s.id);
        }
        Ok(TradeStats { map, fetched })
    }

    fn find(&self, group: &str, line: &str) -> Option<(String, Option<f64>)> {
        let n = normalise(line);
        let groups = [group, "explicit"];
        for g in groups {
            if let Some(id) = self.map.get(&(g.to_string(), n.clone())) {
                let v = if n.contains('#') { line_value(line) } else { None };
                return Some((id.clone(), v));
            }
        }
        // "You can apply an additional Curse" -> "You can apply # additional Curses" (value 1)
        if n.contains(" an additional ") {
            let words: Vec<&str> = n.split(' ').collect();
            if let Some(k) = words.iter().position(|w| *w == "additional") {
                let mut w: Vec<String> = words.iter().map(|s| s.to_string()).collect();
                w[k - 1] = "#".into();
                if k + 1 < w.len() && !w[k + 1].ends_with('s') {
                    w[k + 1].push('s');
                }
                let alt = w.join(" ");
                for g in groups {
                    if let Some(id) = self.map.get(&(g.to_string(), alt.clone())) {
                        return Some((id.clone(), Some(1.0)));
                    }
                }
            }
        }
        None
    }
}

/// Trade category for an item class (used when searching any base of the class).
pub fn category(cls: &str) -> Option<&'static str> {
    Some(match cls {
        "Body Armour" => "armour.chest",
        "Boots" => "armour.boots",
        "Gloves" => "armour.gloves",
        "Helmet" => "armour.helmet",
        "Shield" => "armour.shield",
        "Quiver" => "armour.quiver",
        "Ring" => "accessory.ring",
        "Amulet" => "accessory.amulet",
        "Belt" => "accessory.belt",
        "Jewel" => "jewel.base",
        "AbyssJewel" => "jewel.abyss",
        "Bow" => "weapon.bow",
        "Claw" => "weapon.claw",
        "Dagger" => "weapon.basedagger",
        "Rune Dagger" => "weapon.runedagger",
        "One Hand Axe" => "weapon.oneaxe",
        "One Hand Mace" => "weapon.onemace",
        "One Hand Sword" | "Thrusting One Hand Sword" => "weapon.onesword",
        "Sceptre" => "weapon.sceptre",
        "Staff" => "weapon.basestaff",
        "Warstaff" => "weapon.warstaff",
        "Two Hand Axe" => "weapon.twoaxe",
        "Two Hand Mace" => "weapon.twomace",
        "Two Hand Sword" => "weapon.twosword",
        "Wand" => "weapon.wand",
        _ => return None,
    })
}

pub struct LinkOpts<'a> {
    pub league: &'a str,
    /// search any base of the item class instead of the exact base type
    pub any_base: bool,
    pub status: &'a str,
}

pub fn build_link(gd: &GameData, ts: &TradeStats, item: &Item, o: &LinkOpts) -> TradeLink {
    let mut filters: Vec<Filter> = vec![];
    let mut skipped = vec![];
    let mut seen = std::collections::BTreeSet::new();
    for gen in [Gen::Prefix, Gen::Suffix] {
        for r in item.side(gd, gen) {
            if gd.m(r.m).meta.is_some() || gd.m(r.m).domain == "veiled" {
                continue;
            }
            let group = if r.crafted {
                "crafted"
            } else if r.fractured {
                "fractured"
            } else {
                "explicit"
            };
            for line in mod_lines(gd, r.m) {
                let line = clean(&line);
                match ts.find(group, &line) {
                    Some((id, v)) if seen.insert(id.clone()) => filters.push(Filter { line, stat: id, value: v }),
                    Some(_) => {}
                    None => skipped.push(line),
                }
            }
        }
    }
    let stats: Vec<Value> = filters
        .iter()
        .map(|f| match f.value {
            Some(v) if v < 0.0 => json!({"id": f.stat, "value": {"max": v}}),
            Some(v) => json!({"id": f.stat, "value": {"min": v}}),
            None => json!({"id": f.stat}),
        })
        .collect();
    let mut misc = serde_json::Map::new();
    for i in item.influences.iter() {
        misc.insert(format!("{}_item", i.name()), json!({"option": "true"}));
    }
    let cls = gd.item_class(item.base);
    let mut query = json!({ "status": {"option": o.status}, "stats": [{"type": "and", "filters": stats}] });
    let mut filt = serde_json::Map::new();
    if !misc.is_empty() {
        filt.insert("misc_filters".into(), json!({"filters": misc}));
    }
    match (o.any_base, category(cls)) {
        (true, Some(c)) => {
            filt.insert("type_filters".into(), json!({"filters": {"category": {"option": c}}}));
        }
        _ => query["type"] = json!(gd.base(item.base).name),
    }
    if !filt.is_empty() {
        query["filters"] = Value::Object(filt);
    }
    let code = encode(&query);
    let league = o.league.replace(' ', "%20");
    TradeLink { url: format!("https://www.pathofexile.com/trade/search/{league}/{code}"), query, filters, skipped }
}

/// gzip + base64url (no padding) of the query JSON, as the trade page does.
pub fn encode(query: &Value) -> String {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(query.to_string().as_bytes()).unwrap();
    let bytes = gz.finish().unwrap();
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for ch in bytes.chunks(3) {
        let n = (ch[0] as u32) << 16 | (*ch.get(1).unwrap_or(&0) as u32) << 8 | *ch.get(2).unwrap_or(&0) as u32;
        let k = ch.len() + 1;
        for i in 0..k {
            out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// Inverse of `encode` (for tests and for reading links back).
pub fn decode(code: &str) -> Option<Value> {
    let mut bits = 0u32;
    let mut nb = 0;
    let mut bytes = vec![];
    for c in code.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => continue,
        } as u32;
        bits = bits << 6 | v;
        nb += 6;
        if nb >= 8 {
            nb -= 8;
            bytes.push((bits >> nb) as u8);
            bits &= (1 << nb) - 1;
        }
    }
    let mut s = String::new();
    std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(&bytes[..]), &mut s).ok()?;
    serde_json::from_str(&s).ok()
}
