//! Immutable, hashable item state (hashable so the exact engine can use it as a Markov state).

use crate::data::{title, BaseIdx, GameData, Gen, Influences, MetaKind, ModIdx};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Roll {
    pub m: ModIdx,
    pub fractured: bool,
    pub crafted: bool,
}

impl Roll {
    pub fn new(m: ModIdx) -> Roll {
        Roll { m, fractured: false, crafted: false }
    }
    pub fn crafted(m: ModIdx) -> Roll {
        Roll { m, fractured: false, crafted: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rarity {
    Normal,
    Magic,
    Rare,
}

impl Rarity {
    pub fn name(self) -> &'static str {
        match self {
            Rarity::Normal => "normal",
            Rarity::Magic => "magic",
            Rarity::Rare => "rare",
        }
    }
    pub fn parse(s: &str) -> Option<Rarity> {
        match s {
            "normal" => Some(Rarity::Normal),
            "magic" => Some(Rarity::Magic),
            "rare" => Some(Rarity::Rare),
            _ => None,
        }
    }
    pub fn max_affixes(self) -> usize {
        match self {
            Rarity::Normal => 0,
            Rarity::Magic => 1,
            Rarity::Rare => 3,
        }
    }
}

/// Protection a metamod gives one side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lock {
    None,
    Full,
    Count(u8),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Item {
    pub base: BaseIdx,
    pub ilvl: u32,
    pub rarity: Rarity,
    pub influences: Influences,
    /// Always sorted.
    pub mods: Vec<Roll>,
    /// Hinekora's Lock applied.
    pub preview_lock: bool,
    /// 6 after the max-links beastcraft; 0 = not tracked.
    pub links: u8,
    /// Omen of Blanching stub.
    pub white_sockets: u8,
    pub tag: Option<Arc<str>>,
}

impl Item {
    pub fn new(base: BaseIdx, ilvl: u32, rarity: Rarity, influences: Influences) -> Item {
        Item { base, ilvl, rarity, influences, mods: vec![], preview_lock: false, links: 0, white_sockets: 0, tag: None }
    }

    /// Copy with a new mod list (sorted); clears the Hinekora preview like any change.
    pub fn with_mods(&self, mut mods: Vec<Roll>) -> Item {
        mods.sort();
        Item { mods, preview_lock: false, ..self.clone() }
    }

    pub fn with_added(&self, r: Roll) -> Item {
        let mut v = self.mods.clone();
        v.push(r);
        self.with_mods(v)
    }

    pub fn without(&self, r: &Roll) -> Item {
        let mut v = self.mods.clone();
        if let Some(p) = v.iter().position(|x| x == r) {
            v.remove(p);
        }
        self.with_mods(v)
    }

    pub fn has_mod(&self, m: ModIdx) -> bool {
        self.mods.iter().any(|r| r.m == m)
    }

    pub fn side<'a>(&'a self, gd: &'a GameData, gen: Gen) -> impl Iterator<Item = &'a Roll> + 'a {
        self.mods.iter().filter(move |r| gd.m(r.m).gen == gen)
    }

    pub fn n_side(&self, gd: &GameData, gen: Gen) -> usize {
        self.side(gd, gen).count()
    }

    pub fn max_affixes(&self) -> usize {
        self.rarity.max_affixes()
    }

    pub fn open_slots(&self, gd: &GameData, gen: Gen) -> usize {
        self.max_affixes().saturating_sub(self.n_side(gd, gen))
    }

    pub fn meta(&self, gd: &GameData, kind: MetaKind) -> bool {
        self.mods.iter().any(|r| gd.m(r.m).meta == Some(kind))
    }

    pub fn is_metamod(gd: &GameData, m: ModIdx) -> bool {
        matches!(gd.m(m).meta, Some(MetaKind::LockPrefixes(_) | MetaKind::LockSuffixes(_) | MetaKind::Multimod))
    }

    /// How a metamod on the item protects `gen`.
    pub fn lock_on(&self, gd: &GameData, gen: Gen) -> Lock {
        let mut best = Lock::None;
        for r in &self.mods {
            let l = match (gd.m(r.m).meta, gen) {
                (Some(MetaKind::LockPrefixes(n)), Gen::Prefix) | (Some(MetaKind::LockSuffixes(n)), Gen::Suffix) => match n {
                    None => Lock::Full,
                    Some(n) => Lock::Count(n),
                },
                _ => continue,
            };
            best = match (best, l) {
                (Lock::Full, _) | (_, Lock::Full) => Lock::Full,
                (Lock::Count(a), Lock::Count(b)) => Lock::Count(a.max(b)),
                (Lock::None, x) => x,
                (x, Lock::None) => x,
            };
        }
        best
    }

    /// True when no new mod may be added to `gen` because a lock protects all of it.
    pub fn side_closed_by_lock(&self, gd: &GameData, gen: Gen) -> bool {
        match self.lock_on(gd, gen) {
            Lock::None => false,
            Lock::Full => true,
            // a partial lock that covers every mod of the side acts as a full lock
            Lock::Count(n) => self.side(gd, gen).filter(|r| !Item::is_metamod(gd, r.m)).count() <= n as usize,
        }
    }

    pub fn influenced_mods(&self, gd: &GameData) -> Vec<Roll> {
        self.mods.iter().filter(|r| gd.m(r.m).influence.is_some() && !r.crafted).copied().collect()
    }

    pub fn crafted_count(&self) -> usize {
        self.mods.iter().filter(|r| r.crafted).count()
    }

    pub fn group_ids(&self, gd: &GameData) -> Vec<u32> {
        let mut v: Vec<u32> = self.mods.iter().flat_map(|r| gd.m(r.m).group_ids.iter().copied()).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    pub fn describe(&self, gd: &GameData) -> String {
        let b = gd.base(self.base);
        let mut inf: Vec<String> = self.influences.iter().map(|i| title(i.name())).collect();
        inf.sort();
        let inf = if inf.is_empty() { "no influence".to_string() } else { inf.join("/") };
        let mut lines = vec![format!("{} {} (ilvl {}, {})", title(self.rarity.name()), b.name, self.ilvl, inf)];
        for gen in [Gen::Prefix, Gen::Suffix] {
            for r in self.side(gd, gen) {
                let m = gd.m(r.m);
                let t = gd.tier(r.m, self.base);
                let tier = if r.crafted {
                    "crafted".to_string()
                } else if t == 0 {
                    "ELEVATED".to_string()
                } else {
                    format!("T{t}")
                };
                let fr = if r.fractured { " [fractured]" } else { "" };
                let inf = m.influence.map(|i| format!("{} ", title(i.name()))).unwrap_or_default();
                lines.push(format!("  {} {inf}{tier}{fr}: {}", if gen == Gen::Prefix { 'P' } else { 'S' }, m.short()));
            }
        }
        if self.preview_lock {
            lines.push("  (Hinekora's Lock: next currency outcome can be foreseen)".into());
        }
        if self.links > 0 {
            lines.push(format!("  {}-linked", self.links));
        }
        lines.join("\n")
    }
}
