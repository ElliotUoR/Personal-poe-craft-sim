//! Every crafting rule lives in this file (port of `poecraft/mechanics.py`).
//!
//! Each rule names its entry in `rules.rs` (source + verification status). Rules take a
//! `Chooser` for every random decision, so the same code drives Monte Carlo sampling
//! (`RandomChooser`) and exact enumeration (the replaying chooser in `engine.rs`).

use crate::data::{FamIdx, GameData, Gen, Influence, MetaKind, ModIdx, TagIdx};
use crate::item::{Item, Lock, Rarity, Roll};
use crate::rules::{RuleUse, Switches};
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub struct CraftError(pub String);
impl std::fmt::Display for CraftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for CraftError {}

pub type CResult<T> = Result<T, CraftError>;

fn require(cond: bool, msg: impl FnOnce() -> String) -> CResult<()> {
    if cond {
        Ok(())
    } else {
        Err(CraftError(msg()))
    }
}

// ------------------------------------------------------------------ choosers
pub trait Chooser {
    /// Index chosen with probability w[i] / sum(w).
    fn weighted(&mut self, w: &[u32]) -> usize;
    /// Index chosen uniformly from 0..n.
    fn uniform(&mut self, n: usize) -> usize;
}

pub struct RandomChooser {
    pub rng: SmallRng,
}

impl RandomChooser {
    pub fn new(seed: u64) -> Self {
        RandomChooser { rng: SmallRng::seed_from_u64(seed) }
    }
}

impl Chooser for RandomChooser {
    fn weighted(&mut self, w: &[u32]) -> usize {
        let total: u64 = w.iter().map(|&x| x as u64).sum();
        let mut x = self.rng.random_range(0..total);
        for (i, &wi) in w.iter().enumerate() {
            if x < wi as u64 {
                return i;
            }
            x -= wi as u64;
        }
        w.len() - 1
    }
    fn uniform(&mut self, n: usize) -> usize {
        self.rng.random_range(0..n)
    }
}

// ------------------------------------------------------------------ constants
/// rule rare_mod_count (UNVERIFIED): 4/5/6 mods with weights 8/3/1.
pub const RARE_MOD_COUNT: [(usize, u32); 3] = [(4, 8), (5, 3), (6, 1)];
/// rule magic_mod_count (UNVERIFIED): 1 or 2 affixes, equal chance.
pub const MAGIC_MOD_COUNT: [(usize, u32); 2] = [(1, 1), (2, 1)];

pub const ARMOUR_CLASSES: [&str; 4] = ["Body Armour", "Boots", "Gloves", "Helmet"];

/// Harvest reforge costs (rule reforge_costs). Source: PoEDB Horticrafting.
pub const HARVEST_REFORGE: &[(&str, &str, f64)] = &[
    ("fire", "lifeforce_wild", 50.0),
    ("cold", "lifeforce_vivid", 50.0),
    ("lightning", "lifeforce_primal", 50.0),
    ("physical", "lifeforce_vivid", 50.0),
    ("life", "lifeforce_wild", 75.0),
    ("defences", "lifeforce_primal", 75.0),
    ("chaos", "lifeforce_vivid", 100.0),
    ("attack", "lifeforce_wild", 75.0),
    ("caster", "lifeforce_primal", 75.0),
    ("speed", "lifeforce_vivid", 150.0),
    ("critical", "lifeforce_primal", 150.0),
    ("minion", "lifeforce_primal", 200.0),
    ("elemental", "lifeforce_wild", 200.0),
    ("attribute", "lifeforce_vivid", 200.0),
    ("mana", "lifeforce_primal", 200.0),
];

pub fn harvest_tag(tag: &str) -> &str {
    if tag == "defence" {
        "defences"
    } else {
        tag
    }
}

pub fn harvest_cost(tag: &str) -> Option<(&'static str, f64)> {
    let t = harvest_tag(tag);
    HARVEST_REFORGE.iter().find(|x| x.0 == t).map(|x| (x.1, x.2))
}

/// Beastcrafts: recipe -> (beasts consumed, number of "any rare beast", description).
/// Source: PoEDB Bestiary_Recipe (fetched 2026-11-18).
pub const BEASTCRAFTS: &[(&str, &[&str], u32, &str)] = &[
    ("metamod", &["Wild Bristle Matron"], 3, "Add a crafted Meta-modifier to a non-Unique Item"),
    ("imprint", &["Craicic Croaker"], 3, "Create an Imprint: Of a Magic Item"),
    ("hinekoras_lock", &["Craicic Croaker", "Black Mórrigan"], 2, "Apply a Hinekora's Lock: To a Magic Item"),
    ("max_links", &["Craicic Sand Spitter", "Black Mórrigan"], 2, "Modify an Item: to Have Maximum Possible Links"),
    ("max_sockets", &["Craicic Shield Crab", "Black Mórrigan"], 2, "Modify an Item: to Have Maximum Possible Number of Sockets"),
    ("six_link_rare", &["Saqawine Vulture"], 3, "Create an Item: Fully-linked Six-socket Rare"),
    ("add_prefix_remove_suffix", &["Farric Wolf Alpha"], 3, "Add a Prefix, Remove a Random Suffix (rare only)"),
    ("add_suffix_remove_prefix", &["Farric Lynx Alpha"], 3, "Add a Suffix, Remove a Random Prefix (rare only)"),
    ("add_hunter_mod", &["Craicic Watcher", "Craicic Maw"], 2, "Add a Mod to a Hunter Item"),
    ("add_warlord_mod", &["Fenumal Scrabbler", "Craicic Maw"], 2, "Add a Mod to a Warlord Item"),
    ("add_redeemer_mod", &["Fenumal Queen", "Craicic Maw"], 2, "Add a Mod to a Redeemer Item"),
];

pub type Cost = Vec<(String, f64)>;

pub fn beast_cost(name: &str) -> Cost {
    let (_, beasts, any, _) = BEASTCRAFTS.iter().find(|b| b.0 == name).expect("known beastcraft");
    let mut c: Cost = beasts.iter().map(|b| (format!("beast:{b}"), 1.0)).collect();
    if *any > 0 {
        c.push(("any_rare_beast".into(), *any as f64));
    }
    c
}

// ------------------------------------------------------------------ pools
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PoolOpts {
    pub prefix: bool,
    pub suffix: bool,
    pub influence_only: Option<Influence>,
    pub tag: Option<TagIdx>,
}

impl PoolOpts {
    pub fn both() -> Self {
        PoolOpts { prefix: true, suffix: true, influence_only: None, tag: None }
    }
    pub fn side(g: Gen) -> Self {
        PoolOpts { prefix: g == Gen::Prefix, suffix: g == Gen::Suffix, influence_only: None, tag: None }
    }
    fn allows(&self, g: Gen) -> bool {
        match g {
            Gen::Prefix => self.prefix,
            Gen::Suffix => self.suffix,
            Gen::Other => false,
        }
    }
}

#[derive(Hash, PartialEq, Eq)]
struct PoolKey {
    base: u32,
    infl: u8,
    ilvl: u32,
    groups: Vec<u32>,
    open_p: bool,
    open_s: bool,
    opts: PoolOpts,
    no_attack: bool,
    no_caster: bool,
}

pub struct Pool {
    pub mods: Vec<ModIdx>,
    pub weights: Vec<u32>,
}

/// The rule engine for one thread: game data, rule switches, a pool cache and a record
/// of which rules were used.
pub struct Mech<'a> {
    pub gd: &'a GameData,
    pub sw: Switches,
    cache: RefCell<FxHashMap<PoolKey, Rc<Pool>>>,
    pub used: RefCell<RuleUse>,
    tag_attack: Option<TagIdx>,
    tag_caster: Option<TagIdx>,
    veiled_prefix: Option<ModIdx>,
    veiled_suffix: Option<ModIdx>,
}

impl<'a> Mech<'a> {
    pub fn new(gd: &'a GameData, sw: Switches) -> Mech<'a> {
        Mech {
            gd,
            sw,
            cache: RefCell::new(FxHashMap::default()),
            used: RefCell::new(RuleUse::default()),
            tag_attack: gd.tag_idx("attack"),
            tag_caster: gd.tag_idx("caster"),
            veiled_prefix: gd.mod_idx("VeiledPrefix"),
            veiled_suffix: gd.mod_idx("VeiledSuffix"),
        }
    }

    fn rule(&self, id: &'static str) {
        self.used.borrow_mut().used(id);
    }

    /// Mods that can be added to `item` right now with their weights (rule mod_weights).
    pub fn pool(&self, item: &Item, opts: PoolOpts) -> Rc<Pool> {
        let gd = self.gd;
        let groups = item.group_ids(gd);
        let open_p = item.open_slots(gd, Gen::Prefix) > 0;
        let open_s = item.open_slots(gd, Gen::Suffix) > 0;
        let no_attack = item.meta(gd, MetaKind::NoAttackMods);
        let no_caster = item.meta(gd, MetaKind::NoCasterMods);
        let key = PoolKey { base: item.base, infl: item.influences.0, ilvl: item.ilvl, groups, open_p, open_s, opts, no_attack, no_caster };
        if let Some(p) = self.cache.borrow().get(&key) {
            return p.clone();
        }
        let mut mods = Vec::new();
        let mut weights = Vec::new();
        for &(mi, w) in gd.spawn_pool(item.base, item.influences, item.ilvl).iter() {
            let m = gd.m(mi);
            if !opts.allows(m.gen) || (m.gen == Gen::Prefix && !open_p) || (m.gen == Gen::Suffix && !open_s) {
                continue;
            }
            if let Some(i) = opts.influence_only {
                if m.influence != Some(i) {
                    continue;
                }
            }
            if let Some(t) = opts.tag {
                if !m.tag_ids.contains(&t) {
                    continue;
                }
            }
            if (no_attack && self.tag_attack.is_some_and(|t| m.tag_ids.contains(&t)))
                || (no_caster && self.tag_caster.is_some_and(|t| m.tag_ids.contains(&t)))
            {
                continue;
            }
            if m.group_ids.iter().any(|g| key.groups.binary_search(g).is_ok()) {
                continue;
            }
            mods.push(mi);
            weights.push(w);
        }
        if no_attack || no_caster {
            self.rule("no_attack_caster");
        }
        let p = Rc::new(Pool { mods, weights });
        let mut c = self.cache.borrow_mut();
        if c.len() > 200_000 {
            c.clear();
        }
        c.insert(key, p.clone());
        p
    }

    pub fn add_random_mod(&self, item: &Item, ch: &mut dyn Chooser, opts: PoolOpts) -> Option<Item> {
        let p = self.pool(item, opts);
        if p.mods.is_empty() {
            return None;
        }
        self.rule("mod_weights");
        let i = ch.weighted(&p.weights);
        Some(item.with_added(Roll::new(p.mods[i])))
    }

    /// Add random mods until the item has `n_total` mods (or nothing can roll).
    pub fn fill_random(&self, mut item: Item, ch: &mut dyn Chooser, n_total: usize, opts: PoolOpts) -> Item {
        while item.mods.len() < n_total {
            match self.add_random_mod(&item, ch, opts) {
                Some(n) => item = n,
                None => break,
            }
        }
        item
    }

    fn count(&self, ch: &mut dyn Chooser, table: &[(usize, u32)], rule: &'static str) -> usize {
        self.rule(rule);
        let w: Vec<u32> = table.iter().map(|x| x.1).collect();
        table[ch.weighted(&w)].0
    }

    /// Which mods are protected from removal/reroll (aligned with `item.mods`).
    /// Fractured mods always; a full lock protects every non-metamod of its side; a partial
    /// "N ... cannot be changed" lock protects N of them (rule partial_lock_pick when the
    /// side has more than N: chosen uniformly at random).
    pub fn protected(&self, item: &Item, ch: &mut dyn Chooser) -> Vec<bool> {
        let gd = self.gd;
        let mut prot: Vec<bool> = item.mods.iter().map(|r| r.fractured).collect();
        for gen in [Gen::Prefix, Gen::Suffix] {
            let lock = item.lock_on(gd, gen);
            if lock == Lock::None {
                continue;
            }
            let cands: Vec<usize> = (0..item.mods.len())
                .filter(|&i| gd.m(item.mods[i].m).gen == gen && !Item::is_metamod(gd, item.mods[i].m) && !prot[i])
                .collect();
            match lock {
                Lock::Full => {
                    self.rule("annul_protected");
                    for i in cands {
                        prot[i] = true;
                    }
                }
                Lock::Count(n) => {
                    self.rule("partial_lock");
                    if cands.len() <= n as usize {
                        for i in cands {
                            prot[i] = true;
                        }
                    } else {
                        self.rule("partial_lock_pick");
                        let mut left = cands;
                        for _ in 0..n {
                            let k = ch.uniform(left.len());
                            prot[left.remove(k)] = true;
                        }
                    }
                }
                Lock::None => {}
            }
        }
        prot
    }

    fn kept(&self, item: &Item, prot: &[bool], drop_metamods: bool) -> Vec<Roll> {
        item.mods
            .iter()
            .zip(prot)
            .filter(|(r, p)| **p && !(drop_metamods && Item::is_metamod(self.gd, r.m)))
            .map(|(r, _)| *r)
            .collect()
    }

    // -------------------------------------------------------------- basic currency
    pub fn transmute(&self, item: &Item, ch: &mut dyn Chooser) -> CResult<Item> {
        require(item.rarity == Rarity::Normal, || format!("Orb of Transmutation needs a normal item (item is {}).", item.rarity.name()))?;
        let it = Item { rarity: Rarity::Magic, preview_lock: false, ..item.clone() };
        let n = item.mods.len() + self.count(ch, &MAGIC_MOD_COUNT, "magic_mod_count");
        Ok(self.fill_random(it, ch, n, PoolOpts::both()))
    }

    pub fn alteration(&self, item: &Item, ch: &mut dyn Chooser) -> CResult<Item> {
        require(item.rarity == Rarity::Magic, || format!("Orb of Alteration needs a magic item (item is {}).", item.rarity.name()))?;
        let keep: Vec<Roll> = item.mods.iter().filter(|r| r.fractured).copied().collect();
        let n = keep.len() + self.count(ch, &MAGIC_MOD_COUNT, "magic_mod_count");
        Ok(self.fill_random(item.with_mods(keep), ch, n, PoolOpts::both()))
    }

    pub fn augment(&self, item: &Item, ch: &mut dyn Chooser) -> CResult<Item> {
        require(item.rarity == Rarity::Magic, || "Orb of Augmentation needs a magic item.".into())?;
        require(item.mods.len() < 2, || "Orb of Augmentation needs a magic item with an open affix.".into())?;
        Ok(self.add_random_mod(item, ch, PoolOpts::both()).unwrap_or_else(|| item.clone()))
    }

    pub fn regal(&self, item: &Item, ch: &mut dyn Chooser) -> CResult<Item> {
        require(item.rarity == Rarity::Magic, || format!("Regal Orb needs a magic item (item is {}).", item.rarity.name()))?;
        let it = Item { rarity: Rarity::Rare, ..item.clone() };
        Ok(self.add_random_mod(&it, ch, PoolOpts::both()).unwrap_or_else(|| it.with_mods(it.mods.clone())))
    }

    pub fn alchemy(&self, item: &Item, ch: &mut dyn Chooser) -> CResult<Item> {
        require(item.rarity == Rarity::Normal, || "Orb of Alchemy needs a normal item.".into())?;
        let it = Item { rarity: Rarity::Rare, preview_lock: false, ..item.clone() };
        let n = item.mods.len() + self.count(ch, &RARE_MOD_COUNT, "rare_mod_count");
        Ok(self.fill_random(it, ch, n, PoolOpts::both()))
    }

    /// Chaos Orb: reroll a rare's mods, keeping fractured and protected mods (rule chaos_keeps_locked).
    pub fn chaos(&self, item: &Item, ch: &mut dyn Chooser) -> CResult<Item> {
        require(item.rarity == Rarity::Rare, || format!("Chaos Orb needs a rare item (item is {}).", item.rarity.name()))?;
        self.rule("chaos_keeps_locked");
        let prot = self.protected(item, ch);
        let keep = self.kept(item, &prot, false);
        let n = (keep.len() + 1).max(self.count(ch, &RARE_MOD_COUNT, "rare_mod_count"));
        Ok(self.fill_random(item.with_mods(keep), ch, n, PoolOpts::both()))
    }

    pub fn exalt(&self, item: &Item, ch: &mut dyn Chooser) -> CResult<Item> {
        let gd = self.gd;
        require(item.rarity == Rarity::Rare, || "Exalted Orb needs a rare item.".into())?;
        require(
            item.mods.len() < 6 && (item.open_slots(gd, Gen::Prefix) > 0 || item.open_slots(gd, Gen::Suffix) > 0),
            || "Exalted Orb needs a rare item with an open prefix or suffix.".into(),
        )?;
        self.add_random_mod(item, ch, PoolOpts::both()).ok_or_else(|| CraftError("No modifier can roll on this item (all eligible groups taken).".into()))
    }

    /// Orb of Annulment (rule annul_protected).
    pub fn annul(&self, item: &Item, ch: &mut dyn Chooser) -> CResult<Item> {
        require(matches!(item.rarity, Rarity::Magic | Rarity::Rare), || "Orb of Annulment needs a magic or rare item.".into())?;
        let prot = self.protected(item, ch);
        let cands: Vec<Roll> = item.mods.iter().zip(&prot).filter(|(_, p)| !**p).map(|(r, _)| *r).collect();
        require(!cands.is_empty(), || "Orb of Annulment: no removable modifier.".into())?;
        self.rule("annul_protected");
        let v = cands[ch.uniform(cands.len())];
        Ok(item.without(&v))
    }

    /// Orb of Scouring (rule scour_protected).
    pub fn scour(&self, item: &Item, ch: &mut dyn Chooser) -> CResult<Item> {
        self.rule("scour_protected");
        let prot = self.protected(item, ch);
        let keep = self.kept(item, &prot, true);
        let it = item.with_mods(keep);
        let (np, ns) = (it.n_side(self.gd, Gen::Prefix), it.n_side(self.gd, Gen::Suffix));
        let rarity = if it.mods.is_empty() {
            Rarity::Normal
        } else if np <= 1 && ns <= 1 {
            Rarity::Magic
        } else {
            Rarity::Rare
        };
        Ok(Item { rarity, ..it })
    }

    // -------------------------------------------------------------- influence
    /// Conqueror exalted orbs (rule conqueror_exalt).
    pub fn conqueror_exalt(&self, item: &Item, ch: &mut dyn Chooser, inf: Influence) -> CResult<Item> {
        require(inf.is_conqueror(), || format!("{} is not a conqueror influence.", inf.name()))?;
        let name = crate::data::title(inf.name());
        require(item.rarity == Rarity::Rare, || format!("{name}'s Exalted Orb needs a rare item."))?;
        require(item.influences.is_empty(), || format!("{name}'s Exalted Orb needs an item with no influence."))?;
        self.gd.influence_tag(item.base, inf).map_err(|e| CraftError(e.0))?;
        self.rule("conqueror_exalt");
        let it = Item { influences: item.influences.with(inf), ..item.clone() };
        let opts = PoolOpts { influence_only: Some(inf), ..PoolOpts::both() };
        self.add_random_mod(&it, ch, opts).ok_or_else(|| CraftError(format!("No {name} modifier can roll (check open prefix/suffix slots).")))
    }

    /// Orb of Dominance (rules dominance, dominance_uniform).
    pub fn dominance(&self, item: &Item, ch: &mut dyn Chooser) -> CResult<Item> {
        let gd = self.gd;
        require(ARMOUR_CLASSES.contains(&gd.item_class(item.base)), || "Orb of Dominance only works on body armours, boots, gloves and helmets.".into())?;
        require(item.rarity == Rarity::Rare, || "Orb of Dominance needs a rare item.".into())?;
        let infl = item.influenced_mods(gd);
        require(infl.len() >= 2, || format!("Orb of Dominance needs at least two influenced modifiers (item has {}).", infl.len()))?;
        let prot = self.protected(item, ch);
        let elig: Vec<Roll> = infl.iter().filter(|r| !item.mods.iter().zip(&prot).any(|(x, p)| x == *r && *p)).copied().collect();
        require(elig.len() >= 2, || "Orb of Dominance: fewer than two influenced modifiers can be changed (fractured or protected by a 'cannot be changed' metamod).".into())?;
        self.rule("dominance");
        self.rule("dominance_uniform");
        let ti = ch.uniform(elig.len());
        let target = elig[ti];
        let others: Vec<Roll> = elig.iter().enumerate().filter(|(i, _)| *i != ti).map(|(_, r)| *r).collect();
        let victim = others[ch.uniform(others.len())];
        let fam = gd.family(target.m).expect("influenced mods have families");
        let tiers = gd.tiers_for(gd.family_idx(target.m), item.base);
        let new_id = if fam.elevated == Some(target.m) {
            target.m // value reroll only
        } else {
            match tiers.iter().position(|&x| x == target.m) {
                Some(p) if p > 0 => tiers[p - 1],
                _ => fam.elevated.unwrap_or(target.m),
            }
        };
        let mut mods: Vec<Roll> = item.mods.iter().filter(|r| **r != target && **r != victim).copied().collect();
        mods.push(Roll { m: new_id, ..target });
        Ok(item.with_mods(mods))
    }

    /// Awakener's Orb: destroy `donor`, apply its influence to `target` (rules awakener,
    /// awakener_uniform_fill).
    pub fn awaken(&self, target: &Item, donor: &Item, ch: &mut dyn Chooser) -> CResult<Item> {
        let gd = self.gd;
        require(gd.item_class(target.base) == gd.item_class(donor.base), || "Awakener's Orb: both items must be of the same item class.".into())?;
        require(!target.influences.is_empty() && !donor.influences.is_empty(), || "Awakener's Orb: both items must be influenced.".into())?;
        require(!target.influences.intersects(donor.influences), || "Awakener's Orb: the items must have different influences.".into())?;
        self.rule("awakener");
        self.rule("awakener_uniform_fill");
        let mut kept: Vec<Roll> = Vec::new();
        for it in [donor, target] {
            let infl = it.influenced_mods(gd);
            if !infl.is_empty() {
                kept.push(infl[ch.uniform(infl.len())]);
            }
        }
        if kept.len() == 2 && gd.m(kept[0].m).group_ids.iter().any(|g| gd.m(kept[1].m).group_ids.contains(g)) {
            let k = kept[ch.uniform(2)];
            kept = vec![k];
        }
        let res = Item { rarity: Rarity::Rare, influences: target.influences.union(donor.influences), preview_lock: false, ..target.clone() };
        let res = res.with_mods(kept.iter().map(|r| Roll::new(r.m)).collect());
        let n = kept.len().max(self.count(ch, &RARE_MOD_COUNT, "rare_mod_count"));
        Ok(self.fill_random(res, ch, n, PoolOpts::both()))
    }

    // -------------------------------------------------------------- harvest
    /// Harvest "Reforge a Rare item with random modifiers, including a <Tag> modifier"
    /// (rules reforge_respects_lock, reforge_consumes_lock, reforge_fills_locked_side,
    /// reforge_mod_count).
    pub fn harvest_reforge(&self, item: &Item, ch: &mut dyn Chooser, tag: &str) -> CResult<Item> {
        let gd = self.gd;
        let tag = harvest_tag(tag);
        require(harvest_cost(tag).is_some(), || {
            format!("Unknown Harvest reforge tag '{tag}'. Known: {}.", HARVEST_REFORGE.iter().map(|x| x.0).collect::<Vec<_>>().join(", "))
        })?;
        require(item.rarity == Rarity::Rare, || "Harvest reforge needs a rare item.".into())?;
        let tag_idx = gd.tag_idx(tag).ok_or_else(|| CraftError(format!("no mod has the tag '{tag}'")))?;
        let prot = self.protected(item, ch);
        let keep = self.kept(item, &prot, true); // metamod consumed
        if item.lock_on(gd, Gen::Prefix) != Lock::None || item.lock_on(gd, Gen::Suffix) != Lock::None {
            self.rule("reforge_respects_lock");
            self.rule("reforge_consumes_lock");
            self.rule("reforge_fills_locked_side");
        }
        self.rule("reforge_mod_count");
        let base = item.with_mods(keep.clone());
        let (p_ok, s_ok) = if self.sw.reforge_fills_locked_side {
            (true, true)
        } else {
            (!item.side_closed_by_lock(gd, Gen::Prefix), !item.side_closed_by_lock(gd, Gen::Suffix))
        };
        let sides = PoolOpts { prefix: p_ok, suffix: s_ok, influence_only: None, tag: None };
        let first_pool = self.pool(&base, PoolOpts { tag: Some(tag_idx), ..sides });
        require(!first_pool.mods.is_empty(), || format!("Harvest reforge {tag}: no {tag} modifier can roll on the changeable side(s) of this item."))?;
        let first = first_pool.mods[ch.weighted(&first_pool.weights)];
        let mut mods = keep;
        mods.push(Roll::new(first));
        let it = base.with_mods(mods);
        let n = it.mods.len().max(self.count(ch, &RARE_MOD_COUNT, "rare_mod_count"));
        Ok(self.fill_random(it, ch, n, sides))
    }

    // -------------------------------------------------------------- essence
    pub fn essence(&self, item: &Item, ch: &mut dyn Chooser, essence: usize) -> CResult<Item> {
        let gd = self.gd;
        let e = &gd.essences[essence];
        let cls = gd.item_class(item.base);
        let mid = e.mods.get(cls).and_then(|id| gd.mod_idx(id)).ok_or_else(|| CraftError(format!("{} cannot be used on {cls}.", e.name)))?;
        require(matches!(item.rarity, Rarity::Normal | Rarity::Rare), || format!("{} needs a normal or rare item.", e.name))?;
        if let Some(lim) = e.item_level_restriction {
            require(item.ilvl <= lim, || format!("{} only works on items up to item level {lim}.", e.name))?;
        }
        let mut keep: Vec<Roll> = item.mods.iter().filter(|r| r.fractured).copied().collect();
        keep.push(Roll::new(mid));
        let it = Item { rarity: Rarity::Rare, ..item.clone() }.with_mods(keep);
        let n = it.mods.len().max(self.count(ch, &RARE_MOD_COUNT, "rare_mod_count"));
        Ok(self.fill_random(it, ch, n, PoolOpts::both()))
    }

    // -------------------------------------------------------------- bench
    /// Crafting bench (rule bench_metamod_slot).
    pub fn bench_craft(&self, item: &Item, mi: ModIdx) -> CResult<Item> {
        let gd = self.gd;
        let m = gd.m(mi);
        require(matches!(item.rarity, Rarity::Magic | Rarity::Rare), || "The crafting bench needs a magic or rare item.".into())?;
        require(item.open_slots(gd, m.gen) > 0, || format!("Bench craft '{}' needs an open {}.", m.short(), m.gen.name()))?;
        let limit = if item.meta(gd, MetaKind::Multimod) || m.meta == Some(MetaKind::Multimod) { 3 } else { 1 };
        require(item.crafted_count() < limit, || "Item already has a crafted modifier (craft 'multimod' first to allow up to 3).".into())?;
        let groups = item.group_ids(gd);
        require(!m.group_ids.iter().any(|g| groups.binary_search(g).is_ok()), || format!("Item already has a modifier of group {}.", m.groups.first().cloned().unwrap_or_default()))?;
        self.rule("bench_metamod_slot");
        Ok(item.with_added(Roll::crafted(mi)))
    }

    pub fn remove_crafted(&self, item: &Item) -> Item {
        item.with_mods(item.mods.iter().filter(|r| !r.crafted).copied().collect())
    }

    // -------------------------------------------------------------- beastcraft
    /// Metamod beastcraft (rules beast_metamod_magic_only, beast_metamod_choice).
    pub fn beast_metamod(&self, item: &Item, kind: MetaKind) -> CResult<Item> {
        require(item.rarity == Rarity::Magic, || {
            "the metamod beastcraft only works on magic items (in-game, owner); on a rare use the bench craft: craft lock_suffixes".into()
        })?;
        self.rule("beast_metamod_magic_only");
        self.rule("beast_metamod_choice");
        let cls = self.gd.item_class(item.base);
        let opt = self.gd.metamod_for(cls, kind).ok_or_else(|| CraftError(format!("no such metamod for {cls}")))?;
        self.bench_craft(item, opt.module)
    }

    /// "Add a Mod to a <Conqueror> Item" (rule beast_add_influence_mod).
    pub fn beast_add_influence_mod(&self, item: &Item, ch: &mut dyn Chooser, inf: Influence) -> CResult<Item> {
        require(item.rarity == Rarity::Rare, || "This beastcraft needs a rare item.".into())?;
        require(item.influences.contains(inf), || format!("Item has no {} influence.", crate::data::title(inf.name())))?;
        self.rule("beast_add_influence_mod");
        let opts = PoolOpts { influence_only: Some(inf), ..PoolOpts::both() };
        self.add_random_mod(item, ch, opts).ok_or_else(|| CraftError(format!("No {} modifier can be added (open slot?).", crate::data::title(inf.name()))))
    }

    /// "Add a Prefix, Remove a Random Suffix" and the reverse (rule beast_swap_side).
    pub fn beast_swap_side(&self, item: &Item, ch: &mut dyn Chooser, add: Gen) -> CResult<Item> {
        let gd = self.gd;
        require(item.rarity == Rarity::Rare, || "This beastcraft needs a rare item.".into())?;
        self.rule("beast_swap_side");
        let rem = add.other_side();
        let prot = self.protected(item, ch);
        let cands: Vec<Roll> = item.mods.iter().zip(&prot).filter(|(r, p)| !**p && gd.m(r.m).gen == rem).map(|(r, _)| *r).collect();
        require(!cands.is_empty(), || format!("No removable {}.", rem.name()))?;
        let v = cands[ch.uniform(cands.len())];
        let it = item.without(&v);
        self.add_random_mod(&it, ch, PoolOpts::side(add)).ok_or_else(|| CraftError(format!("No {} can roll.", add.name())))
    }

    pub fn hinekoras_lock(&self, item: &Item) -> CResult<Item> {
        require(item.rarity == Rarity::Magic, || "The Hinekora's Lock beastcraft needs a magic item.".into())?;
        Ok(Item { preview_lock: true, ..item.clone() })
    }

    /// Max-links beastcraft (rule max_links).
    pub fn max_links(&self, item: &Item) -> Item {
        self.rule("max_links");
        let n = if self.gd.item_class(item.base) == "Body Armour" { 6 } else { 4 };
        Item { links: n, ..item.clone() }
    }

    /// Omen of Blanching: STUB, cost only.
    pub fn omen_blanching(&self, item: &Item) -> Item {
        Item { white_sockets: item.white_sockets.max(2), ..item.clone() }
    }

    /// Fracturing Orb (rule fracture).
    pub fn fracture(&self, item: &Item, ch: &mut dyn Chooser) -> CResult<Item> {
        require(item.rarity == Rarity::Rare && item.mods.len() >= 4, || "Fracturing Orb needs a rare item with 4+ modifiers.".into())?;
        require(!item.mods.iter().any(|r| r.fractured), || "Item already has a fractured modifier.".into())?;
        self.rule("fracture");
        let cands: Vec<Roll> = item.mods.iter().filter(|r| !r.crafted).copied().collect();
        let t = cands[ch.uniform(cands.len())];
        let mut mods: Vec<Roll> = item.mods.iter().filter(|r| **r != t).copied().collect();
        mods.push(Roll { fractured: true, ..t });
        Ok(item.with_mods(mods))
    }

    /// Veiled Exalted Orb (rule veiled_exalt).
    pub fn veiled_exalt(&self, item: &Item, ch: &mut dyn Chooser) -> CResult<Item> {
        let gd = self.gd;
        require(item.rarity == Rarity::Rare, || "Veiled Exalted Orb needs a rare item.".into())?;
        self.rule("veiled_exalt");
        let prot = self.protected(item, ch);
        let cands: Vec<Roll> = item.mods.iter().zip(&prot).filter(|(_, p)| !**p).map(|(r, _)| *r).collect();
        require(!cands.is_empty(), || "Veiled Exalted Orb: no removable modifier.".into())?;
        let v = cands[ch.uniform(cands.len())];
        let it = item.without(&v);
        let sides: Vec<Gen> = [Gen::Prefix, Gen::Suffix].into_iter().filter(|g| it.open_slots(gd, *g) > 0).collect();
        let side = sides[ch.uniform(sides.len())];
        let vm = if side == Gen::Prefix { self.veiled_prefix } else { self.veiled_suffix }.ok_or_else(|| CraftError("veiled placeholder mods missing from data".into()))?;
        Ok(it.with_added(Roll::new(vm)))
    }

    pub const UNVEIL_OPTIONS: usize = 3;

    /// Unveil: the veiled affix becomes one of 3 offered unveiled mods; the player takes the
    /// first family in `prefs` that is offered, otherwise the first offer (rule veiled_exalt).
    pub fn unveil(&self, item: &Item, ch: &mut dyn Chooser, prefs: &[FamIdx]) -> CResult<Item> {
        let gd = self.gd;
        let v = item
            .mods
            .iter()
            .find(|r| Some(r.m) == self.veiled_prefix || Some(r.m) == self.veiled_suffix)
            .copied()
            .ok_or_else(|| CraftError("unveil: the item has no veiled modifier.".into()))?;
        let gen = if Some(v.m) == self.veiled_prefix { Gen::Prefix } else { Gen::Suffix };
        let rest = item.without(&v);
        let present = rest.group_ids(gd);
        let mut pool: Vec<(ModIdx, u32)> = gd
            .unveil_pool(item.base, item.influences, item.ilvl, gen)
            .into_iter()
            .filter(|(m, _)| !gd.m(*m).group_ids.iter().any(|g| present.binary_search(g).is_ok()))
            .collect();
        require(!pool.is_empty(), || "unveil: no unveiled modifier can appear.".into())?;
        let mut offers = Vec::new();
        while !pool.is_empty() && offers.len() < Self::UNVEIL_OPTIONS {
            let w: Vec<u32> = pool.iter().map(|x| x.1).collect();
            let m = pool[ch.weighted(&w)].0;
            offers.push(m);
            let gs = gd.m(m).group_ids.clone();
            pool.retain(|(x, _)| !gd.m(*x).group_ids.iter().any(|g| gs.contains(g)));
        }
        let mut choice = offers[0];
        for &f in prefs {
            if let Some(&m) = offers.iter().find(|&&m| gd.family_idx(m) == f) {
                choice = m;
                break;
            }
        }
        Ok(rest.with_added(Roll::new(choice)))
    }

    /// Foulborn Exalted Orb (rule foulborn_exalt, UNVERIFIED model).
    pub fn foulborn_exalt(&self, item: &Item, ch: &mut dyn Chooser, side: Option<Gen>) -> CResult<Item> {
        require(item.rarity == Rarity::Rare, || "Foulborn Exalted Orb needs a rare item.".into())?;
        self.rule("foulborn_exalt");
        let opts = side.map(PoolOpts::side).unwrap_or_else(PoolOpts::both);
        self.add_random_mod(item, ch, opts).ok_or_else(|| CraftError(format!("Foulborn Exalted Orb: no open {}.", side.map(|g| g.name()).unwrap_or("affix"))))
    }
}
