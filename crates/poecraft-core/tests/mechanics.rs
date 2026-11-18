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
    assert_eq!(
        g.m(elevated("Warlord: total mana cost", "Body Armour")).id,
        "SocketedAttacksManaCostInfluenceMaven"
    );
}

#[test]
fn dominance_two_mods_is_fifty_fifty() {
    let mech = Mech::new(gd(), Switches::default());
    let crit = fam_tier("Hunter: Attacks have", "Body Armour", 0);
    let life = fam_tier("Hunter: max life %", "Body Armour", 0);
    let it = plate(&[crit, life], &[Influence::Hunter], Rarity::Rare);
    let out = outcomes(|ch| mech.dominance(&it, ch).unwrap());
    let el = elevated("Hunter: Attacks have", "Body Armour");
    let p: f64 = out
        .iter()
        .filter(|(i, _)| i.has_mod(el))
        .map(|(_, p)| p)
        .sum();
    assert!((p - 0.5).abs() < 1e-12, "p = {p}");
    assert!(out.keys().all(|i| i.influenced_mods(gd()).len() == 1));
}

#[test]
fn dominance_skips_metamod_protected_mods() {
    let mech = Mech::new(gd(), Switches::default());
    let crit = fam_tier("Hunter: Attacks have", "Body Armour", 0);
    let life = fam_tier("Hunter: max life %", "Body Armour", 0);
    let mana = fam_tier("Warlord: total mana cost", "Body Armour", 0);
    let it = lock(
        &plate(
            &[crit, mana, life],
            &[Influence::Hunter, Influence::Warlord],
            Rarity::Rare,
        ),
        MetaKind::LockPrefixes(None),
    );
    for (o, _) in outcomes(|ch| mech.dominance(&it, ch).unwrap()) {
        assert!(o.has_mod(life));
    }
}

#[test]
fn reforge_respects_full_lock_consumes_it_and_guarantees_curse() {
    let g = gd();
    // with the switch off nothing is added to the locked side (pre-2026-11-18 model)
    let mech = Mech::new(
        g,
        Switches {
            reforge_fills_locked_side: false,
        },
    );
    let crit = elevated("Hunter: Attacks have", "Body Armour");
    let mana = elevated("Warlord: total mana cost", "Body Armour");
    let it = lock(
        &plate(
            &[crit, mana],
            &[Influence::Hunter, Influence::Warlord],
            Rarity::Rare,
        ),
        MetaKind::LockSuffixes(None),
    );
    let curse = g.mod_idx("AdditionalCurseOnEnemiesInfluence1").unwrap();
    let mut ch = RandomChooser::new(1);
    for _ in 0..300 {
        let r = mech.harvest_reforge(&it, &mut ch, "caster").unwrap();
        assert!(r.has_mod(crit) && r.has_mod(mana) && r.has_mod(curse));
        assert_eq!(r.crafted_count(), 0, "metamod consumed");
        assert_eq!(
            r.n_side(g, Gen::Suffix),
            2,
            "no suffix added to the locked side"
        );
    }
}

#[test]
fn scour_with_suffix_lock_leaves_magic_item() {
    let g = gd();
    let mech = Mech::new(g, Switches::default());
    let mana = elevated("Warlord: total mana cost", "Body Armour");
    let armour = g
        .mod_idx("LocalIncreasedPhysicalDamageReductionRatingPercent5")
        .unwrap();
    let it = lock(
        &plate(&[mana, armour], &[Influence::Warlord], Rarity::Rare),
        MetaKind::LockSuffixes(None),
    );
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
    assert!(mech
        .conqueror_exalt(
            &plate(&[], &[Influence::Hunter], Rarity::Rare),
            &mut RandomChooser::new(0),
            Influence::Warlord
        )
        .is_err());
    let r = mech
        .conqueror_exalt(
            &plate(&[], &[], Rarity::Rare),
            &mut RandomChooser::new(0),
            Influence::Hunter,
        )
        .unwrap();
    assert!(r.influences.contains(Influence::Hunter));
    assert_eq!(gd().m(r.mods[0].m).influence, Some(Influence::Hunter));
}

#[test]
fn beast_metamod_only_on_magic() {
    let mech = Mech::new(gd(), Switches::default());
    let crit = elevated("Hunter: Attacks have", "Body Armour");
    assert!(mech
        .beast_metamod(
            &plate(&[crit], &[Influence::Hunter], Rarity::Rare),
            MetaKind::LockSuffixes(None)
        )
        .is_err());
    let m = mech
        .beast_metamod(
            &plate(&[crit], &[Influence::Hunter], Rarity::Magic),
            MetaKind::LockSuffixes(None),
        )
        .unwrap();
    assert!(m.side_closed_by_lock(gd(), Gen::Suffix));
}

// ---------------------------------------------------------------- partial locks (owner, 2026-11-18)
#[test]
fn partial_lock_covering_all_suffixes_acts_as_full_lock() {
    let g = gd();
    let mech = Mech::new(
        g,
        Switches {
            reforge_fills_locked_side: false,
        },
    );
    let crit = elevated("Hunter: Attacks have", "Body Armour");
    let mana = elevated("Warlord: total mana cost", "Body Armour");
    let it = lock(
        &plate(
            &[crit, mana],
            &[Influence::Hunter, Influence::Warlord],
            Rarity::Rare,
        ),
        MetaKind::LockSuffixes(Some(2)),
    );
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
    let it = lock(
        &plate(
            &[crit, mana],
            &[Influence::Hunter, Influence::Warlord],
            Rarity::Rare,
        ),
        MetaKind::LockSuffixes(Some(1)),
    );
    // annul only touches unprotected mods: one of the two suffixes is protected, so the
    // annul removes the other one or the metamod
    let out = outcomes(|ch| mech.annul(&it, ch).unwrap());
    let total: f64 = out.values().sum();
    assert!((total - 1.0).abs() < 1e-12);
    for o in out.keys() {
        assert!(
            o.has_mod(crit) || o.has_mod(mana),
            "at least one suffix survives"
        );
    }
}

// ---------------------------------------------------------------- owner answers 2026-11-18
#[test]
fn locked_reforge_can_add_to_locked_side_by_default() {
    let g = gd();
    let mech = Mech::new(g, Switches::default());
    let crit = elevated("Hunter: Attacks have", "Body Armour");
    let mana = elevated("Warlord: total mana cost", "Body Armour");
    let it = lock(
        &plate(
            &[crit, mana],
            &[Influence::Hunter, Influence::Warlord],
            Rarity::Rare,
        ),
        MetaKind::LockSuffixes(None),
    );
    let mut ch = RandomChooser::new(4);
    let mut added = 0;
    for _ in 0..500 {
        let r = mech.harvest_reforge(&it, &mut ch, "caster").unwrap();
        assert!(
            r.has_mod(crit) && r.has_mod(mana),
            "protected suffixes stay"
        );
        added += (r.n_side(g, Gen::Suffix) == 3) as usize;
    }
    assert!(
        added > 0,
        "a third suffix is sometimes added to the locked side"
    );
}

#[test]
fn foulborn_keeps_top_tiers_with_wiki_weights() {
    use poecraft_core::mechanics::keep_tiers;
    let table = [
        (1, 1),
        (2, 1),
        (3, 2),
        (4, 2),
        (5, 3),
        (6, 3),
        (7, 4),
        (8, 5),
        (9, 5),
        (10, 6),
        (11, 6),
        (12, 7),
        (13, 7),
    ];
    for (n, k) in table {
        assert_eq!(keep_tiers(n), k, "N={n}");
    }
    // the wiki example: 4 eligible tiers 800/400/200/100 (lowest -> highest) keep 200/100 as 400/200
    let g = gd();
    let fam = g.resolve_mod("flat life", Some("Body Armour")).unwrap();
    let b = g.resolve_base("Astral Plate").unwrap();
    let tiers: Vec<ModIdx> = g.tiers_for(fam, b).iter().rev().take(4).copied().collect(); // 4 lowest, lowest first
    let (m, w) = poecraft_core::mechanics::foulborn_cull(g, &tiers, &[800, 400, 200, 100]);
    assert_eq!(m, vec![tiers[3], tiers[2]]);
    assert_eq!(
        w.iter().map(|x| x / 840).collect::<Vec<_>>(),
        vec![200, 400]
    );
}

#[test]
fn jewels_have_two_prefixes_and_suffixes() {
    let g = gd();
    let mech = Mech::new(g, Switches::default());
    let b = g.resolve_base("Cobalt Jewel").unwrap();
    let it = Item::new(b, 84, Rarity::Normal, Influences::default());
    let mut ch = RandomChooser::new(9);
    for _ in 0..300 {
        let r = mech.alchemy(&it, &mut ch).unwrap();
        assert!((3..=4).contains(&r.mods.len()));
        assert!(r.n_side(g, Gen::Prefix) <= 2 && r.n_side(g, Gen::Suffix) <= 2);
    }
}

// ---------------------------------------------------------------- Harvest add / influence reforge
#[test]
fn harvest_add_needs_non_influenced_and_swaps_a_mod() {
    let g = gd();
    let mech = Mech::new(g, Switches::default());
    let life = fam_tier("flat life", "Body Armour", 2);
    let fire = g.mod_idx("FireResist5").unwrap();
    let it = plate(&[life, fire], &[], Rarity::Rare);
    let mut ch = RandomChooser::new(5);
    for _ in 0..200 {
        let r = mech.harvest_add(&it, &mut ch, "cold").unwrap();
        assert_eq!(r.mods.len(), 2, "one added, one removed");
        assert!(r.mods.iter().any(|x| g.m(x.m).has_tag("cold")));
    }
    let infl = plate(&[life, fire], &[Influence::Hunter], Rarity::Rare);
    assert!(mech.harvest_add(&infl, &mut ch, "cold").is_err());
}

#[test]
fn harvest_reforge_influence_guarantees_an_influenced_mod() {
    let g = gd();
    let mech = Mech::new(g, Switches::default());
    let it = plate(
        &[g.mod_idx("FireResist5").unwrap()],
        &[Influence::Hunter, Influence::Warlord],
        Rarity::Rare,
    );
    let mut ch = RandomChooser::new(6);
    for _ in 0..200 {
        let r = mech.harvest_reforge(&it, &mut ch, "influence").unwrap();
        assert!(!r.influenced_mods(g).is_empty());
    }
}
