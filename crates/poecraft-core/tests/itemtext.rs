//! Item text: parse real PoB text, and round-trip crafted items through PoB text.
use poecraft_core::data::{default_data, Influence, Influences};
use poecraft_core::engine::Engine;
use poecraft_core::item::Rarity;
use poecraft_core::itemtext::{clean, parse_text, render_line, to_pob_text, ExportOpts};
use poecraft_core::lang::parse;
use poecraft_core::prices::Prices;
use std::path::Path;

/// The owner's Royal Plate as PoB's harness printed it (JellyFlow build, 2026-11-18).
const OWNER_CHEST: &str = "Rarity: RARE
Cataclysm Ward
Royal Plate
Armour: 2728
ArmourBasePercentile: 0.7405
Unique ID: 7821d7db2ae36fe88cbe63d337102cbafdfe6ed6c01014bf345ecd75894bbc1b
Warlord Item
Hunter Item
Item Level: 87
Quality: 20
Sockets: R-W-W-G-W-W
LevelReq: 84
Implicits: 0
You and your Minions prevent +100% of Reflected Physical Damage
Ignore Stuns while using Socketed Attack Skills
Socketed Attacks have -20 to Total Mana Cost
+44% to Cold Resistance
You can apply an additional Curse
Attacks have +1.69% to Critical Strike Chance
{crafted}73% increased Armour";

#[test]
fn helpers() {
    assert_eq!(
        clean("Bleeding you inflict deals Damage (10-12)% [DamageFaster|Faster]"),
        "Bleeding you inflict deals Damage (10-12)% Faster"
    );
    assert_eq!(
        clean("gain [Onslaught] for 4 seconds"),
        "gain Onslaught for 4 seconds"
    );
    assert_eq!(
        render_line("Attacks have +(1.6-2)% to Critical Strike Chance", 1.0),
        "Attacks have +2.0% to Critical Strike Chance"
    );
    assert_eq!(
        render_line("Regenerate (128.1-152) Life per second", 0.0),
        "Regenerate 128.1 Life per second"
    );
    assert_eq!(
        render_line("+(175-189) to maximum Life", 0.5),
        "+182 to maximum Life"
    );
}

#[test]
fn parses_owner_chest() {
    let gd = default_data();
    let p = parse_text(gd, OWNER_CHEST).unwrap();
    assert!(p.unmatched.is_empty(), "unmatched: {:?}", p.unmatched);
    let it = &p.item;
    assert_eq!(gd.base(it.base).name, "Royal Plate");
    assert_eq!((it.ilvl, it.rarity, p.quality), (87, Rarity::Rare, 20));
    assert!(
        it.influences.contains(Influence::Hunter) && it.influences.contains(Influence::Warlord)
    );
    let ids: Vec<&str> = it.mods.iter().map(|r| gd.m(r.m).id.as_str()).collect();
    for want in [
        "SocketedAttacksManaCostInfluenceMaven",
        "AdditionalCritWithAttacksInfluenceMaven",
        "AdditionalCurseOnEnemiesInfluence1",
    ] {
        assert!(ids.contains(&want), "{want} missing from {ids:?}");
    }
    assert_eq!(it.mods.len(), 6);
    assert_eq!(it.crafted_count(), 1);
    // elevated crit tier 0, curse tier 1
    let crit = it
        .mods
        .iter()
        .find(|r| gd.m(r.m).id == "AdditionalCritWithAttacksInfluenceMaven")
        .unwrap();
    assert_eq!(gd.tier(crit.m, it.base), 0);
}

#[test]
fn round_trips_crafted_items() {
    let gd = default_data();
    let pr = Prices::load(&gd.data_dir, &[], false).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut n = 0;
    for f in [
        "recipes/endgame_chest_no_preview.craft",
        "recipes/boots/route_a_crit_reforge_bench_ms.craft",
        "recipes/boots/route_d_hybrid_veiled_ms.craft",
    ] {
        let prog = poecraft_core::lang::parse_file(&root.join(f)).unwrap();
        let eng = Engine::new(gd, &pr, Engine::switches_for(&prog).unwrap());
        for seed in 0..60 {
            let r = eng.run_mc(&prog, seed, 200_000, None).unwrap();
            let Some(item) = r.item else { continue };
            if item.mods.iter().any(|x| gd.m(x.m).id.starts_with("Veiled")) {
                continue; // veiled placeholders have no real text
            }
            for roll in [0.0, 0.5, 1.0] {
                let text = to_pob_text(
                    gd,
                    &item,
                    &ExportOpts {
                        roll,
                        ..Default::default()
                    },
                );
                let p = parse_text(gd, &text).unwrap();
                assert!(
                    p.unmatched.is_empty(),
                    "{f} seed {seed}: unmatched {:?}\n{text}",
                    p.unmatched
                );
                if p.item.mods != item.mods && !p.ambiguous.is_empty() {
                    continue; // identical prefix/suffix wording with room on both sides: reported
                }
                assert_eq!(
                    p.item.mods,
                    item.mods,
                    "{f} seed {seed} roll {roll}\n{text}\n{}\n{}",
                    item.describe(gd),
                    p.item.describe(gd)
                );
                assert_eq!(p.item.influences, item.influences);
                n += 1;
            }
        }
    }
    assert!(n > 300, "only {n} round trips");
}

#[test]
fn item_rarity_side_is_fixed_by_slot_limits_or_reported() {
    let gd = default_data();
    // 3 prefixes already: the rarity line must be the suffix version
    let txt = "Rarity: RARE
X
Two-Toned Boots
Item Level: 84
Implicits: 0
30% increased Movement Speed
+80 to maximum Life
+30 to maximum Mana
+9% to Item Rarity";
    let p = parse_text(gd, txt).unwrap();
    assert!(p.unmatched.is_empty(), "{:?}", p.unmatched);
    let r = p.item.mods.iter().find(|r| gd.m(r.m).text.contains("Item Rarity")).unwrap();
    assert_eq!(gd.m(r.m).gen, poecraft_core::data::Gen::Suffix);
    assert!(p.ambiguous.is_empty(), "{:?}", p.ambiguous);
    // room on both sides: reported as ambiguous
    let txt2 = "Rarity: RARE
X
Two-Toned Boots
Item Level: 84
Implicits: 0
+9% to Item Rarity";
    assert_eq!(parse_text(gd, txt2).unwrap().ambiguous.len(), 1);
    // advanced copy header decides it
    let txt3 = "Rarity: RARE
X
Two-Toned Boots
Item Level: 84
Implicits: 0
{ Suffix Modifier \"of Plunder\" (Tier: 4) }
+9% to Item Rarity";
    let p3 = parse_text(gd, txt3).unwrap();
    assert!(p3.ambiguous.is_empty(), "{:?}", p3.ambiguous);
    assert_eq!(gd.m(p3.item.mods[0].m).gen, poecraft_core::data::Gen::Suffix);
}

#[test]
fn parses_game_clipboard_format() {
    let gd = default_data();
    let txt = "Item Class: Boots
Rarity: Rare
Gale Stride
Two-Toned Boots
--------
Quality: +20% (augmented)
Evasion Rating: 200 (augmented)
--------
Requirements:
Level: 70
--------
Item Level: 84
--------
+12% to Fire and Cold Resistances (implicit)
--------
30% increased Movement Speed
(5-10)% chance to gain Elusive on Critical Strike
+40% to Lightning Resistance (crafted)
--------
Hunter Item
Redeemer Item";
    let txt = txt.replace("(5-10)%", "7%");
    let p = parse_text(gd, &txt).unwrap();
    assert!(p.unmatched.is_empty(), "{:?}", p.unmatched);
    assert_eq!(p.item.mods.len(), 3);
    assert_eq!(p.item.crafted_count(), 1);
    assert_eq!(
        p.item.influences,
        Influences::default()
            .with(Influence::Hunter)
            .with(Influence::Redeemer)
    );
    let _ = parse; // keep import used
}
