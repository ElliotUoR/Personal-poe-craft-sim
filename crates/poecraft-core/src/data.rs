//! Game data: bases, mods, mod families and per-base tiers, readable mod names, mod pools.
//!
//! Reads `data/gamedata.json` (built from RePoE by `tools/build_data.py`; the same file the
//! Python reference engine uses).
//!
//! Terminology (same as the Python version):
//! * **mod**: one RePoE mod id, e.g. `AdditionalCritWithAttacksInfluence2_`.
//! * **family**: all tiers of "the same" mod: same side, mod group, influence (or "crafted")
//!   and normalised mod type.
//! * **tier**: 1 = best tier that can roll *on the given base*; elevated mods are tier 0.

use rustc_hash::{FxHashMap, FxHashSet};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub type ModIdx = u32;
pub type BaseIdx = u32;
pub type TagIdx = u32;
pub type FamIdx = u32;

#[derive(Debug, Clone)]
pub struct DataError(pub String);
impl std::fmt::Display for DataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for DataError {}

// ------------------------------------------------------------------ influences
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Influence {
    Shaper,
    Elder,
    Crusader,
    Redeemer,
    Hunter,
    Warlord,
}

impl Influence {
    pub const ALL: [Influence; 6] = [
        Influence::Shaper,
        Influence::Elder,
        Influence::Crusader,
        Influence::Redeemer,
        Influence::Hunter,
        Influence::Warlord,
    ];
    /// Suffix of the RePoE influence spawn tag (e.g. `body_armour_basilisk` = Hunter).
    /// Source: RePoE item_classes.json influence_tags, cross-checked with mod names.
    pub fn tag_suffix(self) -> &'static str {
        match self {
            Influence::Shaper => "shaper",
            Influence::Elder => "elder",
            Influence::Crusader => "crusader",
            Influence::Redeemer => "eyrie",
            Influence::Hunter => "basilisk",
            Influence::Warlord => "adjudicator",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Influence::Shaper => "shaper",
            Influence::Elder => "elder",
            Influence::Crusader => "crusader",
            Influence::Redeemer => "redeemer",
            Influence::Hunter => "hunter",
            Influence::Warlord => "warlord",
        }
    }
    pub fn parse(s: &str) -> Option<Influence> {
        Influence::ALL
            .into_iter()
            .find(|i| i.name() == s.to_ascii_lowercase())
    }
    pub fn bit(self) -> u8 {
        1 << (self as u8)
    }
    pub fn is_conqueror(self) -> bool {
        matches!(
            self,
            Influence::Crusader | Influence::Redeemer | Influence::Hunter | Influence::Warlord
        )
    }
}

/// A set of influences as a bit mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Influences(pub u8);
impl Influences {
    pub fn contains(self, i: Influence) -> bool {
        self.0 & i.bit() != 0
    }
    pub fn with(self, i: Influence) -> Self {
        Influences(self.0 | i.bit())
    }
    pub fn union(self, o: Self) -> Self {
        Influences(self.0 | o.0)
    }
    pub fn intersects(self, o: Self) -> bool {
        self.0 & o.0 != 0
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
    pub fn iter(self) -> impl Iterator<Item = Influence> {
        Influence::ALL
            .into_iter()
            .filter(move |i| self.contains(*i))
    }
}

// ------------------------------------------------------------------ mods
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Gen {
    Prefix,
    Suffix,
    Other,
}

impl Gen {
    pub fn name(self) -> &'static str {
        match self {
            Gen::Prefix => "prefix",
            Gen::Suffix => "suffix",
            Gen::Other => "other",
        }
    }
    pub fn other_side(self) -> Gen {
        match self {
            Gen::Prefix => Gen::Suffix,
            Gen::Suffix => Gen::Prefix,
            Gen::Other => Gen::Other,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Mod {
    pub id: String,
    pub name: String,
    pub gen: Gen,
    pub groups: Vec<String>,
    pub typ: String,
    pub tags: Vec<String>,
    pub level: u32,
    pub spawn: Vec<(TagIdx, u32)>,
    pub genw: Vec<(TagIdx, u32)>,
    pub text: String,
    pub essence_only: bool,
    pub influence: Option<Influence>,
    pub elevated: bool,
    pub crafted: bool,
    pub domain: String,
    pub stats: Vec<(String, f64, f64)>,
    /// Interned mod groups (for fast "same group already on the item" checks).
    pub group_ids: Vec<u32>,
    /// Interned implicit tags (attack, caster, critical, ...).
    pub tag_ids: Vec<TagIdx>,
    pub meta: Option<MetaKind>,
}

/// Bench "meta" crafts, detected by RePoE mod type so every item class's variant is covered
/// (armour "...B", jewellery "...C" and weapon versions have different ids).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetaKind {
    /// "Prefix Modifiers cannot be changed" (None) or "N Prefix Modifier(s) cannot be changed".
    LockPrefixes(Option<u8>),
    LockSuffixes(Option<u8>),
    /// "Can have up to 3 Crafted Modifiers"
    Multimod,
    /// "Cannot roll Attack Modifiers"
    NoAttackMods,
    /// "Cannot roll Caster Modifiers"
    NoCasterMods,
}

fn meta_kind(typ: &str, text: &str) -> Option<MetaKind> {
    let count = || {
        text.split_whitespace()
            .next()
            .and_then(|w| w.parse::<u8>().ok())
    };
    match typ {
        "ItemGenerationCannotChangePrefixes" => Some(MetaKind::LockPrefixes(None)),
        "ItemGenerationCannotChangeSuffixes" => Some(MetaKind::LockSuffixes(None)),
        "ItemGenerationCannotChangePrefixesCount" => Some(MetaKind::LockPrefixes(count())),
        "ItemGenerationCannotChangeSuffixesCount" => Some(MetaKind::LockSuffixes(count())),
        "ItemGenerationCanHaveMultipleCraftedMods" => Some(MetaKind::Multimod),
        "ItemGenerationCannotRollAttackAffixes" => Some(MetaKind::NoAttackMods),
        "ItemGenerationCannotRollCasterAffixes" => Some(MetaKind::NoCasterMods),
        _ => None,
    }
}

impl Mod {
    pub fn short(&self) -> String {
        self.text.replace('\n', " / ")
    }
    pub fn has_tag(&self, t: &str) -> bool {
        self.tags.iter().any(|x| x == t)
    }
}

/// Family key: (side, first group, influence or "crafted", normalised type).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FamilyKey {
    pub gen: Gen,
    pub group: String,
    pub influence: String, // "" = none, "crafted", or influence name
    pub typ: String,
}

#[derive(Debug, Clone)]
pub struct Family {
    pub key: FamilyKey,
    /// Bench-crafted (or veiled placeholder) family: reachable by id/alias only, not by
    /// free-text search (same as the Python reference).
    pub crafted: bool,
    /// All natural tiers across every item class, best (highest level) first.
    pub tiers: Vec<ModIdx>,
    pub elevated: Option<ModIdx>,
}

#[derive(Debug, Clone)]
pub struct Base {
    pub id: String,
    pub name: String,
    pub item_class: String,
    pub tags: Vec<TagIdx>,
    pub domain: String,
    pub implicits: Vec<String>,
    pub drop_level: u32,
    pub properties: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct Essence {
    pub id: String,
    pub name: String,
    pub level: u32,
    pub item_level_restriction: Option<u32>,
    pub mods: BTreeMap<String, String>, // item class -> mod id
}

#[derive(Debug, Clone)]
pub struct BenchOption {
    pub module: ModIdx,
    pub classes: Vec<String>,
    pub cost: BTreeMap<String, f64>,
}

// ------------------------------------------------------------------ raw JSON
#[derive(Deserialize)]
struct RawData {
    source: serde_json::Value,
    classes: BTreeMap<String, RawClass>,
    bases: BTreeMap<String, RawBase>,
    mods: BTreeMap<String, RawMod>,
    essences: BTreeMap<String, RawEssence>,
    bench: Vec<RawBench>,
}
#[derive(Deserialize)]
struct RawClass {
    influence_tags: Vec<String>,
}
#[derive(Deserialize)]
struct RawBase {
    name: String,
    item_class: String,
    tags: Vec<String>,
    #[serde(default)]
    drop_level: u32,
    #[serde(default)]
    implicits: Vec<String>,
    #[serde(default = "default_domain")]
    domain: String,
    #[serde(default)]
    properties: serde_json::Value,
}
fn null_as_default<'de, D: serde::Deserializer<'de>, T: Default + Deserialize<'de>>(
    d: D,
) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}
fn default_domain() -> String {
    "item".into()
}
#[derive(Deserialize)]
struct RawMod {
    name: String,
    gen: String,
    groups: Vec<String>,
    #[serde(rename = "type")]
    typ: String,
    tags: Vec<String>,
    level: u32,
    spawn: Vec<(String, u32)>,
    #[serde(default)]
    genw: Vec<(String, u32)>,
    #[serde(default, deserialize_with = "null_as_default")]
    text: String,
    #[serde(default)]
    essence_only: bool,
    #[serde(default = "default_domain")]
    domain: String,
    #[serde(default)]
    stats: Vec<(String, f64, f64)>,
}
#[derive(Deserialize)]
struct RawEssence {
    name: String,
    level: u32,
    item_level_restriction: Option<u32>,
    mods: BTreeMap<String, Option<String>>,
}
#[derive(Deserialize)]
struct RawBench {
    #[serde(rename = "mod")]
    module: String,
    classes: Vec<String>,
    cost: BTreeMap<String, f64>,
}

// ------------------------------------------------------------------ GameData
pub struct GameData {
    pub source: serde_json::Value,
    pub class_influence_tags: BTreeMap<String, Vec<TagIdx>>,
    pub bases: Vec<Base>,
    pub base_index: FxHashMap<String, BaseIdx>,
    pub mods: Vec<Mod>,
    pub mod_index: FxHashMap<String, ModIdx>,
    pub tags: Vec<String>,
    pub tag_index: FxHashMap<String, TagIdx>,
    pub families: Vec<Family>,
    pub family_of: Vec<FamIdx>, // per mod; u32::MAX = no family (crafted/implicit/veiled)
    pub family_index: FxHashMap<FamilyKey, FamIdx>,
    pub essences: Vec<Essence>,
    pub bench: Vec<BenchOption>,
    pub groups: Vec<String>,
    pub group_index: FxHashMap<String, u32>,
    pub aliases: FxHashMap<String, String>,
    pub data_dir: PathBuf,
    tiers_cache: Mutex<FxHashMap<(FamIdx, BaseIdx), std::sync::Arc<Vec<ModIdx>>>>,
    pool_cache: Mutex<FxHashMap<(BaseIdx, Influences, u32), std::sync::Arc<Vec<(ModIdx, u32)>>>>,
}

pub const NO_FAMILY: FamIdx = u32::MAX;

/// Python: re.sub(r"(Influence)?Maven(V\d+)?_*$", "", type)
fn normalise_type(t: &str) -> String {
    let s = t.trim_end_matches('_');
    // optional V\d+
    let mut end = s.len();
    let b = s.as_bytes();
    let mut i = end;
    while i > 0 && b[i - 1].is_ascii_digit() {
        i -= 1;
    }
    if i < end && i > 0 && b[i - 1] == b'V' {
        end = i - 1;
    }
    let core = &s[..end];
    if let Some(stripped) = core.strip_suffix("Maven") {
        stripped
            .strip_suffix("Influence")
            .unwrap_or(stripped)
            .to_string()
    } else {
        t.to_string()
    }
}

impl GameData {
    pub fn load(path: &Path) -> Result<GameData, DataError> {
        let txt = std::fs::read_to_string(path)
            .map_err(|e| DataError(format!("cannot read {}: {e}", path.display())))?;
        let raw: RawData =
            serde_json::from_str(&txt).map_err(|e| DataError(format!("bad game data: {e}")))?;
        let data_dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();

        let mut tags: Vec<String> = Vec::new();
        let mut tag_index: FxHashMap<String, TagIdx> = FxHashMap::default();
        let mut intern = |t: &str, tags: &mut Vec<String>| -> TagIdx {
            if let Some(&i) = tag_index.get(t) {
                return i;
            }
            let i = tags.len() as TagIdx;
            tags.push(t.to_string());
            tag_index.insert(t.to_string(), i);
            i
        };

        // influence tag -> influence
        let mut infl_of_tag: FxHashMap<String, Influence> = FxHashMap::default();
        let mut class_influence_tags = BTreeMap::new();
        for (cls, c) in &raw.classes {
            let mut v = Vec::new();
            for t in &c.influence_tags {
                for inf in Influence::ALL {
                    if t.ends_with(&format!("_{}", inf.tag_suffix())) {
                        infl_of_tag.insert(t.clone(), inf);
                    }
                }
                v.push(intern(t, &mut tags));
            }
            class_influence_tags.insert(cls.clone(), v);
        }

        let mut bases = Vec::new();
        let mut base_index = FxHashMap::default();
        for (id, b) in &raw.bases {
            let t = b.tags.iter().map(|x| intern(x, &mut tags)).collect();
            base_index.insert(id.clone(), bases.len() as BaseIdx);
            bases.push(Base {
                id: id.clone(),
                name: b.name.clone(),
                item_class: b.item_class.clone(),
                tags: t,
                domain: b.domain.clone(),
                implicits: b.implicits.clone(),
                drop_level: b.drop_level,
                properties: b.properties.clone(),
            });
        }

        let bench_mods: FxHashSet<&str> = raw.bench.iter().map(|b| b.module.as_str()).collect();
        let mut groups: Vec<String> = Vec::new();
        let mut group_index: FxHashMap<String, u32> = FxHashMap::default();
        let mut mods = Vec::new();
        let mut mod_index = FxHashMap::default();
        for (id, m) in &raw.mods {
            let influence = m
                .spawn
                .iter()
                .find_map(|(t, _)| infl_of_tag.get(t).copied());
            let spawn = m
                .spawn
                .iter()
                .map(|(t, w)| (intern(t, &mut tags), *w))
                .collect();
            let genw = m
                .genw
                .iter()
                .map(|(t, w)| (intern(t, &mut tags), *w))
                .collect();
            let tag_ids = m.tags.iter().map(|t| intern(t, &mut tags)).collect();
            let group_ids = m
                .groups
                .iter()
                .map(|g| {
                    *group_index.entry(g.clone()).or_insert_with(|| {
                        groups.push(g.clone());
                        (groups.len() - 1) as u32
                    })
                })
                .collect();
            mod_index.insert(id.clone(), mods.len() as ModIdx);
            mods.push(Mod {
                id: id.clone(),
                name: m.name.clone(),
                gen: match m.gen.as_str() {
                    "prefix" => Gen::Prefix,
                    "suffix" => Gen::Suffix,
                    _ => Gen::Other,
                },
                groups: m.groups.clone(),
                typ: m.typ.clone(),
                tags: m.tags.clone(),
                level: m.level,
                spawn,
                genw,
                text: m.text.clone(),
                essence_only: m.essence_only,
                influence,
                elevated: m.name.contains("Elevated"),
                crafted: bench_mods.contains(id.as_str()),
                domain: m.domain.clone(),
                stats: m.stats.clone(),
                group_ids,
                tag_ids,
                meta: if bench_mods.contains(id.as_str()) {
                    meta_kind(&m.typ, &m.text)
                } else {
                    None
                },
            });
        }

        let essences = raw
            .essences
            .iter()
            .map(|(id, e)| Essence {
                id: id.clone(),
                name: e.name.clone(),
                level: e.level,
                item_level_restriction: e.item_level_restriction,
                // Remnant of Corruption etc. have null mods
                mods: e
                    .mods
                    .iter()
                    .filter_map(|(k, v)| v.clone().map(|v| (k.clone(), v)))
                    .collect(),
            })
            .collect();
        let bench = raw
            .bench
            .iter()
            .filter_map(|b| {
                mod_index.get(&b.module).map(|&mi| BenchOption {
                    module: mi,
                    classes: b.classes.clone(),
                    cost: b.cost.clone(),
                })
            })
            .collect();

        let mut gd = GameData {
            source: raw.source,
            class_influence_tags,
            bases,
            base_index,
            mods,
            mod_index,
            tags,
            tag_index,
            families: Vec::new(),
            family_of: Vec::new(),
            family_index: FxHashMap::default(),
            essences,
            bench,
            groups,
            group_index,
            aliases: FxHashMap::default(),
            data_dir: data_dir.clone(),
            tiers_cache: Mutex::new(FxHashMap::default()),
            pool_cache: Mutex::new(FxHashMap::default()),
        };
        gd.build_families();
        gd.aliases = load_aliases(&data_dir);
        Ok(gd)
    }

    /// Default location: `<repo>/data/gamedata.json`, found from POECRAFT_DATA, the current
    /// directory or the executable's ancestors.
    pub fn load_default() -> Result<GameData, DataError> {
        Self::load(&default_data_dir()?.join("gamedata.json"))
    }

    fn raw_family_key(&self, m: &Mod) -> FamilyKey {
        FamilyKey {
            gen: m.gen,
            group: m.groups.first().cloned().unwrap_or_else(|| m.id.clone()),
            influence: if m.crafted {
                "crafted".into()
            } else {
                m.influence
                    .map(|i| i.name().to_string())
                    .unwrap_or_default()
            },
            typ: normalise_type(&m.typ),
        }
    }

    fn build_families(&mut self) {
        let mut fams: BTreeMap<FamilyKey, Family> = BTreeMap::new();
        for (i, m) in self.mods.iter().enumerate() {
            if m.gen == Gen::Other || m.typ.starts_with("OLD") {
                continue;
            }
            let key = self.raw_family_key(m);
            let crafted = m.crafted || m.domain == "veiled";
            let fam = fams.entry(key.clone()).or_insert_with(|| Family {
                key,
                crafted,
                tiers: vec![],
                elevated: None,
            });
            if crafted {
                fam.tiers.push(i as ModIdx);
                continue;
            }
            if m.elevated {
                fam.elevated = Some(i as ModIdx);
            } else if m.spawn.iter().any(|(_, w)| *w > 0)
                || m.essence_only
                || m.domain == "unveiled"
            {
                fam.tiers.push(i as ModIdx);
            }
        }
        // Elevated orphans whose "type" differs from their base tiers: attach to the single
        // family with the same side/group/influence that has tiers but no elevated version.
        let mut overrides: FxHashMap<ModIdx, FamilyKey> = FxHashMap::default();
        let keys: Vec<FamilyKey> = fams.keys().cloned().collect();
        for key in keys {
            let (has_tiers, elev) = {
                let f = &fams[&key];
                (!f.tiers.is_empty(), f.elevated)
            };
            let Some(elev) = elev else { continue };
            if has_tiers {
                continue;
            }
            let matches: Vec<FamilyKey> = fams
                .iter()
                .filter(|(k, f)| {
                    k.gen == key.gen
                        && k.group == key.group
                        && k.influence == key.influence
                        && !f.tiers.is_empty()
                        && f.elevated.is_none()
                })
                .map(|(k, _)| k.clone())
                .collect();
            if matches.len() == 1 {
                fams.get_mut(&matches[0]).unwrap().elevated = Some(elev);
                overrides.insert(elev, matches[0].clone());
                fams.remove(&key);
            }
        }
        for f in fams.values_mut() {
            let mods = &self.mods;
            f.tiers.sort_by(|a, b| {
                mods[*b as usize]
                    .level
                    .cmp(&mods[*a as usize].level)
                    .then(mods[*a as usize].id.cmp(&mods[*b as usize].id))
            });
        }
        self.families = fams.into_values().collect();
        self.family_index = self
            .families
            .iter()
            .enumerate()
            .map(|(i, f)| (f.key.clone(), i as FamIdx))
            .collect();
        self.family_of = vec![NO_FAMILY; self.mods.len()];
        for (i, m) in self.mods.iter().enumerate() {
            let key = overrides
                .get(&(i as ModIdx))
                .cloned()
                .unwrap_or_else(|| self.raw_family_key(m));
            if let Some(&fi) = self.family_index.get(&key) {
                self.family_of[i] = fi;
            }
        }
    }

    // -------------------------------------------------------------- lookups
    pub fn m(&self, i: ModIdx) -> &Mod {
        &self.mods[i as usize]
    }
    pub fn base(&self, i: BaseIdx) -> &Base {
        &self.bases[i as usize]
    }
    pub fn mod_idx(&self, id: &str) -> Option<ModIdx> {
        self.mod_index.get(id).copied()
    }
    pub fn family(&self, mi: ModIdx) -> Option<&Family> {
        let f = self.family_of[mi as usize];
        (f != NO_FAMILY).then(|| &self.families[f as usize])
    }
    pub fn family_idx(&self, mi: ModIdx) -> FamIdx {
        self.family_of[mi as usize]
    }

    pub fn tag_idx(&self, t: &str) -> Option<TagIdx> {
        self.tag_index.get(t).copied()
    }

    /// The bench option of this meta kind available for an item class.
    pub fn metamod_for(&self, cls: &str, kind: MetaKind) -> Option<&BenchOption> {
        self.bench
            .iter()
            .find(|b| self.m(b.module).meta == Some(kind) && b.classes.iter().any(|c| c == cls))
    }

    pub fn item_class(&self, b: BaseIdx) -> &str {
        &self.bases[b as usize].item_class
    }

    pub fn influence_tag(&self, b: BaseIdx, inf: Influence) -> Result<TagIdx, DataError> {
        let cls = self.item_class(b);
        let suffix = format!("_{}", inf.tag_suffix());
        self.class_influence_tags
            .get(cls)
            .and_then(|v| {
                v.iter()
                    .copied()
                    .find(|t| self.tags[*t as usize].ends_with(&suffix))
            })
            .ok_or_else(|| DataError(format!("{cls} items cannot have {} influence", inf.name())))
    }

    /// Spawn weight of a mod on a base counting every influence its class can have
    /// (first matching spawn tag wins: RePoE docs/mods.md).
    pub fn weight_on(&self, mi: ModIdx, b: BaseIdx) -> u32 {
        let base = self.base(b);
        let inf = self.class_influence_tags.get(&base.item_class);
        for &(t, w) in &self.m(mi).spawn {
            if base.tags.contains(&t) || inf.is_some_and(|v| v.contains(&t)) {
                return w;
            }
        }
        0
    }

    /// The family's natural tiers that can appear on this base, best first.
    pub fn tiers_for(&self, fam: FamIdx, b: BaseIdx) -> std::sync::Arc<Vec<ModIdx>> {
        if let Some(v) = self.tiers_cache.lock().unwrap().get(&(fam, b)) {
            return v.clone();
        }
        let base = self.base(b);
        let cls = &base.item_class;
        let ess: FxHashSet<&str> = self
            .essences
            .iter()
            .filter_map(|e| e.mods.get(cls).map(|s| s.as_str()))
            .collect();
        let out: Vec<ModIdx> = self.families[fam as usize]
            .tiers
            .iter()
            .copied()
            .filter(|&mi| {
                let m = self.m(mi);
                ((m.domain == base.domain || m.domain == "unveiled") && self.weight_on(mi, b) > 0)
                    || (m.essence_only && ess.contains(m.id.as_str()))
            })
            .collect();
        let arc = std::sync::Arc::new(out);
        self.tiers_cache
            .lock()
            .unwrap()
            .insert((fam, b), arc.clone());
        arc
    }

    /// 1 = best tier on this base, 0 = elevated. Crafted / family-less mods count as 1.
    pub fn tier(&self, mi: ModIdx, b: BaseIdx) -> u32 {
        let m = self.m(mi);
        let fi = self.family_of[mi as usize];
        if m.crafted || fi == NO_FAMILY || self.families[fi as usize].crafted {
            return 1;
        }
        let fam = &self.families[fi as usize];
        if fam.elevated == Some(mi) {
            return 0;
        }
        let t = self.tiers_for(fi, b);
        if let Some(p) = t.iter().position(|&x| x == mi) {
            return p as u32 + 1;
        }
        fam.tiers
            .iter()
            .position(|&x| x == mi)
            .map(|p| p as u32 + 1)
            .unwrap_or(1)
    }

    /// All (mod, weight) that can naturally roll on this base with these influences at this
    /// item level, ignoring the mods already on the item. Weight = first matching spawn tag,
    /// times the first matching generation-weight multiplier (percent).
    pub fn spawn_pool(
        &self,
        b: BaseIdx,
        infl: Influences,
        ilvl: u32,
    ) -> std::sync::Arc<Vec<(ModIdx, u32)>> {
        let key = (b, infl, ilvl);
        if let Some(v) = self.pool_cache.lock().unwrap().get(&key) {
            return v.clone();
        }
        let base = self.base(b);
        let mut tags: FxHashSet<TagIdx> = base.tags.iter().copied().collect();
        for i in infl.iter() {
            if let Ok(t) = self.influence_tag(b, i) {
                tags.insert(t);
            }
        }
        let mut out = Vec::new();
        for (i, m) in self.mods.iter().enumerate() {
            if m.crafted
                || m.essence_only
                || m.elevated
                || m.level > ilvl
                || m.domain != base.domain
                || m.gen == Gen::Other
            {
                continue;
            }
            let mut w = 0u32;
            for &(t, tw) in &m.spawn {
                if tags.contains(&t) {
                    w = tw;
                    break;
                }
            }
            if w == 0 {
                continue;
            }
            for &(t, gw) in &m.genw {
                if tags.contains(&t) {
                    w = w * gw / 100;
                    break;
                }
            }
            if w > 0 {
                out.push((i as ModIdx, w));
            }
        }
        out.sort_by(|a, b| self.mods[a.0 as usize].id.cmp(&self.mods[b.0 as usize].id));
        let arc = std::sync::Arc::new(out);
        self.pool_cache.lock().unwrap().insert(key, arc.clone());
        arc
    }

    /// Unveiled mods that can appear when unveiling a veiled affix of side `gen` here.
    pub fn unveil_pool(
        &self,
        b: BaseIdx,
        infl: Influences,
        ilvl: u32,
        gen: Gen,
    ) -> Vec<(ModIdx, u32)> {
        let base = self.base(b);
        let mut tags: FxHashSet<TagIdx> = base.tags.iter().copied().collect();
        for i in infl.iter() {
            if let Ok(t) = self.influence_tag(b, i) {
                tags.insert(t);
            }
        }
        let mut out = Vec::new();
        for (i, m) in self.mods.iter().enumerate() {
            if m.domain != "unveiled" || m.gen != gen || m.level > ilvl {
                continue;
            }
            let w = m
                .spawn
                .iter()
                .find(|(t, _)| tags.contains(t))
                .map(|x| x.1)
                .unwrap_or(0);
            if w > 0 {
                out.push((i as ModIdx, w));
            }
        }
        out.sort_by(|a, b| self.mods[a.0 as usize].id.cmp(&self.mods[b.0 as usize].id));
        out
    }

    // -------------------------------------------------------------- names
    pub fn resolve_base(&self, name: &str) -> Result<BaseIdx, DataError> {
        if let Some(&b) = self.base_index.get(name) {
            return Ok(b);
        }
        let low = name.to_lowercase();
        let mut hits: Vec<&Base> = self
            .bases
            .iter()
            .filter(|b| b.name.to_lowercase() == low)
            .collect();
        if hits.is_empty() {
            let mut close: Vec<&str> = self
                .bases
                .iter()
                .filter(|b| b.name.to_lowercase().contains(&low))
                .map(|b| b.name.as_str())
                .collect();
            close.sort();
            close.dedup();
            let hint = if close.is_empty() {
                String::new()
            } else {
                format!(
                    " Did you mean one of: {}?",
                    close
                        .iter()
                        .take(10)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            return Err(DataError(format!("Unknown base \"{name}\".{hint}")));
        }
        // several ids can share a name (e.g. Two-Toned Boots variants): first in id order
        hits.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(self.base_index[&hits[0].id])
    }

    pub fn class_tags(&self, cls: &str) -> FxHashSet<TagIdx> {
        let mut s: FxHashSet<TagIdx> = FxHashSet::default();
        for b in &self.bases {
            if b.item_class == cls {
                s.extend(b.tags.iter().copied());
            }
        }
        if let Some(v) = self.class_influence_tags.get(cls) {
            s.extend(v.iter().copied());
        }
        if let Some(&d) = self.tag_index.get("default") {
            s.remove(&d);
        }
        s
    }

    fn can_spawn_on_class(&self, mi: ModIdx, tags: &FxHashSet<TagIdx>) -> bool {
        self.m(mi).spawn.iter().any(|(t, _)| tags.contains(t))
    }

    /// Resolve a readable mod reference to a family. Accepted forms (case-insensitive):
    /// an alias from data/aliases.json; a RePoE mod id; "<Influence>: <text>";
    /// "prefix: <text>" / "suffix: <text>"; any fragment of the in-game text.
    pub fn resolve_mod(
        &self,
        reference: &str,
        item_class: Option<&str>,
    ) -> Result<FamIdx, DataError> {
        let mut r = reference.trim().to_string();
        if let Some(a) = self.aliases.get(&r.to_lowercase()) {
            r = a.clone();
        }
        if let Some(&mi) = self.mod_index.get(&r) {
            let f = self.family_of[mi as usize];
            if f != NO_FAMILY {
                return Ok(f);
            }
        }
        let low = r.to_lowercase();
        let mut text = low.as_str();
        let mut influence: Option<String> = None;
        let mut explicit_none = false;
        let mut gen: Option<Gen> = None;
        if let Some((head, rest)) = low.split_once(':') {
            let head = head.trim();
            if Influence::parse(head).is_some() || head == "normal" || head == "none" {
                if head == "normal" || head == "none" {
                    explicit_none = true;
                } else {
                    influence = Some(head.to_string());
                }
                text = rest.trim_start();
            }
        }
        let text_owned;
        if let Some((head, rest)) = text.split_once(':') {
            match head.trim() {
                "prefix" => {
                    gen = Some(Gen::Prefix);
                    text_owned = rest.trim_start().to_string();
                    text = &text_owned;
                }
                "suffix" => {
                    gen = Some(Gen::Suffix);
                    text_owned = rest.trim_start().to_string();
                    text = &text_owned;
                }
                _ => {}
            }
        }
        let class_tags = item_class.map(|c| self.class_tags(c));
        let mut cands: Vec<(FamIdx, ModIdx)> = Vec::new();
        for (fi, fam) in self.families.iter().enumerate() {
            if fam.crafted {
                continue;
            }
            if gen.is_some_and(|g| g != fam.key.gen) {
                continue;
            }
            if let Some(inf) = &influence {
                if &fam.key.influence != inf {
                    continue;
                }
            }
            if explicit_none && !fam.key.influence.is_empty() {
                continue;
            }
            let ids: Vec<ModIdx> = fam.tiers.iter().copied().chain(fam.elevated).collect();
            if let Some(ct) = &class_tags {
                if !ids.iter().any(|&i| self.can_spawn_on_class(i, ct)) {
                    continue;
                }
            }
            if let Some(&hit) = ids.iter().find(|&&i| {
                self.m(i).text.to_lowercase().contains(text) || self.m(i).id.to_lowercase() == text
            }) {
                cands.push((fi as FamIdx, hit));
            }
        }
        let where_ = item_class.map(|c| format!(" on {c}")).unwrap_or_default();
        match cands.len() {
            1 => Ok(cands[0].0),
            0 => Err(DataError(format!(
                "No mod matches \"{reference}\"{where_}. Use the in-game text (e.g. \"Hunter: additional Curse\"), prefix it with an influence (\"Warlord: ...\") or side (\"suffix: ...\"), or run `poecraft mods <base> --search \"<text>\"` to list mods."
            ))),
            n => {
                let mut lines: Vec<String> = cands
                    .iter()
                    .map(|(fi, mi)| {
                        let k = &self.families[*fi as usize].key;
                        let inf = if k.influence.is_empty() { "normal".to_string() } else { k.influence.clone() };
                        format!("    - {} {}: \"{}\"  (e.g. id {})", title(&inf), k.gen.name(), self.m(*mi).short(), self.m(*mi).id)
                    })
                    .collect();
                lines.sort();
                let more = if lines.len() > 15 { "\n    ..." } else { "" };
                Err(DataError(format!(
                    "Mod reference \"{reference}\"{where_} is ambiguous; it matches {n} mod families:\n{}{more}\n  Disambiguate with an influence (\"Hunter: ...\") or side (\"prefix: ...\") or more text.",
                    lines.into_iter().take(15).collect::<Vec<_>>().join("\n")
                )))
            }
        }
    }

    pub fn family_label(&self, fi: FamIdx) -> String {
        let f = &self.families[fi as usize];
        let ex = f
            .tiers
            .first()
            .copied()
            .or(f.elevated)
            .map(|m| self.m(m).short())
            .unwrap_or_default();
        let inf = if f.key.influence.is_empty() {
            "normal".to_string()
        } else {
            f.key.influence.clone()
        };
        format!("{} {} \"{}\"", title(&inf), f.key.gen.name(), ex)
    }
}

pub fn title(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn load_aliases(dir: &Path) -> FxHashMap<String, String> {
    let p = dir.join("aliases.json");
    let Ok(txt) = std::fs::read_to_string(&p) else {
        return FxHashMap::default();
    };
    let v: BTreeMap<String, String> = serde_json::from_str(&txt).unwrap_or_default();
    v.into_iter()
        .filter(|(k, _)| !k.starts_with('_'))
        .map(|(k, v)| (k.to_lowercase(), v))
        .collect()
}

pub fn default_data_dir() -> Result<PathBuf, DataError> {
    if let Ok(p) = std::env::var("POECRAFT_DATA") {
        return Ok(PathBuf::from(p));
    }
    let mut starts = vec![];
    if let Ok(c) = std::env::current_dir() {
        starts.push(c);
    }
    if let Ok(e) = std::env::current_exe() {
        starts.push(e);
    }
    for s in starts {
        for anc in s.ancestors() {
            let d = anc.join("data");
            if d.join("gamedata.json").exists() {
                return Ok(d);
            }
        }
    }
    Err(DataError(
        "cannot find data/gamedata.json (set POECRAFT_DATA to the data folder)".into(),
    ))
}

static DEFAULT: OnceLock<GameData> = OnceLock::new();

/// Shared default game data (loaded once).
pub fn default_data() -> &'static GameData {
    DEFAULT.get_or_init(|| GameData::load_default().expect("load game data"))
}
