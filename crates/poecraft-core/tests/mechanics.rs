//! Mechanics tests: mirror tests/test_poecraft.py and add partial-lock cases.
use poecraft_core::data::{default_data, GameData, Gen, Influence, Influences, MetaKind, ModIdx};
use poecraft_core::enumerate::enumerate;
use poecraft_core::item::{Item, Rarity, Roll};
use poecraft_core::mechanics::{Chooser, Mech, RandomChooser};
use poecraft_core::rules::Switches;
use std::collections::HashMap;

fn gd() -> &'static GameData {
    default_data()
}

fn fam_tier(r: &str, cls: &str, tier: usize) -> ModIdx {
    let g = gd();
    let f = g.resolve_mod(r, Some(cls)).unwrap();
    g.families[f as usize].tiers[tier]
}
fn elevated(r: &str, cls: &str) -> ModIdx {
    let g = gd();
    let f = g.resolve_mod(r, Some(cls)).unwrap();
    g.families[f as usize].elevated.unwrap()
}
fn infl(list: &[Influence]) -> Influences {
    list.iter().fold(Influences::default(), |a, i| a.with(*i))
}
fn plate(mods: &[ModIdx], inf: &[Influence], rarity: Rarity) -> Item {
    let b = gd().resolve_base("Astral Plate").unwrap();
    Item::new(b, 86, rarity, infl(inf)).with_mods(mods.iter().map(|&m| Roll::new(m)).collect())
}
fn lock(item: &Item, kind: MetaKind) -> Item {
    let g = gd();
    let m = g.metamod_for(g.item_class(item.base), kind).unwrap().module;
    item.with_added(Roll::crafted(m))
}
fn outcomes(f: impl FnMut(&mut dyn Chooser) -> Item) -> HashMap<Item, f64> {
    let mut out = HashMap::new();
    let mut budget = 1_000_000;
    enumerate(f, &mut budget, |it, p| {
        *out.entry(it).or_insert(0.0) += p;
        true
    })
    .ok()
    .unwrap();
    out
}

#[test]
fn names_resolve_and_curse_is_only_caster_prefix() {
    let g = gd();
    let b = g.resolve_base("Astral Plate").unwrap();
    let pool = g.spawn_pool(b, infl(&[Influence::Hunter, Influence::Warlord]), 86);
    let caster: Vec<&str> = pool
        .iter()
        .filter(|(m, _)| g.m(*m).gen == Gen::Prefix && g.m(*m).has_tag("caster"))
        .map(|(m, _)| g.m(*m).id.as_str())
        .collect();
    assert_eq!(caster, vec!["AdditionalCurseOnEnemiesInfluence1"]);
    assert_eq!(g.m(elevated("Warlord: total mana cost", "Body Armour")).id, "SocketedAttacksManaCostInfluenceMaven");
}

#[test]
fn dominance_two_mods_is_fifty_fifty() {
    let mech = Mech::new(gd(), Switches::default());
    let crit = fam_tier("Hunter: Attacks have", "Body Armour", 0);
    let life = fam_tier("Hunter: max life %", "Body Armour", 0);
    let it = plate(&[crit, life], &[Influence::Hunter], Rarity::Rare);
    let out = outcomes(|ch| mech.dominance(&it, ch).unwrap());
    let el = elevated("Hunter: Attacks have", "Body Armour");
    let p: f64 = out.iter().filter(|(i, _)| i.has_mod(el)).map(|(_, p)| p).sum();
    assert!((p - 0.5).abs() < 1e-12, "p = {p}");
    assert!(out.keys().all(|i| i.influenced_mods(gd()).len() == 1));
}

#[test]
fn dominance_skips_metamod_protected_mods() {
    let mech = Mech::new(gd(), Switches::default());
    let crit = fam_tier("Hunter: Attacks have", "Body Armour", 0);
    let life = fam_tier("Hunter: max life %", "Body Armour", 0);
    let mana = fam_tier("Warlord: total mana cost", "Body Armour", 0);
    let it = lock(&plate(&[crit, mana, life], &[Influence::Hunter, Influence::Warlord], Rarity::Rare), MetaKind::LockPrefixes(None));
    for (o, _) in outcomes(|ch| mech.dominance(&it, ch).unwrap()) {
        assert!(o.has_mod(life));
    }
}

#[test]
fn reforge_respects_full_lock_consumes_it_and_guarantees_curse() {
    let g = gd();
    let mech = Mech::new(g, Switches::default());
    let crit = elevated("Hunter: Attacks have", "Body Armour");
    let mana = elevated("Warlord: total mana cost", "Body Armour");
    let it = lock(&plate(&[crit, mana], &[Influence::Hunter, Influence::Warlord], Rarity::Rare), MetaKind::LockSuffixes(None));
    let curse = g.mod_idx("AdditionalCurseOnEnemiesInfluence1").unwrap();
    let mut ch = RandomChooser::new(1);
    for _ in 0..300 {
        let r = mech.harvest_reforge(&it, &mut ch, "caster").unwrap();
        assert!(r.has_mod(crit) && r.has_mod(mana) && r.has_mod(curse));
        assert_eq!(r.crafted_count(), 0, "metamod consumed");
        assert_eq!(r.n_side(g, Gen::Suffix), 2, "no suffix added to the locked side");
    }
}

#[test]
fn scour_with_suffix_lock_leaves_magic_item() {
    let g = gd();
    let mech = Mech::new(g, Switches::default());
    let mana = elevated("Warlord: total mana cost", "Body Armour");
    let armour = g.mod_idx("LocalIncreasedPhysicalDamageReductionRatingPercent5").unwrap();
    let it = lock(&plate(&[mana, armour], &[Influence::Warlord], Rarity::Rare), MetaKind::LockSuffixes(None));
    let r = mech.scour(&it, &mut RandomChooser::new(0)).unwrap();
    assert_eq!(r.rarity, Rarity::Magic);
    assert_eq!(r.mods, vec![Roll::new(mana)]);
}

#[test]
fn awakener_keeps_one_influenced_mod_from_each() {
    let g = gd();
    let mech = Mech::new(g, Switches::default());
    let crit = elevated("Hunter: Attacks have", "Body Armour");
    let mana = elevated("Warlord: total mana cost", "Body Armour");
    let target = plate(&[mana], &[Influence::Warlord], Rarity::Magic);
    let donor = plate(&[crit], &[Influence::Hunter], Rarity::Rare);
    let mut ch = RandomChooser::new(2);
    for _ in 0..200 {
        let r = mech.awaken(&target, &donor, &mut ch).unwrap();
        assert!(r.has_mod(crit) && r.has_mod(mana) && r.rarity == Rarity::Rare);
        assert!((4..=6).contains(&r.mods.len()));
    }
}

#[test]
fn conqueror_exalt_requires_uninfluenced_rare() {
    let mech = Mech::new(gd(), Switches::default());
    assert!(mech.conqueror_exalt(&plate(&[], &[Influence::Hunter], Rarity::Rare), &mut RandomChooser::new(0), Influence::Warlord).is_err());
    let r = mech.conqueror_exalt(&plate(&[], &[], Rarity::Rare), &mut RandomChooser::new(0), Influence::Hunter).unwrap();
    assert!(r.influences.contains(Influence::Hunter));
    assert_eq!(gd().m(r.mods[0].m).influence, Some(Influence::Hunter));
}

#[test]
fn beast_metamod_only_on_magic() {
    let mech = Mech::new(gd(), Switches::default());
    let crit = elevated("Hunter: Attacks have", "Body Armour");
    assert!(mech.beast_metamod(&plate(&[crit], &[Influence::Hunter], Rarity::Rare), MetaKind::LockSuffixes(None)).is_err());
    let m = mech.beast_metamod(&plate(&[crit], &[Influence::Hunter], Rarity::Magic), MetaKind::LockSuffixes(None)).unwrap();
    assert!(m.side_closed_by_lock(gd(), Gen::Suffix));
}

// ---------------------------------------------------------------- partial locks (owner, 2026-11-18)
#[test]
fn partial_lock_covering_all_suffixes_acts_as_full_lock() {
    let g = gd();
    let mech = Mech::new(g, Switches::default());
    let crit = elevated("Hunter: Attacks have", "Body Armour");
    let mana = elevated("Warlord: total mana cost", "Body Armour");
    let it = lock(&plate(&[crit, mana], &[Influence::Hunter, Influence::Warlord], Rarity::Rare), MetaKind::LockSuffixes(Some(2)));
    let mut ch = RandomChooser::new(3);
    for _ in 0..200 {
        let r = mech.harvest_reforge(&it, &mut ch, "caster").unwrap();
        assert!(r.has_mod(crit) && r.has_mod(mana));
        assert_eq!(r.n_side(g, Gen::Suffix), 2);
        assert_eq!(r.crafted_count(), 0, "partial lock consumed");
    }
}

#[test]
fn one_suffix_lock_with_two_suffixes_protects_one_at_random() {
    let g = gd();
    let mech = Mech::new(g, Switches::default());
    let crit = elevated("Hunter: Attacks have", "Body Armour");
    let mana = elevated("Warlord: total mana cost", "Body Armour");
    let it = lock(&plate(&[crit, mana], &[Influence::Hunter, Influence::Warlord], Rarity::Rare), MetaKind::LockSuffixes(Some(1)));
    // annul only touches unprotected mods: one of the two suffixes is protected, so the
    // annul removes the other one or the metamod
    let out = outcomes(|ch| mech.annul(&it, ch).unwrap());
    let total: f64 = out.values().sum();
    assert!((total - 1.0).abs() < 1e-12);
    for o in out.keys() {
        assert!(o.has_mod(crit) || o.has_mod(mana), "at least one suffix survives");
    }
}
