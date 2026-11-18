import glob
import os

import pytest

from poecraft import mechanics as M
from poecraft.analysis import analyse, check_agreement, exact_stats, mc_stats
from poecraft.data import DataError, default_data
from poecraft.engine import Engine
from poecraft.item import PREFIX_LOCK, SUFFIX_LOCK, Item, Roll
from poecraft.lang import RecipeError, parse, parse_file
from poecraft.prices import Prices

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
GD = default_data()
ENG = Engine(GD, Prices.load(include_local=False))
AP = GD.resolve_base("Astral Plate")


def fam(ref, cls="Body Armour"):
    return GD.families[GD.resolve_mod(ref, cls)]


def item(*mods, infl=("hunter", "warlord"), rarity="rare", ilvl=86):
    return Item(AP, ilvl, rarity, frozenset(infl)).with_mods(tuple(Roll(m) for m in mods))


# ------------------------------------------------------------------ data
def test_names_resolve_and_elevated_mapping():
    assert fam("Hunter: additional curse").elevated
    assert fam("Warlord: total mana cost").elevated == "SocketedAttacksManaCostInfluenceMaven"
    crit = fam("Hunter: Attacks have")
    assert crit.tiers[0] == "AdditionalCritWithAttacksInfluence2_"  # T1 needs ilvl 84
    assert GD.mods[crit.tiers[0]].level == 84
    assert crit.elevated == "AdditionalCritWithAttacksInfluenceMaven"


def test_ambiguous_mod_lists_candidates():
    with pytest.raises(DataError) as e:
        GD.resolve_mod("increased maximum life", "Body Armour")
    assert "ambiguous" in str(e.value) and "Hunter" in str(e.value)


def test_curse_is_only_caster_prefix_on_hunter_warlord_astral_plate():
    """Open question 4: answered from RePoE 3.30 data."""
    pool = GD.spawn_pool(AP, frozenset({"hunter", "warlord"}), 86)
    caster_prefixes = [m for m, _w in pool if GD.mods[m].gen == "prefix" and "caster" in GD.mods[m].tags]
    assert caster_prefixes == ["AdditionalCurseOnEnemiesInfluence1"]


def test_suffix_lock_metamod_is_a_prefix():
    """Open question 3: the 'Suffixes Cannot Be Changed' metamod takes a prefix slot."""
    assert GD.mods[SUFFIX_LOCK].gen == "prefix"
    assert GD.mods[PREFIX_LOCK].gen == "suffix"


# ------------------------------------------------------------------ mechanics
def outcomes(fn):
    from poecraft.engine import _enumerate
    res = _enumerate(lambda ch: (fn(ch), {}), [10 ** 6])
    return {st: p for (st, _c), p in res.items()}


def test_dominance_two_mods_is_fifty_fifty():
    crit, life = fam("Hunter: Attacks have").tiers[0], fam("Hunter: max life %").tiers[0]
    it = item(crit, life, infl=("hunter",))
    out = outcomes(lambda ch: M.dominance(it, ch))
    elevated = sum(p for st, p in out.items() if st.has_mod(fam("Hunter: Attacks have").elevated))
    assert elevated == pytest.approx(0.5)
    assert all(len(st.influenced_mods(GD)) == 1 for st in out)


def test_dominance_skips_metamod_protected_mods():
    crit, life = fam("Hunter: Attacks have").tiers[0], fam("Hunter: max life %").tiers[0]
    mana = fam("Warlord: total mana cost").tiers[0]
    it = item(crit, mana, life).with_mods(item(crit, mana, life).mods + (Roll(PREFIX_LOCK, crafted=True),))
    # suffixes (crit, mana) are not protected; prefixes (life) are -> only crit/mana eligible
    for st in outcomes(lambda ch: M.dominance(it, ch)):
        assert st.has_mod(fam("Hunter: max life %").tiers[0])


def test_reforge_respects_suffix_lock_and_consumes_metamod():
    """Ground truth from in-game testing (user)."""
    crit_e = fam("Hunter: Attacks have").elevated
    mana_e = fam("Warlord: total mana cost").elevated
    it = item(crit_e, mana_e)
    it = M.bench_craft(it, SUFFIX_LOCK)
    ch = M.RandomChooser(1)
    for _ in range(200):
        r = M.harvest_reforge(it, ch, "caster")
        assert r.has_mod(crit_e) and r.has_mod(mana_e)
        assert not r.has_mod(SUFFIX_LOCK)  # consumed
        assert r.has_mod("AdditionalCurseOnEnemiesInfluence1")  # only caster prefix -> guaranteed
        assert r.n_suffix(GD) == 2


def test_scour_with_suffix_lock_leaves_magic_item():
    mana_e = fam("Warlord: total mana cost").elevated
    it = M.bench_craft(item(mana_e, "LocalIncreasedPhysicalDamageReductionRating5", infl=("warlord",)), SUFFIX_LOCK)
    r = M.scour(it)
    assert r.rarity == "magic" and r.mod_ids() == [mana_e]


def test_awakener_keeps_one_influenced_mod_from_each():
    crit_e = fam("Hunter: Attacks have").elevated
    mana_e = fam("Warlord: total mana cost").elevated
    target = item(mana_e, infl=("warlord",), rarity="magic")
    donor = item(crit_e, infl=("hunter",))
    ch = M.RandomChooser(2)
    for _ in range(100):
        r = M.awaken(target, donor, ch)
        assert r.has_mod(crit_e) and r.has_mod(mana_e) and r.rarity == "rare"
        assert r.influences == {"hunter", "warlord"} and r.ilvl == 86
        assert 4 <= len(r.mods) <= 6


def test_conqueror_exalt_requires_uninfluenced_rare():
    with pytest.raises(M.CraftError):
        M.conqueror_exalt(item(infl=("hunter",)), M.RandomChooser(0), "warlord")
    r = M.conqueror_exalt(item(infl=()), M.RandomChooser(0), "hunter")
    assert r.influences == {"hunter"} and GD.mods[r.mods[0].id].influence == "hunter"


# ------------------------------------------------------------------ engine: exact vs MC
SMALL = '''
repeat {
    buy "Astral Plate" ilvl 86 rare influence hunter
        with "Hunter: attack crit" tier 1 with "Hunter: max life %" tier 1 for 50 chaos
    dominance
} until has "Hunter: attack crit" elevated
goal has "Hunter: attack crit" elevated
'''


def test_geometric_loop_exact():
    ex = exact_stats(ENG, parse(SMALL))
    dom = ENG.prices["dominance"]
    assert ex["mean_counts"]["dominance"] == pytest.approx(2.0)
    assert ex["mean_chaos"] == pytest.approx(2 * (dom + 50))
    # Var of geometric(1/2) trials * cost^2 = 2 * cost^2
    assert ex["std_chaos"] == pytest.approx((2 ** 0.5) * (dom + 50))


@pytest.mark.parametrize("path", ["recipes/tests/dominance_2mods.craft", "recipes/tests/dominance_loop.craft",
                                  "recipes/tests/three_mods.craft", "recipes/tests/alt_spam.craft"])
def test_exact_and_monte_carlo_agree(path):
    prog = parse_file(os.path.join(ROOT, path))
    ex = exact_stats(ENG, prog)
    mc = mc_stats(ENG, prog, runs=3000, seed=7)
    assert check_agreement(ex, mc).startswith("AGREE"), check_agreement(ex, mc)


def test_iid_alteration_loop_matches_geometric():
    prog = parse_file(os.path.join(ROOT, "recipes/tests/alt_spam.craft"))
    ex = exact_stats(ENG, prog)
    assert ex["p_success"] == pytest.approx(1.0)
    assert ex["mean_counts"]["alt"] > 10


# ------------------------------------------------------------------ language errors
def err(src):
    with pytest.raises(RecipeError) as e:
        ENG.run_mc(parse(src), seed=0)
    return str(e.value)


def test_error_unknown_statement_suggests():
    assert "Did you mean: dominance" in err('buy "Astral Plate"\ndominanse')


def test_error_needs_item():
    assert "start with 'buy'" in err("chaos")


def test_error_wrong_rarity_has_line_number():
    m = err('buy "Astral Plate" ilvl 86\nregal')
    assert ":2:" in m and "Regal Orb needs a magic item" in m


def test_error_influence_missing_on_buy():
    assert 'add "influence hunter"' in err('buy "Astral Plate" rare with "Hunter: additional curse"')


def test_error_loop_never_finishes():
    # no Hunter influence -> the curse can never roll
    src = 'buy "Astral Plate" ilvl 86 magic\nrepeat { alt } until has "Hunter: additional curse"'
    assert "can never finish" in err(src)
    with pytest.raises(RecipeError):
        ENG.run_exact(parse(src))


# ------------------------------------------------------------------ all shipped recipes run
@pytest.mark.parametrize("path", sorted(glob.glob(os.path.join(ROOT, "recipes", "**", "*.craft"), recursive=True)))
def test_recipe_runs(path):
    if os.sep + "lib" + os.sep in path:
        parse_file(path)
        return
    prog = parse_file(path)
    for s in range(3):
        ENG.run_mc(prog, seed=s)


def test_veiled_exalt_with_suffix_lock_only_hits_prefixes_and_unveil_offers_ms():
    b = GD.resolve_base("Two-Toned Boots")
    tw = GD.families[GD.resolve_mod("Hunter: tailwind", "Boots")].elevated
    on = GD.families[GD.resolve_mod("Redeemer: onslaught", "Boots")].elevated
    el = GD.families[GD.resolve_mod("Redeemer: elusive", "Boots")].tiers[0]
    it = Item(b, 84, "rare", frozenset({"hunter", "redeemer"})).with_mods(
        (Roll(tw), Roll(on), Roll(el), Roll("FireResist5"), Roll(SUFFIX_LOCK, crafted=True)))
    ms = GD.resolve_mod("JunMasterVeiledMovementVelocityAndOnslaughtOnKill", "Boots")
    ch = M.RandomChooser(5)
    got_ms = 0
    for _ in range(300):
        v = M.veiled_exalt(it, ch)
        assert v.has_mod(tw) and v.has_mod(on) and v.has_mod("VeiledPrefix")
        u = M.unveil(v, ch, [ms])
        got_ms += any("MovementVelocity" in r.id for r in u.mods)
    assert got_ms > 150  # ~66% of unveils offer a Chosen MS mod


def test_rule_switch_reforge_fills_locked_side():
    src = '''
rule reforge_fills_locked_side on
buy "Two-Toned Boots" ilvl 84 rare influence hunter with "Hunter: tailwind"
craft lock_suffixes
harvest reforge physical
goal has "Hunter: tailwind" and count(hunter) >= 2
'''
    prog = parse(src)
    assert ENG.run_mc(prog, seed=1).success
    with pytest.raises(RecipeError):  # default rule: no physical prefix can roll
        ENG.run_mc(parse(src.replace("rule reforge_fills_locked_side on", "")), seed=1)
