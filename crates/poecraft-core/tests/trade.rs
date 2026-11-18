//! Trade links: stat mapping for the owner's chest, and encode/decode round trip.
use poecraft_core::data::default_data;
use poecraft_core::itemtext::parse_text;
use poecraft_core::trade::{build_link, decode, normalise, LinkOpts, TradeStats};

const OWNER_CHEST: &str = "Rarity: RARE\nCataclysm Ward\nRoyal Plate\nWarlord Item\nHunter Item\nItem Level: 87\nImplicits: 0\nYou and your Minions prevent +100% of Reflected Physical Damage\nIgnore Stuns while using Socketed Attack Skills\nSocketed Attacks have -20 to Total Mana Cost\n+44% to Cold Resistance\nYou can apply an additional Curse\nAttacks have +1.69% to Critical Strike Chance\n{crafted}73% increased Armour";

#[test]
fn normalise_examples() {
    assert_eq!(normalise("Attacks have +(1.6-2)% to Critical Strike Chance"), "attacks have #% to critical strike chance");
    assert_eq!(normalise("Socketed Attacks have -20 to Total Mana Cost"), "socketed attacks have # to total mana cost");
    assert_eq!(normalise("Socketed Attacks have +# to Total Mana Cost"), "socketed attacks have # to total mana cost");
}

#[test]
fn owner_chest_link_uses_browser_verified_ids() {
    let gd = default_data();
    let ts = TradeStats::load(&gd.data_dir).unwrap();
    let item = parse_text(gd, OWNER_CHEST).unwrap().item;
    let l = build_link(gd, &ts, &item, &LinkOpts { league: "Covens", any_base: false, status: "securable" });
    let f = |id: &str| l.filters.iter().find(|x| x.stat == id).unwrap_or_else(|| panic!("{id} missing: {:#?}", l.filters)).value;
    assert_eq!(f("explicit.stat_2264586521"), Some(-20.0)); // Socketed Attacks have +# to Total Mana Cost
    assert_eq!(f("explicit.stat_30642521"), Some(1.0)); // You can apply # additional Curses
    assert_eq!(f("explicit.stat_2572042788"), Some(1.6)); // Attacks have +#% to Critical Strike Chance (elevated)
    assert!(l.filters.iter().any(|x| x.stat.starts_with("crafted.")), "armour craft uses the crafted group");
    assert_eq!(l.skipped, vec!["Ignore Stuns while using Socketed Attack Skills".to_string()]);
    assert!(l.url.starts_with("https://www.pathofexile.com/trade/search/Covens/H4sI"));
    let code = l.url.rsplit('/').next().unwrap();
    assert_eq!(decode(code).unwrap(), l.query);
    assert_eq!(l.query["filters"]["misc_filters"]["filters"]["hunter_item"]["option"], "true");
}
