-- poecraft "Test in PoB" scorer, run through pob-headless (run.py --lua score_items.lua).
--
-- Env:
--   ITEMS_JSON  path to {"slot": "Body Armour", "items": ["<PoB item text>", ...]}
--   FILL=1      assume resistances capped (adds a +200% all-res custom block), owner default
--   FAST=1      use PoB's tooltip calculator (GetMiscCalculator, ~8x faster, DPS ~2-3% off)
--
-- Standard scoring (owner rules): enemy is a Pinnacle boss; cooldown guard skills (Molten Shell,
-- Vaal Molten Shell, Steelskin, Immortal Call, Bone Armour, Arcane Cloak) are excluded from EHP;
-- every result is given with flasks up and down. No other config option is changed (the build's
-- saved config is used as-is).
local dkjson = require("dkjson")
local job = dkjson.decode(io.open(os.getenv("ITEMS_JSON"), "rb"):read("*a"))
local slot = job.slot
local it, st, ct = build.itemsTab, build.skillsTab, build.configTab

ct.input.enemyIsBoss = "Pinnacle"
local GUARD = { ["Molten Shell"] = true, ["Vaal Molten Shell"] = true, ["Steelskin"] = true, ["Immortal Call"] = true, ["Bone Armour"] = true, ["Arcane Cloak"] = true }
for _, g in ipairs(st.socketGroupList) do
	for _, gm in ipairs(g.gemList) do
		if GUARD[gm.nameSpec or ""] then gm.enabled = false end
	end
	st:ProcessSocketGroup(g)
end
local cl = ct.configSets[ct.activeConfigSetId].customModsList
for i = #cl, 1, -1 do if cl[i].title == "poecraft ResFill" then table.remove(cl, i) end end
if os.getenv("FILL") == "1" then
	cl[#cl + 1] = { title = "poecraft ResFill", enabled = true, text = "+200% to Fire Resistance\n+200% to Cold Resistance\n+200% to Lightning Resistance\n+200% to Chaos Resistance" }
end
ct:BuildModList()

local function equip(raw)
	local item = new("Item", raw)
	it:AddItem(item, true)
	local set = it.itemSets[it.activeItemSetId]
	set[slot] = set[slot] or {}
	set[slot].selItemId = item.id
	if it.slots[slot] then it.slots[slot]:SetSelItemId(item.id) end
	it:SetActiveItemSet(it.activeItemSetId)
	return item
end

local function flasks(up)
	for i = 1, 5 do local sl = it.slots["Flask " .. i]; if sl then sl.active = up end end
end

local function pick(o)
	local function n(k) return o[k] or 0 end
	return {
		fullDPS = n("FullDPS"), totalDPS = n("TotalDPS"), combinedDPS = n("CombinedDPS"),
		ehp = n("TotalEHP"), life = n("Life"), es = n("EnergyShield"), mana = n("ManaUnreserved"),
		armour = n("Armour"), evasion = n("Evasion"), pdr = n("PhysicalDamageReduction"),
		block = n("EffectiveBlockChance"), spellBlock = n("EffectiveSpellBlockChance"), suppress = n("EffectiveSpellSuppressionChance"),
		physMaxHit = n("PhysicalMaximumHitTaken"),
		eleMaxHit = math.min(n("FireMaximumHitTaken"), n("ColdMaximumHitTaken"), n("LightningMaximumHitTaken")),
		chaosMaxHit = n("ChaosMaximumHitTaken"),
		res = { n("FireResist"), n("ColdResist"), n("LightningResist"), n("ChaosResist") },
		moveSpeed = n("EffectiveMovementSpeedMod"),
		attrShort = math.max(0, n("ReqStr") - n("Str")) + math.max(0, n("ReqDex") - n("Dex")) + math.max(0, n("ReqInt") - n("Int")),
	}
end

local fast = os.getenv("FAST") == "1"
local function score(item)
	local out = {}
	for _, up in ipairs({ true, false }) do
		flasks(up)
		local o
		if fast and item then
			local calcFunc = build.calcsTab:GetMiscCalculator()
			o = calcFunc({ repSlotName = slot, repItem = item })
		else
			if item then equip(item.raw) end
			o = recalc()
		end
		out[up and "up" or "down"] = pick(o)
	end
	return out
end

-- current item in the slot
local curId = it.slots[slot] and it.slots[slot].selItemId
local cur = curId and it.items[curId]
RESULT.slot = slot
RESULT.current = { name = cur and (cur.name or cur.baseName) or "(empty)", raw = cur and cur:BuildRaw() or nil }
RESULT.current.stats = score(nil)
RESULT.mainSkill = (function()
	local g = st.socketGroupList[build.mainSocketGroup]
	return g and (g.displayLabel or g.label) or "?"
end)()
RESULT.items = {}
for i, raw in ipairs(job.items) do
	local ok, res = pcall(function()
		local item = new("Item", raw)
		local unsupported = {}
		for _, list in ipairs({ item.explicitModLines }) do
			for _, ml in ipairs(list) do if ml.extra or #ml.modList == 0 then unsupported[#unsupported + 1] = ml.line end end
		end
		item.raw = raw
		local s = score(item)
		return { stats = s, unsupported = unsupported }
	end)
	RESULT.items[i] = ok and res or { error = tostring(res) }
	if cur and not fast then equip(cur:BuildRaw()) end
end
