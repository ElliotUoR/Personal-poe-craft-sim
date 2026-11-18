//! Rules registry (spec §4.I): every crafting rule the engine relies on, with its source
//! and verification status. Mechanics code refers to rules by id; analysis reports which
//! unverified/disputed rules a result depended on.
//!
//! Sources: PoEDB = https://poedb.tw/us/<page> (fetched 2026-11-18, includes the community
//! wiki text); "owner" = in-game testing by the project owner; Sanavixx = sanavixx.com/crafting.

use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Confirmed by a cited source (PoEDB / game data).
    Verified,
    /// Confirmed by the owner's in-game testing.
    OwnerConfirmed,
    /// Our best guess; needs testing.
    Unverified,
    /// Sources disagree; the default is chosen and a switch may exist.
    Disputed,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleInfo {
    pub id: &'static str,
    pub summary: &'static str,
    pub status: Status,
    pub source: &'static str,
    /// Open-question number in docs/spec.md §7, if any.
    pub question: Option<&'static str>,
}

pub const RULES: &[RuleInfo] = &[
    RuleInfo { id: "mod_weights", summary: "Chance of a mod = its weight / total weight of all mods that can currently roll (ilvl, tags incl. influence, open prefix/suffix slots, mod-group exclusion). Weight = first matching spawn tag, times generation-weight multiplier.", status: Status::Verified, source: "RePoE docs/mods.md; owner's data rules", question: None },
    RuleInfo { id: "rare_mod_count", summary: "A freshly rolled rare has 4/5/6 mods with weights 8/3/1 (chaos, alchemy, reforge, essence, Awakener fill).", status: Status::Unverified, source: "range 4-6 verified (PoEDB Orb_of_Alchemy wiki text); 8/3/1 odds are the community figure. Our Astral Plate chaos prefix split 26.3/45.2/28.5% matches Craft of Exile (26.1/44/28.5%), which shares the assumption", question: Some("Q4") },
    RuleInfo { id: "magic_mod_count", summary: "Magic items roll 1 or 2 affixes with equal chance.", status: Status::Unverified, source: "community figure", question: None },
    RuleInfo { id: "chaos_keeps_locked", summary: "Chaos Orb keeps fractured mods and the side protected by a 'cannot be changed' metamod.", status: Status::Verified, source: "PoE Wiki Meta-crafting_mod (2026-11-18): 'Respected, but open prefixes may be filled'", question: None },
    RuleInfo { id: "annul_protected", summary: "Annulment cannot remove fractured mods or mods protected by a metamod.", status: Status::Verified, source: "PoEDB Prefixes Cannot Be Changed / Fracturing Orb", question: None },
    RuleInfo { id: "scour_protected", summary: "Scouring removes every unprotected mod and the metamod itself; rarity becomes normal/magic/rare from what remains.", status: Status::Verified, source: "PoEDB Orb of Scouring; Sanavixx Endgame Chest step 2", question: None },
    RuleInfo { id: "dominance", summary: "Orb of Dominance (rare armour piece, >= 2 influenced mods): picks one changeable influenced mod to upgrade one tier (T1 -> Elevated, elevated -> value reroll) and removes another; skips fractured and metamod-protected mods; ignores influence type.", status: Status::Verified, source: "PoEDB Orb_of_Dominance", question: None },
    RuleInfo { id: "dominance_uniform", summary: "Dominance picks target and victim uniformly; upgrades below T1 ignore item level; needs >= 2 changeable influenced mods.", status: Status::OwnerConfirmed, source: "owner in-game, 2026-11-18", question: Some("Q8") },
    RuleInfo { id: "awakener", summary: "Awakener's Orb: same item class, different influences; target keeps base/ilvl/sockets/links; one influenced mod kept from each item (same mod group: one dropped at random); result is rare with both influences; elevated mods transfer.", status: Status::Verified, source: "PoEDB Awakeners_Orb, Orb_of_Dominance", question: None },
    RuleInfo { id: "awakener_uniform_fill", summary: "Awakener picks uniformly among several influenced mods; the rest is filled to the usual 4-6 mods.", status: Status::Unverified, source: "kept-mod pick uniform: owner-confirmed; fill count unknown (owner: 3-6, Alchemy/Chaos wiki text: 4-6; modelled 4-6)", question: Some("Q7") },
    RuleInfo { id: "conqueror_exalt", summary: "Conqueror exalted orbs need a rare with no influence; add the influence and one mod of it.", status: Status::Verified, source: "PoEDB Hunters_Exalted_Orb", question: None },
    RuleInfo { id: "reforge_respects_lock", summary: "Harvest reforge respects 'Prefixes/Suffixes cannot be changed': the protected side is untouched, the other side rerolled.", status: Status::OwnerConfirmed, source: "owner in-game test (Craft of Exile models this wrongly)", question: None },
    RuleInfo { id: "reforge_consumes_lock", summary: "The metamod is consumed by the reforge, so each retry costs a reforge plus a new lock.", status: Status::OwnerConfirmed, source: "owner in-game test", question: None },
    RuleInfo { id: "reforge_fills_locked_side", summary: "A locked reforge CAN add new mods into open slots on the locked side (protected mods stay). Switch: rule reforge_fills_locked_side off.", status: Status::Verified, source: "PoE Wiki Meta-crafting_mod (2026-11-18): every reroll is 'Respected, but open prefixes/suffixes may be filled'; owner in-game 2026-11-18", question: Some("Q2") },
    RuleInfo { id: "reforge_mod_count", summary: "Reforge: total mods follow rare_mod_count with kept mods counting toward it (at least one new mod); the guaranteed tagged mod is always new.", status: Status::Unverified, source: "no source gives the odds. Owner expects chaos odds (as modelled: total 4-6 incl. kept mods, >= 1 new). PoE Wiki Meta-crafting_mod (2026-11-18) example (2 locked prefixes + 1 open) says the orb 'may add 1 prefix and 0-3 suffixes, or 0 prefixes and 1-3 suffixes', i.e. possibly as few as 1 new mod", question: Some("Q3") },
    RuleInfo { id: "reforge_costs", summary: "Harvest reforge lifeforce costs per tag.", status: Status::Verified, source: "PoEDB Horticrafting", question: None },
    RuleInfo { id: "partial_lock", summary: "'N Suffix/Prefix Modifier(s) cannot be changed' (1 or 2 divine): can always be crafted; protects exactly N mods of that side; takes a slot on the other side; consumed by a reforge.", status: Status::OwnerConfirmed, source: "owner in-game, 2026-11-18", question: None },
    RuleInfo { id: "partial_lock_pick", summary: "With more than N mods on the side, the N protected mods are chosen at random, and the freed slots are open for new mods.", status: Status::OwnerConfirmed, source: "owner in-game, 2026-11-18", question: Some("Q1") },
    RuleInfo { id: "bench_metamod_slot", summary: "Metamods are crafted mods: 'Suffixes cannot be changed' is a prefix and 'Prefixes cannot be changed' a suffix; one crafted mod unless multimod.", status: Status::Verified, source: "RePoE mods.json / crafting_bench_options.json", question: None },
    RuleInfo { id: "beast_metamod_magic_only", summary: "The metamod beastcraft (Wild Bristle Matron) only works on magic items; rares need the bench craft.", status: Status::OwnerConfirmed, source: "owner in-game; PoE Wiki Meta-crafting_mod (2026-11-18) says all bench metamods 'can also be added via Beastcrafting using a Wild Bristle Matron' (no rarity stated)", question: None },
    RuleInfo { id: "beast_metamod_choice", summary: "The metamod beastcraft lets you choose which metamod.", status: Status::Unverified, source: "assumption; recipe still in 3.30 data (PoEDB Bestiary_Recipe and Wild_Bristle_Matron, 2026-11-18)", question: Some("Q11") },
    RuleInfo { id: "beast_add_influence_mod", summary: "'Add a Mod to a <Conqueror> Item' needs a rare with that influence and adds a random mod of it with conqueror-exalt weights.", status: Status::OwnerConfirmed, source: "owner in-game, 2026-11-18 (needs the influence; conqueror-exalt odds)", question: Some("Q10") },
    RuleInfo { id: "beast_swap_side", summary: "'Add a Prefix, Remove a Random Suffix' (and reverse): removes a random unprotected mod of the other side, then adds; if that side is locked it adds without removing.", status: Status::Verified, source: "PoE Wiki Meta-crafting_mod (2026-11-18): respects the lock on the side it adds to ('open ... may be filled'); with the removal side locked it 'will add without removing'", question: None },
    RuleInfo { id: "hinekora_fresh_preview", summary: "A declined Hinekora preview removes the lock; a new lock gives a new independent preview.", status: Status::OwnerConfirmed, source: "owner in-game, 2026-11-18", question: Some("Q6") },
    RuleInfo { id: "veiled_exalt", summary: "Veiled Exalted Orb removes a random unprotected mod and adds a veiled affix to a uniformly chosen open side; unveil offers 3 options by weight.", status: Status::OwnerConfirmed, source: "PoEDB Veiled_Exalted_Orb + owner (random side, 3 unveil options)", question: Some("Q9") },
    RuleInfo { id: "foulborn_exalt", summary: "Foulborn Exalted Orb adds a mod; for each mod type with N eligible tiers only the top K = ceil(N/2) (N<=6) or floor(N/2)+1 (N>6) can roll, weights x N/K.", status: Status::Verified, source: "PoEDB Foulborn_Exalted_Orb + PoE Wiki tier-culling formula (supplied by owner)", question: Some("Q5") },
    RuleInfo { id: "harvest_add", summary: "Harvest 'Add a new <Tag> modifier and remove another random modifier from a non-Influenced item': adds a tagged mod (weights as usual) then removes a uniformly random other unprotected mod. Assumed: magic or rare, needs an open slot.", status: Status::Unverified, source: "PoEDB Horticrafting (craft text and costs, 2026-11-18); owner screenshot; removal details assumed", question: None },
    RuleInfo { id: "harvest_reforge_influence", summary: "Harvest 'Reforge an Influenced Rare item with random modifiers, including a Influence modifier' (5000 Primal + 1 Sacred): like a tagged reforge whose guaranteed mod is any influenced mod of the item's influences; respects locks.", status: Status::Unverified, source: "PoEDB Horticrafting (craft text and cost, 2026-11-18); behaviour assumed like other reforges", question: None },
    RuleInfo { id: "essence_metamod", summary: "Essences cannot be used on items with metamods.", status: Status::Verified, source: "PoE Wiki Meta-crafting_mod (2026-11-18)", question: None },
    RuleInfo { id: "jewel_mod_count", summary: "Rare jewels have at most 2 prefixes and 2 suffixes and roll 3 or 4 mods (odds assumed equal).", status: Status::Unverified, source: "PoEDB Orb_of_Alchemy wiki text (3-4 on jewels); odds assumed", question: None },
    RuleInfo { id: "fracture", summary: "Fracturing Orb: rare with >= 4 mods; fractures a random non-crafted mod.", status: Status::Unverified, source: "PoEDB Fracturing Orb (crafted exclusion assumed)", question: None },
    RuleInfo { id: "max_links", summary: "Max-links beastcraft gives 6 links on body armours (sockets not otherwise simulated).", status: Status::Unverified, source: "assumption", question: None },
    RuleInfo { id: "no_attack_caster", summary: "'Cannot roll Attack/Caster Modifiers' removes mods with that tag from every pool while present.", status: Status::Unverified, source: "bench text", question: None },
];

pub fn info(id: &str) -> Option<&'static RuleInfo> {
    RULES.iter().find(|r| r.id == id)
}

/// Switchable rules (set per recipe with `rule <name> on|off`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Switches {
    /// Owner-confirmed 2026-11-18: on.
    pub reforge_fills_locked_side: bool,
}

impl Default for Switches {
    fn default() -> Self {
        Switches { reforge_fills_locked_side: true }
    }
}

impl Switches {
    pub fn set(&mut self, name: &str, on: bool) -> Result<(), String> {
        match name {
            "reforge_fills_locked_side" => self.reforge_fills_locked_side = on,
            _ => return Err(format!("unknown rule '{name}' (switchable: reforge_fills_locked_side)")),
        }
        Ok(())
    }
}

/// Collects the ids of rules an evaluation relied on.
#[derive(Debug, Clone, Default)]
pub struct RuleUse(pub BTreeSet<&'static str>);
impl RuleUse {
    pub fn used(&mut self, id: &'static str) {
        self.0.insert(id);
    }
    /// The unverified or disputed rules among those used.
    pub fn uncertain(&self) -> Vec<&'static RuleInfo> {
        self.0
            .iter()
            .filter_map(|id| info(id))
            .filter(|r| matches!(r.status, Status::Unverified | Status::Disputed))
            .collect()
    }
}
