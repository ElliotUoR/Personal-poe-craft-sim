//! Item builder support (spec §4.A/§4.B): legality checks, the mod pool of an item with the
//! reason each mod is blocked, and "trick" hints computed from the pool.

use crate::data::{FamIdx, GameData, Gen, ModIdx};
use crate::item::{Item, Rarity, Roll};
use crate::mechanics::{harvest_add_cost, HARVEST_REFORGE};
use serde::Serialize;
use std::collections::BTreeMap;

/// Why a mod cannot be added right now (None = it can roll).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Blocked {
    /// a mod of the same group is already on the item
    Group,
    /// no open prefix/suffix slot
    NoSlot,
    /// item level too low
    ItemLevel,
}

#[derive(Debug, Clone, Serialize)]
pub struct PoolEntry {
    pub id: String,
    pub side: &'static str,
    pub influence: Option<&'static str>,
    pub tier: u32,
    pub level: u32,
    pub weight: u32,
    /// chance among mods of the same side that can roll now (0 when blocked)
    pub chance: f64,
    pub text: String,
    pub tags: Vec<String>,
    pub family: FamIdx,
    pub blocked: Option<Blocked>,
}

/// Every natural mod for this base/influences (ignoring item level for display), with weights,
/// tiers and whether it can roll on `item` right now.
pub fn pool_view(gd: &GameData, item: &Item) -> Vec<PoolEntry> {
    let all = gd.spawn_pool(item.base, item.influences, 100);
    let groups = item.group_ids(gd);
    let open_p = item.open_slots(gd, Gen::Prefix) > 0;
    let open_s = item.open_slots(gd, Gen::Suffix) > 0;
    let blocked = |mi: ModIdx| -> Option<Blocked> {
        let m = gd.m(mi);
        if m.level > item.ilvl {
            Some(Blocked::ItemLevel)
        } else if m.group_ids.iter().any(|g| groups.binary_search(g).is_ok()) {
            Some(Blocked::Group)
        } else if (m.gen == Gen::Prefix && !open_p) || (m.gen == Gen::Suffix && !open_s) {
            Some(Blocked::NoSlot)
        } else {
            None
        }
    };
    let mut totals = [0u64; 2];
    for &(mi, w) in all.iter() {
        if blocked(mi).is_none() {
            totals[(gd.m(mi).gen == Gen::Suffix) as usize] += w as u64;
        }
    }
    all.iter()
        .map(|&(mi, w)| {
            let m = gd.m(mi);
            let b = blocked(mi);
            let tot = totals[(m.gen == Gen::Suffix) as usize].max(1) as f64;
            PoolEntry {
                id: m.id.clone(),
                side: m.gen.name(),
                influence: m.influence.map(|i| i.name()),
                tier: gd.tier(mi, item.base),
                level: m.level,
                weight: w,
                chance: if b.is_none() { w as f64 / tot } else { 0.0 },
                text: crate::itemtext::clean(&m.text).replace('\n', " / "),
                tags: m.tags.clone(),
                family: gd.family_idx(mi),
                blocked: b,
            }
        })
        .collect()
}

/// Problems that make an item impossible in the game (empty = legal).
pub fn validate(gd: &GameData, item: &Item) -> Vec<String> {
    let mut errs = vec![];
    let base = gd.base(item.base);
    let cls = &base.item_class;
    let mx = item.max_affixes(gd);
    for gen in [Gen::Prefix, Gen::Suffix] {
        let n = item.n_side(gd, gen);
        if n > mx {
            errs.push(format!("{n} {}es, but a {} {} can have at most {mx}", gen.name(), item.rarity.name(), cls));
        }
    }
    if item.rarity == Rarity::Normal && !item.mods.is_empty() {
        errs.push("a normal item cannot have explicit modifiers".into());
    }
    if item.rarity == Rarity::Magic && item.mods.len() > 2 {
        errs.push("a magic item has at most 1 prefix and 1 suffix".into());
    }
    let mut seen_groups: BTreeMap<u32, &str> = BTreeMap::new();
    let multimod = item.mods.iter().any(|r| gd.m(r.m).meta == Some(crate::data::MetaKind::Multimod));
    let crafted = item.crafted_count();
    if crafted > if multimod { 3 } else { 1 } {
        errs.push(format!("{crafted} crafted modifiers (only {} allowed{})", if multimod { 3 } else { 1 }, if multimod { "" } else { "; craft multimod for up to 3" }));
    }
    let fractured = item.mods.iter().filter(|r| r.fractured).count();
    if fractured > 1 {
        errs.push(format!("{fractured} fractured modifiers (at most 1)"));
    }
    let n_infl = item.influences.iter().count();
    if n_infl > 2 {
        errs.push("an item can have at most two influences".into());
    }
    for i in item.influences.iter() {
        if gd.influence_tag(item.base, i).is_err() {
            errs.push(format!("{cls} items cannot have {} influence", i.name()));
        }
    }
    for r in &item.mods {
        let m = gd.m(r.m);
        for g in &m.group_ids {
            if let Some(other) = seen_groups.insert(*g, &m.id) {
                errs.push(format!("{} and {other} share mod group {}", m.id, gd.groups[*g as usize]));
            }
        }
        if let Some(e) = mod_problem(gd, item, r) {
            errs.push(e);
        }
    }
    errs
}

/// Why this roll could not be on this item, if anything.
fn mod_problem(gd: &GameData, item: &Item, r: &Roll) -> Option<String> {
    let m = gd.m(r.m);
    let cls = gd.item_class(item.base);
    if r.crafted || m.crafted {
        let ok = gd.bench.iter().any(|b| b.module == r.m && b.classes.iter().any(|c| c == cls));
        return (!ok).then(|| format!("{} is not a bench craft for {cls}", m.id));
    }
    if let Some(inf) = m.influence {
        if !item.influences.contains(inf) {
            return Some(format!("{} needs {} influence", m.id, inf.name()));
        }
    }
    if m.elevated {
        // elevated mods come from Dominance/Awakener, only on influence families of this base
        let fam = gd.family_idx(r.m);
        let ok = fam != crate::data::NO_FAMILY && !gd.tiers_for(fam, item.base).is_empty();
        return (!ok).then(|| format!("{} (elevated) cannot exist on {cls}", m.id));
    }
    let essence_ok = m.essence_only && gd.essences.iter().any(|e| e.mods.get(cls).is_some_and(|id| id == &m.id));
    if gd.weight_on(r.m, item.base) == 0 && !essence_ok && m.domain != "unveiled" && m.domain != "veiled" {
        return Some(format!("{} cannot roll on {}", m.id, gd.base(item.base).name));
    }
    if m.level > item.ilvl {
        return Some(format!("{} needs item level {} (item is {})", m.id, m.level, item.ilvl));
    }
    None
}

/// A crafting "trick" visible in the pool.
#[derive(Debug, Clone, Serialize)]
pub struct Hint {
    pub kind: &'static str,
    pub text: String,
}

/// Hints computed from the pool: for every Harvest tag, a side where exactly one mod family
/// carries it (a locked reforge / "add" with that tag guarantees it), bench-craftable families,
/// elevatable families.
pub fn hints(gd: &GameData, item: &Item) -> Vec<Hint> {
    let pool = gd.spawn_pool(item.base, item.influences, item.ilvl);
    let mut out = vec![];
    let tags: Vec<&str> = HARVEST_REFORGE.iter().map(|x| x.0).collect();
    for tag in tags {
        for gen in [Gen::Prefix, Gen::Suffix] {
            let mut fams: BTreeMap<FamIdx, ModIdx> = BTreeMap::new();
            for &(mi, _) in pool.iter() {
                let m = gd.m(mi);
                if m.gen == gen && m.has_tag(tag) {
                    fams.entry(gd.family_idx(mi)).or_insert(mi);
                }
            }
            if fams.len() == 1 {
                let (_, mi) = fams.into_iter().next().unwrap();
                let other = gen.other_side().name();
                let add = if item.influences.is_empty() && harvest_add_cost(tag).is_some() { format!(" (also 'harvest add {tag}' on this non-influenced item)") } else { String::new() };
                out.push(Hint {
                    kind: "guaranteed_by_tag",
                    text: format!(
                        "\"{}\" is the only {tag}-tagged {} here: lock the {other}es (all 3 full) and 'harvest reforge {tag}' guarantees it{add}",
                        crate::itemtext::clean(&gd.m(mi).text).replace('\n', " / "),
                        gen.name()
                    ),
                });
            }
        }
    }
    // elevatable families present in the pool (armour pieces only)
    if crate::mechanics::ARMOUR_CLASSES.contains(&gd.item_class(item.base)) {
        let mut seen = std::collections::BTreeSet::new();
        for &(mi, _) in pool.iter() {
            let f = gd.family_idx(mi);
            if gd.m(mi).influence.is_some() {
                seen.insert(f);
            }
        }
        let n = seen.iter().filter(|f| gd.families[**f as usize].elevated.is_some()).count();
        if n > 0 {
            out.push(Hint { kind: "elevatable", text: format!("{n} influenced mod families here have Elevated versions (Orb of Dominance on T1, then Awakener's Orb to combine)") });
        }
    }
    out
}
