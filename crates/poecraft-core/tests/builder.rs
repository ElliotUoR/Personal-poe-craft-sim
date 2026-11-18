//! Item builder: legality checks, pool view, trick hints.
use poecraft_core::builder::{hints, pool_view, validate, Blocked};
use poecraft_core::data::{default_data, Influence, Influences};
use poecraft_core::item::{Item, Rarity, Roll};
use poecraft_core::itemtext::parse_text;

const OWNER_CHEST: &str = "Rarity: RARE\nCataclysm Ward\nRoyal Plate\nWarlord Item\nHunter Item\nItem Level: 87\nQuality: 20\nImplicits: 0\nYou and your Minions prevent +100% of Reflected Physical Damage\nIgnore Stuns while using Socketed Attack Skills\nSocketed Attacks have -20 to Total Mana Cost\n+44% to Cold Resistance\nYou can apply an additional Curse\nAttacks have +1.69% to Critical Strike Chance\n{crafted}73% increased Armour";

#[test]
fn owner_chest_is_legal() {
    let gd = default_data();
    let p = parse_text(gd, OWNER_CHEST).unwrap();
    assert_eq!(validate(gd, &p.item), Vec::<String>::new());
}

#[test]
fn illegal_items_are_reported() {
    let gd = default_data();
    let b = gd.resolve_base("Astral Plate").unwrap();
    let curse = gd.mod_idx("AdditionalCurseOnEnemiesInfluence1").unwrap();
    // Hunter mod without Hunter influence
    let it = Item::new(b, 86, Rarity::Rare, Influences::default()).with_mods(vec![Roll::new(curse)]);
    assert!(validate(gd, &it).iter().any(|e| e.contains("needs hunter influence")), "{:?}", validate(gd, &it));
    // two mods of the same group
    let l1 = gd.mod_idx("IncreasedLife12").unwrap();
    let l2 = gd.mod_idx("IncreasedLife11").unwrap();
    let it = Item::new(b, 86, Rarity::Rare, Influences::default()).with_mods(vec![Roll::new(l1), Roll::new(l2)]);
    assert!(validate(gd, &it).iter().any(|e| e.contains("share mod group")));
    // item level too low for T1 life
    let it = Item::new(b, 80, Rarity::Rare, Influences::default()).with_mods(vec![Roll::new(l1)]);
    assert!(validate(gd, &it).iter().any(|e| e.contains("needs item level 86")));
    // three influences
    let it = Item::new(b, 86, Rarity::Rare, Influences::default().with(Influence::Hunter).with(Influence::Warlord).with(Influence::Crusader));
    assert!(validate(gd, &it).iter().any(|e| e.contains("at most two influences")));
}

#[test]
fn pool_view_marks_blocked_mods_and_sums_to_one() {
    let gd = default_data();
    let b = gd.resolve_base("Astral Plate").unwrap();
    let l1 = gd.mod_idx("IncreasedLife12").unwrap();
    let it = Item::new(b, 84, Rarity::Rare, Influences::default().with(Influence::Hunter)).with_mods(vec![Roll::new(l1)]);
    let v = pool_view(gd, &it);
    let life = v.iter().find(|e| e.id == "IncreasedLife11").unwrap();
    assert_eq!(life.blocked, Some(Blocked::Group));
    let crit_t1 = v.iter().find(|e| e.id == "AdditionalCritWithAttacksInfluence2_").unwrap();
    assert_eq!(crit_t1.blocked, None);
    let sum: f64 = v.iter().filter(|e| e.side == "suffix").map(|e| e.chance).sum();
    assert!((sum - 1.0).abs() < 1e-9);
}

#[test]
fn hints_find_the_known_tricks() {
    let gd = default_data();
    let chest = Item::new(gd.resolve_base("Astral Plate").unwrap(), 86, Rarity::Rare, Influences::default().with(Influence::Hunter).with(Influence::Warlord));
    let h = hints(gd, &chest);
    assert!(h.iter().any(|x| x.text.contains("additional Curse") && x.text.contains("caster")), "{h:#?}");
    let boots = Item::new(gd.resolve_base("Two-Toned Boots").unwrap(), 84, Rarity::Rare, Influences::default().with(Influence::Hunter).with(Influence::Redeemer));
    let h = hints(gd, &boots);
    assert!(h.iter().any(|x| x.text.contains("Elusive") && x.text.contains("critical")), "{h:#?}");
    assert!(h.iter().any(|x| x.text.contains("Movement Speed") && x.text.contains("speed")), "{h:#?}");
}
